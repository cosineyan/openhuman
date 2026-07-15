//! Per-source sync dispatcher.
//!
//! Thin routing layer: dispatches sync requests to the right backend:
//! - GitHub repos → `memory_sync::sources::github`
//! - Composio sources → `memory_sync::composio`
//! - Folder/RSS/WebPage → per-item ingest via reader + ingest pipeline
//! - Twitter → placeholder
//!
//! Sync runs in a `tokio::spawn`-ed task so the RPC returns immediately.
//! Progress is published as `MemorySyncStageChanged` events.
//!
//! A per-source mutex prevents duplicate concurrent syncs when the user
//! presses the sync button multiple times.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::stream::{self, StreamExt};

use crate::openhuman::config::Config;
use crate::openhuman::memory::ingest_pipeline::ingest_document_with_scope;
use crate::openhuman::memory::sync::{emit_sync_stage, MemorySyncStage, MemorySyncTrigger};
use crate::openhuman::memory_sources::readers;
use crate::openhuman::memory_sources::types::{MemorySourceEntry, SourceKind};
use crate::openhuman::memory_sync::canonicalize::document::DocumentInput;
use crate::openhuman::memory_sync::composio::{self, ComposioUsage, SyncReason};

const SYNC_CONCURRENCY: usize = 10;
/// Lower concurrency for M365 mail sources — Graph API enforces MailboxConcurrency
/// limits (typically 4 concurrent requests per mailbox). Using 3 leaves headroom.
const SYNC_CONCURRENCY_MAIL: usize = 3;

static ACTIVE_SYNCS: std::sync::LazyLock<Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));

/// Trigger a sync for one source. Spawns work in the background and
/// returns immediately. Progress is published as `MemorySyncStageChanged`
/// events with `connection_id = Some(source.id)`.
pub async fn sync_source(source: MemorySourceEntry, config: Config) -> Result<(), String> {
    if !source.enabled {
        return Err(format!("source '{}' is disabled", source.id));
    }

    // Per-source mutex: reject if this source is already syncing.
    {
        let mut active = ACTIVE_SYNCS.lock().unwrap_or_else(|e| e.into_inner());
        if !active.insert(source.id.clone()) {
            tracing::debug!(
                source_id = %source.id,
                "[memory_sources:sync] already syncing — skipping duplicate"
            );
            return Ok(());
        }
    }

    let source_id = source.id.clone();
    let kind_str = source.kind.as_str();

    tracing::debug!(
        source_id = %source_id,
        kind = %kind_str,
        "[memory_sources:sync] queueing sync"
    );

    emit_sync_stage(
        MemorySyncTrigger::Manual,
        MemorySyncStage::Requested,
        Some(kind_str),
        Some(&source_id),
        Some(format!("sync requested for {} source", kind_str)),
        Some(&source_id),
    );

    tokio::spawn(async move {
        let source_id_for_panic = source.id.clone();
        let kind_for_panic = source.kind.as_str();
        let inner = tokio::spawn(async move {
            // Retry any previously-failed pipeline jobs so the worker
            // resumes processing through all documents.
            if let Ok(retried) = crate::openhuman::memory_queue::store::retry_all_failed(&config) {
                if retried > 0 {
                    tracing::info!(
                        retried = retried,
                        "[memory_sources:sync] retried {retried} failed pipeline job(s)"
                    );
                }
            }

            tracing::debug!(
                source_id = %source.id,
                kind = %source.kind.as_str(),
                "[memory_sources:sync] dispatching by kind"
            );
            let sync_start = std::time::Instant::now();
            // Composio billable-action usage for this run, populated by
            // `sync_composio` (#3111). Stays zero for non-Composio kinds.
            let mut composio_usage = ComposioUsage::default();
            let outcome = match source.kind {
                SourceKind::Composio => {
                    sync_composio(&source, config.clone(), &mut composio_usage).await
                }
                SourceKind::Conversation => sync_items_individually(&source, &config).await,
                SourceKind::GithubRepo => {
                    // GitHub path writes its own detailed audit entry
                    // with token breakdowns; skip the dispatcher-level
                    // audit for this kind.
                    crate::openhuman::memory_sync::sources::github::run_github_sync(
                        &source, &config,
                    )
                    .await
                    .map(|o| o.records_ingested as usize)
                    .map_err(|e| format!("{e:#}"))
                }
                SourceKind::Folder
                | SourceKind::RssFeed
                | SourceKind::WebPage
                | SourceKind::OutlookMail
                | SourceKind::OutlookCalendar
                | SourceKind::TeamsMessages
                | SourceKind::TeamsTranscript => sync_items_individually(&source, &config).await,
                SourceKind::TwitterQuery => Err(
                    "Twitter sync not yet configured. Provide bearer token in settings."
                        .to_string(),
                ),
            };
            let duration_ms = sync_start.elapsed().as_millis() as u64;

            match outcome {
                Ok(items) => {
                    tracing::debug!(
                        source_id = %source.id,
                        kind = %source.kind.as_str(),
                        items = items,
                        "[memory_sources:sync] completed"
                    );
                    emit_sync_stage(
                        MemorySyncTrigger::Manual,
                        MemorySyncStage::Completed,
                        Some(source.kind.as_str()),
                        Some(&source.id),
                        Some(format!("ingested {items} item(s)")),
                        Some(&source.id),
                    );

                    // Write audit entry (GitHub writes its own with
                    // token detail; other kinds get a simpler entry).
                    if source.kind != SourceKind::GithubRepo {
                        use crate::openhuman::memory_sync::sources::audit::{
                            append_audit_entry, SyncAuditEntry,
                        };
                        append_audit_entry(
                            &config,
                            &SyncAuditEntry {
                                timestamp: chrono::Utc::now(),
                                source_id: source.id.clone(),
                                source_kind: source.kind.as_str().to_string(),
                                scope: source
                                    .url
                                    .clone()
                                    .or(source.toolkit.clone())
                                    .unwrap_or_else(|| source.id.clone()),
                                items_fetched: items as u32,
                                batches: 0,
                                input_tokens: 0,
                                output_tokens: 0,
                                estimated_cost_usd: 0.0,
                                composio_actions_called: composio_usage.actions_called,
                                composio_cost_usd: composio_usage.cost_usd,
                                actual_charged_usd: None,
                                duration_ms,
                                success: true,
                                error: None,
                            },
                        );
                    }

                    // Auto-rebuild: if raw files exist but the tree has
                    // no summaries, build the tree now.
                    check_and_rebuild_tree(&source, &config).await;

                    // Auto-snapshot: capture post-sync state for diff tracking.
                    if let Err(e) = crate::openhuman::memory_diff::ops::auto_snapshot_after_sync(
                        &source, &config,
                    )
                    .await
                    {
                        tracing::warn!(
                            source_id = %source.id,
                            error = %e,
                            "[memory_sources:sync] auto-snapshot failed (non-fatal)"
                        );
                    }
                }
                Err(error) => {
                    // Audit failed syncs too.
                    use crate::openhuman::memory_sync::sources::audit::{
                        append_audit_entry, SyncAuditEntry,
                    };
                    append_audit_entry(
                        &config,
                        &SyncAuditEntry {
                            timestamp: chrono::Utc::now(),
                            source_id: source.id.clone(),
                            source_kind: source.kind.as_str().to_string(),
                            scope: source
                                .url
                                .clone()
                                .or(source.toolkit.clone())
                                .unwrap_or_else(|| source.id.clone()),
                            items_fetched: 0,
                            batches: 0,
                            input_tokens: 0,
                            output_tokens: 0,
                            estimated_cost_usd: 0.0,
                            composio_actions_called: composio_usage.actions_called,
                            composio_cost_usd: composio_usage.cost_usd,
                            actual_charged_usd: None,
                            duration_ms,
                            success: false,
                            error: Some(error.clone()),
                        },
                    );

                    // Report internal failures to Sentry; known-expected
                    // conditions (auth/network/rate-limit/missing config) are
                    // classified by `expected_error_kind` and logged-not-reported
                    // so we surface real bugs without Sentry-spamming routine
                    // user/config errors (#3295). The reason is still shown to
                    // the user via the Failed stage event regardless.
                    crate::core::observability::report_error_or_expected(
                        &error,
                        "memory_sources",
                        "sync",
                        &[
                            ("source_id", source.id.as_str()),
                            ("kind", source.kind.as_str()),
                        ],
                    );

                    emit_sync_stage(
                        MemorySyncTrigger::Manual,
                        MemorySyncStage::Failed,
                        Some(source.kind.as_str()),
                        Some(&source.id),
                        Some(error.clone()),
                        Some(&source.id),
                    );
                    tracing::warn!(
                        source_id = %source.id,
                        kind = %source.kind.as_str(),
                        error = %error,
                        "[memory_sources:sync] failed"
                    );
                }
            }
        });

        if let Err(join_err) = inner.await {
            if join_err.is_panic() {
                tracing::error!(
                    source_id = %source_id_for_panic,
                    kind = %kind_for_panic,
                    "[memory_sources:sync] sync task panicked"
                );
            }
        }

        // Release the per-source lock so future syncs can proceed.
        if let Ok(mut active) = ACTIVE_SYNCS.lock() {
            active.remove(&source_id_for_panic);
        }
    });

    Ok(())
}

async fn sync_composio(
    source: &MemorySourceEntry,
    config: Config,
    usage_out: &mut ComposioUsage,
) -> Result<usize, String> {
    let connection_id = source
        .connection_id
        .as_deref()
        .ok_or("composio source missing connection_id")?;

    emit_sync_stage(
        MemorySyncTrigger::Manual,
        MemorySyncStage::Fetching,
        Some("composio"),
        Some(&source.id),
        Some(format!("delegating to composio sync for {connection_id}")),
        Some(&source.id),
    );

    match composio::run_connection_sync(config, connection_id, SyncReason::Manual).await {
        Ok((outcome, usage)) => {
            *usage_out = usage;
            Ok(outcome.items_ingested)
        }
        Err((e, usage)) => {
            *usage_out = usage;
            Err(format!("composio sync failed: {e}"))
        }
    }
}

/// Per-item sync path for Folder/RSS/WebPage sources.
async fn sync_items_individually(
    source: &MemorySourceEntry,
    config: &Config,
) -> Result<usize, String> {
    let reader = readers::reader_for(&source.kind);

    emit_sync_stage(
        MemorySyncTrigger::Manual,
        MemorySyncStage::Fetching,
        Some(source.kind.as_str()),
        Some(&source.id),
        Some("listing items".to_string()),
        Some(&source.id),
    );

    let items = reader.list_items(source, config).await?;
    let total = items.len();

    if total == 0 {
        return Ok(0);
    }

    emit_sync_stage(
        MemorySyncTrigger::Manual,
        MemorySyncStage::Stored,
        Some(source.kind.as_str()),
        Some(&source.id),
        Some(format!("{total} item(s) discovered")),
        Some(&source.id),
    );

    let ingested = Arc::new(AtomicUsize::new(0));
    let processed = Arc::new(AtomicUsize::new(0));
    let source_id = source.id.clone();
    let source_kind = source.kind.clone();
    let kind_str = source.kind.as_str().to_string();

    stream::iter(items.iter().enumerate())
        .for_each_concurrent(
            match source.kind {
                SourceKind::OutlookMail => SYNC_CONCURRENCY_MAIL,
                _ => SYNC_CONCURRENCY,
            },
            |(_, item)| {
                let config = config.clone();
                let source_kind = source_kind.clone();
                let reader = readers::reader_for(&source_kind);
                let source_clone = source.clone();
                let ingested = Arc::clone(&ingested);
                let processed = Arc::clone(&processed);
                let source_id = source_id.clone();
                let kind_str = kind_str.clone();

                async move {
                    let content = match reader.read_item(&source_clone, &item.id, &config).await {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!(
                                item_id = %item.id,
                                error = %e,
                                "[memory_sources:sync] skipping item — read failed"
                            );
                            processed.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                    };

                    let doc = DocumentInput {
                    provider: format!("memory_sources:{kind_str}"),
                    title: content.title.clone(),
                    // Strip HTML tags so the chunker and LLM see plain text instead
                    // of raw HTML markup. Outlook emails and Teams messages arrive
                    // as HTML; without stripping, chunks are full of <p>, <span>,
                    // style="" noise that obscures the actual content.
                    body: if matches!(
                        content.content_type,
                        crate::openhuman::memory_sources::types::ContentType::Html
                    ) {
                        strip_html(content.body.as_str())
                    } else {
                        content.body.clone()
                    },
                    modified_at: item
                        .updated_at_ms
                        .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms))
                        .unwrap_or_else(chrono::Utc::now),
                    source_ref: Some(format!("{source_id}:{}", item.id)),
                    source_kind_override: match source_kind {
                        SourceKind::OutlookMail => {
                            Some(crate::openhuman::memory_store::chunks::types::SourceKind::Email)
                        }
                        SourceKind::TeamsMessages => {
                            Some(crate::openhuman::memory_store::chunks::types::SourceKind::Chat)
                        }
                        SourceKind::OutlookCalendar => Some(
                            crate::openhuman::memory_store::chunks::types::SourceKind::Calendar,
                        ),
                        SourceKind::TeamsTranscript => Some(
                            crate::openhuman::memory_store::chunks::types::SourceKind::Transcript,
                        ),
                        _ => None,
                    },
                };

                    let composite_source_id = format!("mem_src:{source_id}:{}", item.id);
                    let tags = vec!["memory_sources".to_string(), kind_str.clone()];

                    match ingest_document_with_scope(
                        &config,
                        &composite_source_id,
                        "user",
                        tags,
                        doc,
                        None,
                    )
                    .await
                    {
                        Ok(result) => {
                            if !result.already_ingested {
                                ingested.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                item_id = %item.id,
                                error = %e,
                                "[memory_sources:sync] ingest failed for item"
                            );
                        }
                    }

                    let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
                    let new = ingested.load(Ordering::Relaxed);
                    if done % 10 == 0 || done == total {
                        emit_sync_stage(
                            MemorySyncTrigger::Manual,
                            MemorySyncStage::Ingesting,
                            Some(&kind_str),
                            Some(&source_id),
                            Some(format!("{done}/{total} processed ({new} new)")),
                            Some(&source_id),
                        );
                    }
                }
            },
        )
        .await;

    Ok(ingested.load(Ordering::Relaxed))
}

/// Derive the tree scope(s) for a source and reconcile any raw files that
/// are not yet covered by tree summaries (incremental — see
/// `memory_sync::sources::rebuild`).
pub(crate) async fn check_and_rebuild_tree(source: &MemorySourceEntry, config: &Config) {
    use crate::openhuman::memory_sync::sources::rebuild::{needs_rebuild, rebuild_tree_from_raw};

    let scopes = derive_scopes(source, config);
    for scope in scopes {
        if !needs_rebuild(config, &scope.tree_scope, &scope.archive_source_id) {
            continue;
        }
        tracing::info!(
            source_id = %source.id,
            scope = %scope.tree_scope,
            archive = %scope.archive_source_id,
            "[memory_sources:sync] reconciling uncovered raw files into tree"
        );
        match rebuild_tree_from_raw(config, &scope.tree_scope, &scope.archive_source_id).await {
            Ok(outcome) => {
                tracing::info!(
                    scope = %scope.tree_scope,
                    files = outcome.files_read,
                    batches = outcome.batches,
                    cost = %format!(
                        "${:.4}",
                        outcome.actual_charged_usd.unwrap_or(outcome.estimated_cost_usd)
                    ),
                    cost_is_actual = outcome.actual_charged_usd.is_some(),
                    "[memory_sources:sync] reconcile complete"
                );
            }
            Err(e) => {
                tracing::warn!(
                    scope = %scope.tree_scope,
                    error = %format!("{e:#}"),
                    "[memory_sources:sync] reconcile failed"
                );
            }
        }
    }
}

/// A source's tree scope paired with its raw-archive source id. The two
/// slugify to DIFFERENT directories for GitHub (`github:owner/repo` vs
/// `github.com/owner/repo`) — conflating them makes reconcile scan an
/// empty directory while the real archive sits uncovered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceScope {
    /// Tree registry key, e.g. `"github:owner/repo"`.
    pub tree_scope: String,
    /// Raw-archive id whose slug names `raw/<slug>/`, e.g.
    /// `"github.com/owner/repo"`. Equal to `tree_scope` for sources that
    /// archive under their scope (gmail).
    pub archive_source_id: String,
}

/// Derive the tree scope(s) + raw-archive id(s) that a source maps to.
pub(crate) fn derive_scopes(source: &MemorySourceEntry, config: &Config) -> Vec<SourceScope> {
    use crate::openhuman::memory_sources::readers::github;

    match source.kind {
        SourceKind::GithubRepo => {
            let Some(url) = source.url.as_deref() else {
                return Vec::new();
            };
            match (
                github::repo_chunk_scope(url),
                github::repo_archive_source_id(url),
            ) {
                (Some(tree_scope), Some(archive_source_id)) => vec![SourceScope {
                    tree_scope,
                    archive_source_id,
                }],
                _ => Vec::new(),
            }
        }
        SourceKind::Composio => {
            // Composio sources scope by toolkit + connection email.
            // Gmail: "gmail:<slug_account_email>" — archive dir shares
            // the scope. Others: no raw archive to reconcile yet.
            let toolkit = source.toolkit.as_deref().unwrap_or("unknown");
            match toolkit {
                "gmail" | "GMAIL" => {
                    // The scope for gmail is "gmail:<slugified_email>".
                    // We scan the raw directory to find it.
                    let content_root = config.memory_tree_content_root();
                    let raw_dir = content_root.join("raw");
                    if let Ok(entries) = std::fs::read_dir(&raw_dir) {
                        entries
                            .filter_map(|e| e.ok())
                            .filter(|e| {
                                e.file_name()
                                    .to_str()
                                    .map(|n| n.starts_with("gmail-"))
                                    .unwrap_or(false)
                            })
                            .filter_map(|e| {
                                // Read _source.md to get the scope.
                                let source_md = e.path().join("_source.md");
                                let content = std::fs::read_to_string(&source_md).ok()?;
                                content.lines().find(|l| l.starts_with("scope:")).map(|l| {
                                    let scope = l
                                        .trim_start_matches("scope:")
                                        .trim()
                                        .trim_matches('"')
                                        .to_string();
                                    SourceScope {
                                        tree_scope: scope.clone(),
                                        archive_source_id: scope,
                                    }
                                })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    }
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// Strip HTML tags from a string, preserving meaningful alt/title attributes
/// from emoji, images, and attachments. Used to convert Outlook email bodies
/// and Teams messages to plain text before chunking.
///
/// Special handling:
/// - `<emoji alt="🤐" title="Zipper mouth face">` → `🤐`
/// - `<img alt="screenshot.png">` → `[Image: screenshot.png]`
/// - `<attachment id="...">` (no inner text) → `[Attachment]`
/// - All other tags: stripped, whitespace collapsed
fn strip_html(html: &str) -> String {
    use regex::Regex;

    let mut text = html.to_string();

    // 1. Emoji: extract the alt attribute (the actual emoji character or name)
    // <emoji id="..." alt="🤐" title="Zipper mouth face"> → 🤐
    let emoji_re = Regex::new(r#"(?i)<emoji[^>]*\balt="([^"]*)"[^>]*>"#).unwrap();
    text = emoji_re
        .replace_all(&text, |caps: &regex::Captures| {
            caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string()
        })
        .to_string();

    // 2. Inline images hosted by Graph API — mark with [Image] placeholder.
    // We can't download & describe them here (no async context); callers that
    // want vision descriptions should do so in read_item before strip_html.
    let img_alt_re = Regex::new(r#"(?i)<img[^>]*\balt="([^"]+)"[^>]*/?>"#).unwrap();
    text = img_alt_re
        .replace_all(&text, |caps: &regex::Captures| {
            format!("[Image: {}]", caps.get(1).map(|m| m.as_str()).unwrap_or(""))
        })
        .to_string();
    // Images without alt — just mark as [Image]
    let img_re = Regex::new(r#"(?i)<img[^>]*/?>|<img[^>]*>"#).unwrap();
    text = img_re.replace_all(&text, "[Image]").to_string();

    // 3. Attachments: mark presence
    let attach_re = Regex::new(r#"(?i)<attachment[^>]*>.*?</attachment>"#).unwrap();
    text = attach_re.replace_all(&text, "[Attachment]").to_string();
    let attach_self_re = Regex::new(r#"(?i)<attachment[^>]*/?>|<attachment[^>]*>"#).unwrap();
    text = attach_self_re
        .replace_all(&text, "[Attachment]")
        .to_string();

    // 4. Strip remaining HTML tags, collapse whitespace
    let mut result = String::with_capacity(text.len());
    let mut in_tag = false;
    let mut last_was_space = true;

    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                if !last_was_space && !result.is_empty() {
                    result.push(' ');
                    last_was_space = true;
                }
            }
            _ if !in_tag => {
                if ch.is_whitespace() {
                    if !last_was_space && !result.is_empty() {
                        result.push(' ');
                        last_was_space = true;
                    }
                } else {
                    result.push(ch);
                    last_was_space = false;
                }
            }
            _ => {}
        }
    }

    // Decode common HTML entities
    result
        .trim()
        .to_string()
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}
