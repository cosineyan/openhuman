//! Per-source sync status — chunks ingested, freshness, in-flight progress.
//!
//! Queries `mem_tree_chunks` filtered by source-id prefix:
//! - Reader-backed kinds (folder/github/rss/web/twitter) tag chunks
//!   with `mem_src:{source.id}:%`, so we count those directly.
//! - Composio sources tag chunks with the toolkit-specific id
//!   (e.g. `gmail:user@example.com:msg_xxx`), so we match by toolkit
//!   prefix instead.

use serde::Serialize;

use crate::openhuman::config::Config;
use crate::openhuman::memory_sources::types::{MemorySourceEntry, SourceKind};
use crate::openhuman::memory_store::chunks::store::with_connection;
use crate::openhuman::memory_sync::sources::audit::read_audit_log;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessLabel {
    Active,
    Recent,
    Idle,
}

impl FreshnessLabel {
    pub fn from_age_ms(last_ms: Option<i64>, now_ms: i64) -> Self {
        match last_ms {
            None => Self::Idle,
            Some(ts) => {
                let age = now_ms.saturating_sub(ts);
                if age <= 30_000 {
                    Self::Active
                } else if age <= 5 * 60_000 {
                    Self::Recent
                } else {
                    Self::Idle
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceStatus {
    pub source_id: String,
    pub chunks_synced: u64,
    pub chunks_pending: u64,
    pub last_chunk_at_ms: Option<i64>,
    pub freshness: FreshnessLabel,
}

/// Compute status for one source.
pub async fn source_status(
    config: &Config,
    source: &MemorySourceEntry,
) -> Result<SourceStatus, String> {
    let cfg = config.clone();
    let source_clone = source.clone();

    // Read last successful sync time from audit log — this is more accurate
    // than last chunk timestamp when a source synced successfully but found
    // no new items (e.g. calendar with no new events today).
    let last_sync_ms: Option<i64> = {
        let entries = read_audit_log(config);
        let source_id = &source.id;
        entries
            .iter()
            .filter(|e| e.success && e.source_id == *source_id)
            .filter_map(|e| {
                let ms = e.timestamp.timestamp_millis();
                if ms > 0 {
                    Some(ms)
                } else {
                    None
                }
            })
            .max()
    };

    tokio::task::spawn_blocking(move || {
        with_connection(&cfg, |conn| {
            let prefix = source_id_prefix(&source_clone);

            // Surface real query errors so status telemetry doesn't lie about
            // a healthy zero-row state when the DB is actually broken.
            // NOTE: pending uses the sidecar `mem_tree_chunk_embeddings` table
            // (not the legacy inline `embedding` column which is always NULL
            // since the #1574 migration moved vectors to the sidecar).
            let (synced, pending, last_ts): (i64, i64, Option<i64>) = conn.query_row(
                "SELECT \
                       COUNT(*), \
                       SUM(CASE WHEN NOT EXISTS ( \
                               SELECT 1 FROM mem_tree_chunk_embeddings e \
                               WHERE e.chunk_id = c.id \
                           ) AND c.lifecycle_status != 'dropped' \
                           AND NOT EXISTS ( \
                               SELECT 1 FROM mem_tree_chunk_reembed_skipped s \
                               WHERE s.chunk_id = c.id \
                           ) THEN 1 ELSE 0 END), \
                       MAX(timestamp_ms) \
                     FROM mem_tree_chunks c \
                     WHERE source_id LIKE ?1",
                [&prefix],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        r.get(2)?,
                    ))
                },
            )?;

            let now_ms = chrono::Utc::now().timestamp_millis();

            // Prefer the last successful sync time from the audit log over the
            // last chunk timestamp. When a source syncs successfully but finds
            // no new items (e.g. calendar with no new events), the chunk
            // timestamp stays old but the sync itself just ran — show the sync
            // time so the UI doesn't mislead users into thinking sync is stale.
            let display_ts = last_sync_ms.or(last_ts);
            Ok(SourceStatus {
                source_id: source_clone.id.clone(),
                chunks_synced: synced.max(0) as u64,
                chunks_pending: pending.max(0) as u64,
                last_chunk_at_ms: display_ts,
                freshness: FreshnessLabel::from_age_ms(display_ts, now_ms),
            })
        })
        .map_err(|e| format!("source_status: {e}"))
    })
    .await
    .map_err(|e| format!("source_status join: {e}"))?
}

/// Compute status for all configured sources (one SQL roundtrip per source).
pub async fn status_list(config: &Config) -> Result<Vec<SourceStatus>, String> {
    let sources = crate::openhuman::memory_sources::registry::list_sources().await?;
    let mut out = Vec::with_capacity(sources.len());
    for source in sources {
        match source_status(config, &source).await {
            Ok(s) => out.push(s),
            Err(e) => {
                tracing::warn!(
                    source_id = %source.id,
                    error = %e,
                    "[memory_sources:status] query failed"
                );
                out.push(SourceStatus {
                    source_id: source.id,
                    chunks_synced: 0,
                    chunks_pending: 0,
                    last_chunk_at_ms: None,
                    freshness: FreshnessLabel::Idle,
                });
            }
        }
    }
    Ok(out)
}

/// Build the `source_id LIKE` prefix that matches chunks belonging to a source.
fn source_id_prefix(source: &MemorySourceEntry) -> String {
    match source.kind {
        SourceKind::Composio => {
            // Composio providers write chunks with source_id = `{toolkit}:%`
            // (e.g. `gmail:user@example.com:msg_xxx`). Match by toolkit only.
            source
                .toolkit
                .as_deref()
                .map(|t| format!("{t}:%"))
                .unwrap_or_else(|| "__no_toolkit__:%".to_string())
        }
        _ => format!("mem_src:{}:%", source.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_thresholds() {
        let now = 1_000_000_000_000;
        assert_eq!(
            FreshnessLabel::from_age_ms(Some(now - 1_000), now),
            FreshnessLabel::Active
        );
        assert_eq!(
            FreshnessLabel::from_age_ms(Some(now - 60_000), now),
            FreshnessLabel::Recent
        );
        assert_eq!(
            FreshnessLabel::from_age_ms(Some(now - 600_000), now),
            FreshnessLabel::Idle
        );
        assert_eq!(FreshnessLabel::from_age_ms(None, now), FreshnessLabel::Idle);
    }

    #[test]
    fn source_id_prefix_dispatch() {
        let mut entry = MemorySourceEntry {
            id: "src_abc".into(),
            kind: SourceKind::Folder,
            label: "x".into(),
            enabled: true,
            toolkit: None,
            connection_id: None,
            path: Some("/tmp".into()),
            glob: None,
            url: None,
            branch: None,
            paths: Vec::new(),
            query: None,
            since_days: None,
            max_items: None,
            max_commits: None,
            max_issues: None,
            max_prs: None,
            selector: None,
            max_tokens_per_sync: None,
            max_cost_per_sync_usd: None,
            sync_depth_days: None,
        };
        assert_eq!(source_id_prefix(&entry), "mem_src:src_abc:%");

        entry.kind = SourceKind::Composio;
        entry.toolkit = Some("gmail".into());
        assert_eq!(source_id_prefix(&entry), "gmail:%");
    }
}
