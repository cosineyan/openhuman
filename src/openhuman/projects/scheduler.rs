//! Concurrency-aware dispatcher for AI project tasks.
//!
//! Replaces "assign = immediate spawn" with "assign = enqueue; a slot-aware
//! dispatcher spawns". [`try_dispatch`] is the ONLY place that moves a To Do
//! task into Doing + spawns the run. It is invoked from three triggers, all
//! serialized by [`DISPATCH_LOCK`] so no combination can over-dispatch:
//!   1. the `ProjectTaskAssignedToAi` event (nudge on assign),
//!   2. the `ProjectTaskCompleted` event (a slot just freed),
//!   3. a periodic backstop poller ([`start_throttle_poller`]).
//!
//! Throttle bucket = the START `(profile_id, tier)` the task was assigned with
//! (never the model a fallback later switches to). A task with no profile runs
//! in an unlimited bucket. Limits live in `claude_profiles` (per profile+tier).

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::openhuman::claude_profiles::types::ThrottleLimit;
use crate::openhuman::config::Config;
use crate::openhuman::projects::run_registry::ThrottleKey;
use crate::openhuman::projects::{store, Bucket, Task};

const LOG: &str = "[projects::scheduler]";
const POLLER_TICK_SECONDS: u64 = 5;

/// Serializes the scan→count→select→reserve critical section so the poller,
/// the assign event, and the completion event can never over-dispatch.
static DISPATCH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static POLLER_STARTED: OnceLock<()> = OnceLock::new();

/// Derive a task's throttle bucket from its START (profile, tier).
/// - profile + tier alias → `(profile, alias)`
/// - profile + concrete model id → `(profile, "default")`
/// - no profile → `None` (unlimited)
pub fn throttle_key_for_task(task: &Task) -> Option<ThrottleKey> {
    let profile = task
        .settings_profile
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let tier = match task.model.as_deref().map(|m| m.trim().to_ascii_lowercase()) {
        Some(t) if matches!(t.as_str(), "opus" | "sonnet" | "haiku" | "default") => t,
        _ => "default".to_string(),
    };
    Some((profile.to_string(), tier))
}

/// Is this bucket a "To Do" (non-done) bucket?
fn is_todo_bucket(b: &Bucket) -> bool {
    !b.is_done_bucket && b.title.to_lowercase().contains("to do")
}

/// A candidate task eligible for dispatch, with its throttle key precomputed.
struct Candidate {
    task: Task,
    key: Option<ThrottleKey>,
}

/// Pure selection logic (unit-testable, no DB / no spawn). Given candidates,
/// the configured limits, and current per-key running counts, return the task
/// ids to dispatch this pass. Unlimited-key (None) candidates are all selected;
/// keyed candidates are selected up to `limit - running`, highest priority first
/// (tie-break: earlier `created`, then lower `position`).
fn select_dispatchable(
    mut candidates: Vec<CandidateLite>,
    limits: &[ThrottleLimit],
    running: &std::collections::HashMap<ThrottleKey, u32>,
) -> Vec<String> {
    // Sort globally by priority desc, created asc, position asc — deterministic.
    candidates.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then(a.created.cmp(&b.created))
            .then(
                a.position
                    .partial_cmp(&b.position)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let limit_for = |key: &ThrottleKey| -> Option<u32> {
        limits
            .iter()
            .find(|l| &(l.profile_id.clone(), l.tier.clone()) == key)
            .map(|l| l.limit)
    };

    // Track how many more we can start per key this pass (limit - already running).
    let mut remaining: std::collections::HashMap<ThrottleKey, i64> =
        std::collections::HashMap::new();
    let mut out = Vec::new();
    for c in candidates {
        match &c.key {
            None => out.push(c.id), // unlimited
            Some(key) => {
                let cap = match limit_for(key) {
                    None => {
                        out.push(c.id); // no limit configured → unlimited
                        continue;
                    }
                    Some(l) => l as i64,
                };
                let rem = remaining
                    .entry(key.clone())
                    .or_insert_with(|| cap - *running.get(key).unwrap_or(&0) as i64);
                if *rem > 0 {
                    *rem -= 1;
                    out.push(c.id);
                }
            }
        }
    }
    out
}

/// Lightweight candidate used by the pure selector (avoids cloning full Task).
struct CandidateLite {
    id: String,
    key: Option<ThrottleKey>,
    priority: i64,
    created: chrono::DateTime<chrono::Utc>,
    position: f64,
}

/// The single dispatch entry point. Scans all boards for AI To Do tasks,
/// respects per-(profile,tier) limits + priority, and spawns the selected ones.
pub async fn try_dispatch(config: Arc<Config>) {
    let _guard = DISPATCH_LOCK.lock().await;

    // Load throttle limits once for this pass.
    let limits = crate::openhuman::claude_profiles::ops::get_throttles(&config);

    // 1. Collect candidates: AI-assigned tasks in a To Do bucket, not already running.
    let mut candidates: Vec<Candidate> = Vec::new();
    let project_ids = match store::list_project_ids(&config) {
        Ok(ids) => ids,
        Err(e) => {
            log::warn!("{LOG} list_project_ids failed: {e}");
            return;
        }
    };
    for pid in &project_ids {
        let buckets = match store::list_buckets(&config, pid) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let todo_ids: std::collections::HashSet<String> = buckets
            .iter()
            .filter(|b| is_todo_bucket(b))
            .map(|b| b.id.clone())
            .collect();
        if todo_ids.is_empty() {
            continue;
        }
        let tasks = match store::list_tasks(&config, pid, None) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for task in tasks {
            if task.assignee.as_deref() != Some("ai") {
                continue;
            }
            if !todo_ids.contains(&task.bucket_id) {
                continue;
            }
            if crate::openhuman::projects::run_registry::is_running(&task.id) {
                continue; // already dispatched / reserved
            }
            let key = throttle_key_for_task(&task);
            candidates.push(Candidate { task, key });
        }
    }
    if candidates.is_empty() {
        return;
    }

    // 2. Snapshot current running counts per key (under the lock, so stable).
    let mut running: std::collections::HashMap<ThrottleKey, u32> = std::collections::HashMap::new();
    for c in &candidates {
        if let Some(key) = &c.key {
            running
                .entry(key.clone())
                .or_insert_with(|| crate::openhuman::projects::run_registry::count_running(key));
        }
    }

    // 3. Pure selection.
    let lite: Vec<CandidateLite> = candidates
        .iter()
        .map(|c| CandidateLite {
            id: c.task.id.clone(),
            key: c.key.clone(),
            priority: c.task.priority,
            created: c.task.created,
            position: c.task.position,
        })
        .collect();
    let selected_ids = select_dispatchable(lite, &limits, &running);
    if selected_ids.is_empty() {
        return;
    }
    let selected: std::collections::HashSet<String> = selected_ids.into_iter().collect();

    // 4. Reserve (register) + spawn each selected task, still under the lock so
    //    count_running reflects reservations for any concurrent pass.
    for c in candidates
        .into_iter()
        .filter(|c| selected.contains(&c.task.id))
    {
        let buckets = match store::list_buckets(&config, &c.task.project_id) {
            Ok(b) => b,
            Err(_) => continue,
        };
        crate::openhuman::projects::bus::spawn_run(Arc::clone(&config), c.task, buckets, c.key);
    }
}

/// Start the periodic backstop poller (idempotent). Skips its immediate tick so
/// startup stale-task cleanup runs first.
pub fn start_throttle_poller(config: Arc<Config>) {
    if POLLER_STARTED.set(()).is_err() {
        return;
    }
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(POLLER_TICK_SECONDS));
        ticker.tick().await; // consume the immediate first tick
        loop {
            ticker.tick().await;
            try_dispatch(Arc::clone(&config)).await;
        }
    });
    log::debug!("{LOG} throttle poller started");
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn cand(id: &str, key: Option<(&str, &str)>, priority: i64, secs: i64) -> CandidateLite {
        CandidateLite {
            id: id.to_string(),
            key: key.map(|(p, t)| (p.to_string(), t.to_string())),
            priority,
            created: chrono::Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap(),
            position: secs as f64,
        }
    }
    fn lim(p: &str, t: &str, l: u32) -> ThrottleLimit {
        ThrottleLimit {
            profile_id: p.into(),
            tier: t.into(),
            limit: l,
        }
    }

    #[test]
    fn worked_example_1_unlimited_plus_top3_of_4() {
        // 1 unlimited (hyperspace) + 4 local:default (limit 3), none running.
        let limits = vec![lim("local", "default", 3)];
        let running = std::collections::HashMap::new();
        let cands = vec![
            cand("hyper", None, 0, 0),
            cand("l1", Some(("local", "default")), 5, 1),
            cand("l2", Some(("local", "default")), 4, 2),
            cand("l3", Some(("local", "default")), 3, 3),
            cand("l4", Some(("local", "default")), 1, 4), // lowest priority → withheld
        ];
        let out = select_dispatchable(cands, &limits, &running);
        assert!(out.contains(&"hyper".to_string()));
        assert!(out.contains(&"l1".to_string()));
        assert!(out.contains(&"l2".to_string()));
        assert!(out.contains(&"l3".to_string()));
        assert!(!out.contains(&"l4".to_string())); // 4th, lowest priority, held back
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn respects_already_running_count() {
        // limit 3, 2 already running → only 1 more slot.
        let limits = vec![lim("local", "default", 3)];
        let mut running = std::collections::HashMap::new();
        running.insert(("local".to_string(), "default".to_string()), 2u32);
        let cands = vec![
            cand("a", Some(("local", "default")), 9, 1),
            cand("b", Some(("local", "default")), 8, 2),
        ];
        let out = select_dispatchable(cands, &limits, &running);
        assert_eq!(out, vec!["a".to_string()]); // only the top-priority one fits
    }

    #[test]
    fn no_limit_row_means_unlimited() {
        let limits: Vec<ThrottleLimit> = vec![]; // nothing configured
        let running = std::collections::HashMap::new();
        let cands = vec![
            cand("a", Some(("local", "default")), 1, 1),
            cand("b", Some(("local", "default")), 2, 2),
            cand("c", Some(("local", "default")), 3, 3),
        ];
        let out = select_dispatchable(cands, &limits, &running);
        assert_eq!(out.len(), 3); // all dispatched
    }

    #[test]
    fn full_bucket_dispatches_none() {
        let limits = vec![lim("local", "default", 2)];
        let mut running = std::collections::HashMap::new();
        running.insert(("local".to_string(), "default".to_string()), 2u32);
        let cands = vec![cand("a", Some(("local", "default")), 9, 1)];
        assert!(select_dispatchable(cands, &limits, &running).is_empty());
    }
}
