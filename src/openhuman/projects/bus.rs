//! Event-bus subscriber that picks up project tasks assigned to AI.
//!
//! When a `ProjectTaskAssignedToAi` event fires, this module:
//! 1. Verifies the task is still in a "To Do" (non-done, non-in-progress) bucket.
//! 2. Moves the task to the "Doing" bucket (In Progress).
//! 3. Runs the AI using the task title + description as prompt.
//! 4. On success: posts result as a comment, moves to "Done", uploads AI log.
//! 5. On failure: posts error as a comment, moves to "Blocked", uploads AI log.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use chrono::Utc;

use crate::core::event_bus::{DomainEvent, EventHandler, SubscriptionHandle};
use crate::openhuman::config::Config;
use crate::openhuman::projects::{store, TaskPatch};

static AI_RUNNER_HANDLE: OnceLock<SubscriptionHandle> = OnceLock::new();

const LOG: &str = "[projects::ai_runner]";

fn emit_task_log(task_id: &str, line: &str, kind: &str) {
    use crate::core::socketio::WebChannelEvent;
    use crate::openhuman::channels::providers::web::publish_web_channel_event;
    use serde_json::json;

    let payload = json!({ "task_id": task_id, "line": line, "kind": kind });
    publish_web_channel_event(WebChannelEvent {
        event: "project:task_log".to_string(),
        client_id: "system".to_string(),
        thread_id: format!("project-task-{task_id}"),
        request_id: String::new(),
        message: Some(line.to_string()),
        output: Some(payload.to_string()),
        ..WebChannelEvent::default()
    });
}

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

        let cancel_token = tokio_util::sync::CancellationToken::new();
        let cancel_token_task = cancel_token.clone();
        crate::openhuman::projects::run_registry::register(&task_id, cancel_token);
        tokio::spawn(async move {
            run_ai_task(
                config,
                task_id,
                project_id,
                title,
                description,
                buckets,
                cancel_token_task,
            )
            .await;
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
    cancel_token: tokio_util::sync::CancellationToken,
) {
    let started_at = Utc::now();
    log::debug!("{LOG} picking up task={task_id} title={title:?}");

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
            crate::openhuman::projects::run_registry::deregister(&task_id);
            return;
        }
    };

    let patch_doing = TaskPatch {
        bucket_id: Some(doing_id.clone()),
        ..TaskPatch::default()
    };
    if let Err(e) = store::update_task(&config, &task_id, &patch_doing, "ai") {
        log::error!("{LOG} task={task_id} failed to move to Doing: {e}");
        crate::openhuman::projects::run_registry::deregister(&task_id);
        return;
    }
    let _ = store::add_comment(&config, &task_id, "ai", "Starting to work on this task…");
    emit_task_log(&task_id, "Starting to work on this task…", "log");

    // ── 2. Build prompt ───────────────────────────────────────────────────
    let prompt = build_prompt(&title, description.as_deref());

    // ── 3. Run AI ─────────────────────────────────────────────────────────
    let outcome = tokio::select! {
        result = run_agent(&config, &task_id, &prompt) => result,
        _ = cancel_token.cancelled() => Err("Cancelled by user.".to_string()),
    };
    let finished_at = Utc::now();

    // ── 4. Write back ─────────────────────────────────────────────────────
    let was_cancelled = matches!(&outcome, Err(msg) if msg == "Cancelled by user.");

    let (status, response_text) = if was_cancelled {
        let comment = "Cancelled by user.";
        let _ = store::add_comment(&config, &task_id, "ai", comment);
        emit_task_log(&task_id, comment, "cancelled");
        if let Some(id) = find_bucket("block") {
            let patch = TaskPatch {
                bucket_id: Some(id),
                ..TaskPatch::default()
            };
            if let Err(e) = store::update_task(&config, &task_id, &patch, "ai") {
                log::error!("{LOG} task={task_id} failed to move to Blocked after cancel: {e}");
            }
        } else {
            log::warn!("{LOG} task={task_id} no Blocked bucket — task stays in Doing");
        }
        ("cancelled", comment)
    } else {
        match &outcome {
            Ok(response) => {
                let _ = store::add_comment(&config, &task_id, "ai", response);
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
                    if let Err(e) = store::update_task(&config, &task_id, &patch, "ai") {
                        log::error!("{LOG} task={task_id} failed to move to Done: {e}");
                    } else {
                        log::debug!("{LOG} task={task_id} moved to Done");
                    }
                } else {
                    log::warn!("{LOG} task={task_id} no Done bucket found — task stays in Doing");
                }
                emit_task_log(&task_id, response, "done");
                ("done", response.as_str())
            }
            Err(err_msg) => {
                log::warn!("{LOG} task={task_id} AI failed: {err_msg}");
                let comment = format!("Encountered an issue:\n\n{err_msg}");
                let _ = store::add_comment(&config, &task_id, "ai", &comment);
                emit_task_log(&task_id, &comment, "error");
                if let Some(id) = find_bucket("block") {
                    let patch = TaskPatch {
                        bucket_id: Some(id),
                        ..TaskPatch::default()
                    };
                    if let Err(e) = store::update_task(&config, &task_id, &patch, "ai") {
                        log::error!("{LOG} task={task_id} failed to move to Blocked: {e}");
                    } else {
                        log::debug!("{LOG} task={task_id} moved to Blocked");
                    }
                } else {
                    log::warn!("{LOG} task={task_id} no Blocked bucket — task stays in Doing");
                }
                ("blocked", err_msg.as_str())
            }
        }
    };

    // ── 5. Write and attach AI log ────────────────────────────────────────
    upload_ai_log(
        &config,
        &task_id,
        &title,
        description.as_deref(),
        &prompt,
        status,
        response_text,
        started_at,
        finished_at,
    );

    crate::openhuman::projects::run_registry::deregister(&task_id);
    log::debug!("{LOG} task={task_id} complete (status={status})");
    let _ = project_id;
}

// ---------------------------------------------------------------------------
// AI log file
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn upload_ai_log(
    config: &Config,
    task_id: &str,
    title: &str,
    description: Option<&str>,
    prompt: &str,
    status: &str,
    response: &str,
    started_at: chrono::DateTime<Utc>,
    finished_at: chrono::DateTime<Utc>,
) {
    let timestamp = started_at.format("%Y%m%d_%H%M%S");
    let filename = format!("ai-log-{timestamp}.md");
    let duration_secs = (finished_at - started_at).num_seconds();

    let mut md = String::new();
    md.push_str(&format!("# AI Task Log — {title}\n\n"));
    md.push_str(&format!("| Field | Value |\n|-------|-------|\n"));
    md.push_str(&format!("| Task ID | `{task_id}` |\n"));
    md.push_str(&format!("| Status | **{status}** |\n"));
    md.push_str(&format!(
        "| Started | {} |\n",
        started_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));
    md.push_str(&format!(
        "| Finished | {} |\n",
        finished_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));
    md.push_str(&format!("| Duration | {duration_secs}s |\n\n"));

    md.push_str("## Task\n\n");
    md.push_str(&format!("**Title:** {title}\n\n"));
    if let Some(desc) = description.filter(|d| !d.trim().is_empty()) {
        md.push_str(&format!("**Description:**\n\n{desc}\n\n"));
    }

    md.push_str("## Prompt Sent to AI\n\n");
    md.push_str("```\n");
    md.push_str(prompt);
    md.push_str("\n```\n\n");

    md.push_str(&format!("## AI Response ({})\n\n", status.to_uppercase()));
    md.push_str(response);
    md.push('\n');

    // Write to system temp dir
    let tmp_path = std::env::temp_dir().join(&filename);
    if let Err(e) = std::fs::write(&tmp_path, &md) {
        log::error!("{LOG} task={task_id} failed to write log file: {e}");
        return;
    }

    // Attach to the task
    match store::add_attachment(config, task_id, &tmp_path, "ai") {
        Ok(att) => {
            log::debug!(
                "{LOG} task={task_id} uploaded log as attachment id={}",
                att.id
            );
        }
        Err(e) => {
            log::error!("{LOG} task={task_id} failed to attach log: {e}");
        }
    }

    // Clean up temp file
    let _ = std::fs::remove_file(&tmp_path);
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

    // Use a unique agent definition name so the transcript loader finds no
    // prior session file and history starts clean. Without this the turn()
    // call re-loads a previous transcript (which may start with an assistant
    // message), causing "Expected message role 'user', got 'assistant'" from
    // the Claude API.
    let run_id = uuid::Uuid::new_v4().to_string();
    let run_name = format!("project-task-runner-{run_id}");
    agent.set_agent_definition_name(&run_name);
    agent.set_event_context(&format!("project-task-{task_id}-{run_id}"), "background");

    log::debug!("{LOG} task={task_id} running agent turn (agent_name={run_name})");
    agent.run_single(prompt).await.map_err(|e| e.to_string())
}
