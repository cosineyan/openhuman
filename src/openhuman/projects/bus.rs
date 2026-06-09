//! Event-bus subscriber that picks up project tasks assigned to AI.
//!
//! When a `ProjectTaskAssignedToAi` event fires, this module:
//! 1. Verifies the task is still in a "To Do" (non-done, non-in-progress) bucket.
//! 2. Moves the task to the "Doing" bucket (In Progress).
//! 3. Runs the AI using the task title + description as prompt.
//! 4. On success: posts result as a comment and moves task to "Done".
//! 5. On failure: posts error as a comment and moves task to "Blocked".

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use crate::core::event_bus::{DomainEvent, EventHandler, SubscriptionHandle};
use crate::openhuman::config::Config;
use crate::openhuman::projects::{store, TaskPatch};

static AI_RUNNER_HANDLE: OnceLock<SubscriptionHandle> = OnceLock::new();

const LOG: &str = "[projects::ai_runner]";

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register the project AI task runner subscriber. Idempotent.
pub fn register_project_ai_runner(config: Arc<Config>) {
    if AI_RUNNER_HANDLE.get().is_some() {
        return;
    }
    let sub = Arc::new(ProjectAiRunner { config });
    match crate::core::event_bus::subscribe_global(sub) {
        Some(handle) => {
            let _ = AI_RUNNER_HANDLE.set(handle);
            log::debug!("{LOG} registered");
        }
        None => {
            log::warn!("{LOG} failed to register — event bus not initialized");
        }
    }
}

// ---------------------------------------------------------------------------
// EventHandler
// ---------------------------------------------------------------------------

struct ProjectAiRunner {
    config: Arc<Config>,
}

#[async_trait]
impl EventHandler for ProjectAiRunner {
    fn name(&self) -> &str {
        "projects::ai_runner"
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::ProjectTaskAssignedToAi {
            task_id,
            project_id,
            bucket_id,
            title,
            description,
        } = event
        else {
            return;
        };

        // Only process tasks in a "To Do" bucket (not already in progress / done / blocked).
        // We check is_done_bucket=false and bucket title contains "to do" (case-insensitive).
        let buckets = match store::list_buckets(&self.config, project_id) {
            Ok(b) => b,
            Err(e) => {
                log::error!("{LOG} list_buckets failed: {e}");
                return;
            }
        };

        let current_bucket = buckets.iter().find(|b| &b.id == bucket_id);
        let is_todo = current_bucket
            .map(|b| !b.is_done_bucket && b.title.to_lowercase().contains("to do"))
            .unwrap_or(false);

        if !is_todo {
            log::debug!(
                "{LOG} task={task_id} bucket='{}' is not a To Do bucket — skipping",
                current_bucket.map_or("?", |b| &b.title)
            );
            return;
        }

        let config = Arc::clone(&self.config);
        let task_id = task_id.clone();
        let project_id = project_id.clone();
        let title = title.clone();
        let description = description.clone();
        let buckets = buckets;

        // Spawn detached so we don't block the event bus dispatcher.
        tokio::spawn(async move {
            run_ai_task(config, task_id, project_id, title, description, buckets).await;
        });
    }
}

// ---------------------------------------------------------------------------
// Core execution
// ---------------------------------------------------------------------------

async fn run_ai_task(
    config: Arc<Config>,
    task_id: String,
    project_id: String,
    title: String,
    description: Option<String>,
    buckets: Vec<crate::openhuman::projects::Bucket>,
) {
    log::debug!("{LOG} picking up task={task_id} title={title:?}");

    // Helper: find bucket id by title fragment (case-insensitive).
    let find_bucket = |fragment: &str| -> Option<String> {
        buckets
            .iter()
            .find(|b| b.title.to_lowercase().contains(fragment))
            .map(|b| b.id.clone())
    };

    // ── 1. Move to Doing ──────────────────────────────────────────────────
    let doing_id = match find_bucket("doing").or_else(|| find_bucket("in progress")) {
        Some(id) => id,
        None => {
            log::warn!("{LOG} task={task_id} no 'Doing' bucket found — aborting");
            return;
        }
    };

    let patch_doing = TaskPatch {
        bucket_id: Some(doing_id.clone()),
        ..TaskPatch::default()
    };
    if let Err(e) = store::update_task(&config, &task_id, &patch_doing, "ai") {
        log::error!("{LOG} task={task_id} failed to move to Doing: {e}");
        return;
    }
    log::debug!("{LOG} task={task_id} moved to Doing");

    // Also log a comment so the user knows it started.
    let _ = store::add_comment(&config, &task_id, "ai", "Starting to work on this task…");

    // ── 2. Build prompt ───────────────────────────────────────────────────
    let prompt = build_prompt(&title, description.as_deref());

    // ── 3. Run AI ─────────────────────────────────────────────────────────
    let outcome = run_agent(&config, &task_id, &prompt).await;

    // ── 4. Write back ─────────────────────────────────────────────────────
    match outcome {
        Ok(response) => {
            log::debug!("{LOG} task={task_id} AI succeeded");
            // Post result as a comment.
            let _ = store::add_comment(&config, &task_id, "ai", &response);
            // Move to Done.
            let done_id = buckets
                .iter()
                .find(|b| b.is_done_bucket)
                .map(|b| b.id.clone())
                .or_else(|| find_bucket("done"));
            if let Some(id) = done_id {
                let patch = TaskPatch {
                    bucket_id: Some(id),
                    ..TaskPatch::default()
                };
                let _ = store::update_task(&config, &task_id, &patch, "ai");
            }
        }
        Err(err_msg) => {
            log::warn!("{LOG} task={task_id} AI failed: {err_msg}");
            // Post error as a comment.
            let comment = format!("Encountered an issue:\n\n{err_msg}");
            let _ = store::add_comment(&config, &task_id, "ai", &comment);
            // Move to Blocked.
            if let Some(id) = find_bucket("block") {
                let patch = TaskPatch {
                    bucket_id: Some(id),
                    ..TaskPatch::default()
                };
                let _ = store::update_task(&config, &task_id, &patch, "ai");
            }
        }
    }

    let _ = project_id; // suppress unused warning
    log::debug!("{LOG} task={task_id} complete");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_prompt(title: &str, description: Option<&str>) -> String {
    let mut prompt = format!(
        "You are an AI agent processing a task in a project management system.\n\n\
         Task: {title}"
    );
    if let Some(desc) = description.filter(|d| !d.trim().is_empty()) {
        prompt.push_str(&format!("\nDescription: {desc}"));
    }
    prompt.push_str(
        "\n\nPlease complete this task and provide your result. \
         Be concise and actionable. Respond with the outcome directly.",
    );
    prompt
}

async fn run_agent(config: &Config, task_id: &str, prompt: &str) -> Result<String, String> {
    log::debug!("{LOG} task={task_id} building agent");

    let mut agent = crate::openhuman::agent::harness::session::Agent::from_config(config)
        .map_err(|e| format!("failed to build agent: {e}"))?;

    agent.set_event_context(&format!("project-task-{task_id}"), "background");

    log::debug!("{LOG} task={task_id} running agent turn");
    agent.run_single(prompt).await.map_err(|e| e.to_string())
}
