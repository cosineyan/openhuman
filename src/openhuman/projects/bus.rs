//! Event-bus subscriber that picks up project tasks assigned to AI.
//!
//! When a `ProjectTaskAssignedToAi` event fires, this module:
//! 1. Verifies the task is still in a "To Do" (non-done, non-in-progress) bucket.
//! 2. Moves the task to the "Doing" bucket (In Progress).
//! 3. Runs the AI using the task title + description as prompt.
//! 4. On success: posts result as a comment, moves to "Done", uploads AI log.
//! 5. On failure (hard error or AI self-reports "BLOCKED: …"): posts comment, moves to "Blocked".

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

    // Check if there's a previous claude session to resume. If the task has
    // been run before and has a valid session UUID in ai_plan, pass it as
    // hint_thread_id so the driver uses --resume instead of starting fresh.
    // This lets claude see the full prior conversation history.
    let existing_session_id: Option<String> = store::get_task(&config, &task_id)
        .ok()
        .and_then(|t| t.ai_plan)
        .and_then(|plan| serde_json::from_str::<serde_json::Value>(&plan).ok())
        .and_then(|v| {
            v.get("claude_session_id")
                .and_then(|s| s.as_str())
                .map(str::to_string)
        })
        .filter(|id| {
            crate::openhuman::inference::provider::claude_code::session_store::is_uuid_v4(id)
        });

    // Use existing session if available (resume), otherwise generate a new hint UUID.
    let cc_session_uuid = existing_session_id.unwrap_or_else(
        crate::openhuman::inference::provider::claude_code::session_store::generate_uuid_v4,
    );
    // claude CLI saves the session under the cwd it was launched with,
    // which is action_dir (the user's project root). The resume card must
    // cd there before running --resume, so store action_dir here.
    let cc_workspace_dir = config.action_dir.display().to_string();

    // ── 3. Run AI ─────────────────────────────────────────────────────────
    let (outcome, fwd, actual_session_id) = tokio::select! {
        result = run_agent(&config, &task_id, &prompt, &cc_session_uuid) => result,
        _ = cancel_token.cancelled() => {
            (Err("Cancelled by user.".to_string()), tokio::spawn(async {}), None)
        }
    };
    // Use the real claude session UUID if captured; otherwise fall back to the hint key.
    let claude_resume_uuid = actual_session_id.unwrap_or_else(|| cc_session_uuid.clone());
    let was_cancelled = matches!(&outcome, Err(msg) if msg == "Cancelled by user.");
    // On cancellation abort the forwarder immediately so it doesn't emit stale
    // log lines after the task has already moved to Blocked. On the normal path,
    // just await it — the channel is already closed (agent dropped) so it drains
    // instantly without needing an abort.
    if was_cancelled {
        fwd.abort();
    }
    if let Err(e) = fwd.await {
        if !was_cancelled {
            log::warn!("{LOG} task={task_id} progress forwarder error: {e:?}");
        }
    }
    let finished_at = Utc::now();

    let (status, response_text) = if was_cancelled {
        let comment = "Cancelled by user.";
        let _ = store::add_comment(&config, &task_id, "ai", comment);
        emit_task_log(&task_id, comment, "cancelled");
        // Persist the pre-generated CC session UUID so the UI can offer resume.
        let plan = serde_json::json!({
            "claude_session_id": claude_resume_uuid,
            "claude_workspace_dir": cc_workspace_dir,
        })
        .to_string();
        if let Err(e) = store::update_task(
            &config,
            &task_id,
            &crate::openhuman::projects::TaskPatch {
                ai_plan: Some(plan),
                ..crate::openhuman::projects::TaskPatch::default()
            },
            "ai",
        ) {
            log::warn!("{LOG} task={task_id} failed to write ai_plan: {e}");
        }
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
                log::info!(
                    "{LOG} task={task_id} AI response received chars={} first_100={:?}",
                    response.len(),
                    response.chars().take(100).collect::<String>()
                );
                let _ = store::add_comment(&config, &task_id, "ai", response);
                // Persist the pre-generated CC session UUID so the UI can offer resume.
                let plan = serde_json::json!({
                    "claude_session_id": claude_resume_uuid,
                    "claude_workspace_dir": cc_workspace_dir,
                })
                .to_string();
                if let Err(e) = store::update_task(
                    &config,
                    &task_id,
                    &crate::openhuman::projects::TaskPatch {
                        ai_plan: Some(plan),
                        ..crate::openhuman::projects::TaskPatch::default()
                    },
                    "ai",
                ) {
                    log::warn!("{LOG} task={task_id} failed to write ai_plan: {e}");
                }
                // Check if the response contains the BLOCKED: marker anywhere.
                // claude CLI may emit the marker mid-paragraph without a leading
                // newline, so scanning line starts is insufficient.
                let is_blocked = response.contains("BLOCKED:");
                if is_blocked {
                    log::warn!("{LOG} task={task_id} AI self-reported blocked: {response}");
                    if let Some(id) = find_bucket("block") {
                        let patch = TaskPatch {
                            bucket_id: Some(id),
                            ..TaskPatch::default()
                        };
                        if let Err(e) = store::update_task(&config, &task_id, &patch, "ai") {
                            log::error!("{LOG} task={task_id} failed to move to Blocked: {e}");
                        } else {
                            log::debug!("{LOG} task={task_id} moved to Blocked (self-reported)");
                        }
                    } else {
                        log::warn!("{LOG} task={task_id} no Blocked bucket — task stays in Doing");
                    }
                    emit_task_log(&task_id, response, "blocked");
                    notify_teams_chat(
                        &task_id,
                        &title,
                        "blocked",
                        &cc_workspace_dir,
                        &claude_resume_uuid,
                    );
                    ("blocked", response.as_str())
                } else {
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
                        log::warn!(
                            "{LOG} task={task_id} no Done bucket found — task stays in Doing"
                        );
                    }
                    emit_task_log(&task_id, response, "done");
                    notify_teams_chat(
                        &task_id,
                        &title,
                        "done",
                        &cc_workspace_dir,
                        &claude_resume_uuid,
                    );
                    ("done", response.as_str())
                }
            }
            Err(err_msg) => {
                log::warn!("{LOG} task={task_id} AI failed: {err_msg}");
                let comment = format!("Encountered an issue:\n\n{err_msg}");
                let _ = store::add_comment(&config, &task_id, "ai", &comment);
                emit_task_log(&task_id, &comment, "error");
                // Persist the pre-generated CC session UUID so the UI can offer resume.
                let plan = serde_json::json!({
                    "claude_session_id": claude_resume_uuid,
                    "claude_workspace_dir": cc_workspace_dir,
                })
                .to_string();
                if let Err(e) = store::update_task(
                    &config,
                    &task_id,
                    &crate::openhuman::projects::TaskPatch {
                        ai_plan: Some(plan),
                        ..crate::openhuman::projects::TaskPatch::default()
                    },
                    "ai",
                ) {
                    log::warn!("{LOG} task={task_id} failed to write ai_plan: {e}");
                }
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
                notify_teams_chat(
                    &task_id,
                    &title,
                    "blocked",
                    &cc_workspace_dir,
                    &claude_resume_uuid,
                );
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
         Be concise and actionable. Respond with the outcome directly.\n\n\
         IMPORTANT RULES:\n\
         1. You MUST use tools to complete the task — do NOT answer from memory \
         or prior context. Every task requires live action (fetching data, \
         running a command, calling a service, etc.).\n\
         2. Use any tools available to you: Bash commands, web search, locally \
         installed plugins/skills, MCP servers, or any other tool in your \
         toolset. Choose the best tool for the job.\n\
         3. If you cannot complete the task because a required tool, service, \
         or resource is unavailable or unreachable (e.g. a required integration \
         is not connected, credentials are missing, an external system is down), \
         start your response with exactly \"BLOCKED: \" followed by a short \
         explanation of what is missing and how to resolve it. \
         Do NOT mark the task as done when you cannot complete it.",
    );
    prompt
}

async fn run_agent(
    config: &Config,
    task_id: &str,
    prompt: &str,
    hint_thread_id: &str,
) -> (
    Result<String, String>,
    tokio::task::JoinHandle<()>,
    Option<String>,
) {
    use crate::openhuman::inference::provider::claude_code::{
        workspace_dir_from_config, ClaudeCodeProvider,
    };
    use crate::openhuman::inference::provider::traits::ChatRequest;
    use crate::openhuman::inference::provider::{ChatMessage, Provider};

    log::debug!("{LOG} task={task_id} building ClaudeCodeProvider directly");

    // Build the provider directly — bypass agent harness so the task is
    // handled entirely by the claude CLI subprocess, not delegated through
    // openhuman's tool/subagent machinery.
    let workspace = workspace_dir_from_config(config);
    let provider = match ClaudeCodeProvider::from_env(
        // Extract model from chat_provider string ("claude-code:<model>")
        config
            .chat_provider
            .as_deref()
            .and_then(|p| p.strip_prefix("claude-code:"))
            .unwrap_or("claude-sonnet-latest"),
        workspace,
        config.action_dir.clone(),
    ) {
        Ok(p) => p,
        Err(e) => {
            return (
                Err(format!("failed to build ClaudeCodeProvider: {e}")),
                tokio::spawn(async {}),
                None,
            );
        }
    };

    // Set up a progress channel so the task log shows live lines.
    let (stream_tx, mut stream_rx) =
        tokio::sync::mpsc::channel::<crate::openhuman::inference::provider::ProviderDelta>(64);

    let task_id_fwd = task_id.to_string();
    let fwd = tokio::spawn(async move {
        use crate::openhuman::inference::provider::ProviderDelta;
        // Buffer streaming text deltas and emit whole lines only.
        // claude CLI streams text character-by-character; emitting each
        // delta as a separate log line would split words mid-character.
        let mut buf = String::new();
        while let Some(delta) = stream_rx.recv().await {
            if let ProviderDelta::TextDelta { delta: text } = delta {
                buf.push_str(&text);
                // Emit every complete line (terminated by '\n').
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim_end_matches('\r').to_string();
                    buf = buf[pos + 1..].to_string();
                    if !line.is_empty() {
                        emit_task_log(&task_id_fwd, &line, "log");
                    }
                }
            }
        }
        // Emit any remaining text that had no trailing newline.
        let remainder = buf.trim().to_string();
        if !remainder.is_empty() {
            emit_task_log(&task_id_fwd, &remainder, "log");
        }
    });

    let messages = vec![ChatMessage::user(prompt)];
    let request = ChatRequest {
        messages: &messages,
        tools: None,
        stream: Some(&stream_tx),
        max_tokens: None,
        hint_thread_id: Some(hint_thread_id),
    };

    // Determine model from config
    let model = config
        .chat_provider
        .as_deref()
        .and_then(|p| p.strip_prefix("claude-code:"))
        .unwrap_or("claude-sonnet-latest");

    // Snapshot session store keys before the run so we can detect the new UUID
    // written by the driver (via system init event capture).
    let workspace =
        crate::openhuman::inference::provider::claude_code::workspace_dir_from_config(config);
    let keys_before: std::collections::HashSet<String> = {
        use crate::openhuman::inference::provider::claude_code::session_store::SessionStore;
        // Re-read from disk to get current keys; the store doesn't expose iteration,
        // so re-open a fresh instance and check hint_thread_id key.
        // We use a simpler approach: read the JSON file directly.
        let path = workspace.join("claude-code-sessions.json");
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("sessions").cloned())
            .and_then(|s| s.as_object().cloned())
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    };

    let result = provider
        .chat(request, model, 0.0)
        .await
        .map(|resp| resp.text.unwrap_or_default())
        .map_err(|e| e.to_string());

    drop(stream_tx); // close channel so forwarder task ends

    // After the run, find the newly added session store entry (written by the
    // driver's system event capture). Return it alongside the result so the
    // caller can store the real resumable UUID in ai_plan.
    let actual_session_id: Option<String> = {
        let path = workspace.join("claude-code-sessions.json");
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("sessions").cloned())
            .and_then(|s| s.as_object().cloned())
            .and_then(|m| {
                // First try the hint_thread_id key directly
                m.get(hint_thread_id)
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .or_else(|| {
                        // Fall back: find any new UUID key added since the snapshot
                        m.iter()
                            .filter(|(k, _)| !keys_before.contains(*k) && !k.starts_with("hash_"))
                            .filter_map(|(_, v)| v.as_str().map(str::to_string))
                            .next()
                    })
            })
    };

    (result, fwd, actual_session_id)
}

// ---------------------------------------------------------------------------
// Teams-chat notification (fire-and-forget)
// ---------------------------------------------------------------------------

/// POST task status change to the local teams-chat server so it forwards the
/// notification to the relay RSS feed (picked up by Power Automate → Teams).
/// Silently skips if teams-chat is not running; logs a warning if the call
/// fails for any other reason.
fn notify_teams_chat(
    task_id: &str,
    title: &str,
    status: &str,
    workspace_dir: &str,
    session_id: &str,
) {
    let base = std::env::var("TEAMS_CHAT_URL").unwrap_or_else(|_| "http://localhost:13001".into());
    let notify_title = if status == "done" {
        format!("✅ Task done: {title}")
    } else {
        format!("⚠️ Task blocked: {title}")
    };
    let body = serde_json::json!({
        "session_id": session_id,
        "project_path": workspace_dir,
        "title": notify_title,
    });
    let task_id = task_id.to_string();
    tokio::spawn(async move {
        let url = format!("{base}/notify");
        match reqwest::Client::new().post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                log::debug!("[projects] teams-chat notified task={task_id}")
            }
            Ok(resp) => log::warn!(
                "[projects] teams-chat /notify returned {} for task={task_id}",
                resp.status()
            ),
            Err(e) => log::warn!(
                "[projects] teams-chat unreachable, notification skipped for task={task_id}: {e}"
            ),
        }
    });
}
