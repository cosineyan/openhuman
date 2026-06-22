//! Background watcher for claude session files written during Resume sessions.
//!
//! When a user clicks the Resume button and completes work in the Claude Code
//! CLI, this module detects that the session has gone idle (no new writes for
//! IDLE_TIMEOUT), reads the conversation, summarises it via LLM, and writes
//! the summary back to the task as a comment.  If the last assistant message
//! contains "DONE:" the task is also moved to the Done bucket.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use serde_json::Value;

use crate::openhuman::config::Config;
use crate::openhuman::projects::store;
use crate::openhuman::projects::TaskPatch;

/// Polling interval between mtime checks.
const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// How long the session file must be unmodified before we consider the session
/// idle and process the conversation.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes

static REGISTRY: OnceLock<Arc<Mutex<HashMap<String, WatchEntry>>>> = OnceLock::new();

fn registry() -> &'static Arc<Mutex<HashMap<String, WatchEntry>>> {
    REGISTRY.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

struct WatchEntry {
    task_id: String,
    session_path: PathBuf,
    config: Arc<Config>,
}

/// Register a background watcher for the given claude session.
///
/// `workspace_dir` is the cwd claude used (e.g. `~/OpenHuman/projects`);
/// we derive the `~/.claude/projects/<sanitized-cwd>/` directory from it.
pub fn register_session_watch(
    config: Arc<Config>,
    task_id: String,
    session_uuid: String,
    workspace_dir: String,
) {
    let session_path = match resolve_session_path(&workspace_dir, &session_uuid) {
        Some(p) => p,
        None => {
            log::warn!(
                "[session_watcher] could not resolve session path for task={task_id} uuid={session_uuid}"
            );
            return;
        }
    };

    if !session_path.exists() {
        log::debug!(
            "[session_watcher] session file not found yet, watching anyway: {}",
            session_path.display()
        );
    }

    let entry = WatchEntry {
        task_id: task_id.clone(),
        session_path: session_path.clone(),
        config: Arc::clone(&config),
    };

    // Deregister any previous watcher for this task before adding the new one.
    {
        let mut reg = registry().lock().expect("registry lock");
        reg.remove(&task_id);
        reg.insert(task_id.clone(), entry);
    }

    log::info!(
        "[session_watcher] registered task={task_id} session={}",
        session_path.display()
    );

    let reg = Arc::clone(registry());
    tokio::spawn(async move {
        watch_loop(task_id, reg).await;
    });
}

async fn watch_loop(task_id: String, reg: Arc<Mutex<HashMap<String, WatchEntry>>>) {
    let mut last_mtime: Option<SystemTime> = None;
    let mut idle_since: Option<std::time::Instant> = None;

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        // Check if we're still registered (deregistered means another run started).
        let (path, config) = {
            let reg_guard = reg.lock().expect("registry lock");
            match reg_guard.get(&task_id) {
                Some(e) => (e.session_path.clone(), Arc::clone(&e.config)),
                None => {
                    log::debug!("[session_watcher] task={task_id} deregistered, stopping watcher");
                    return;
                }
            }
        };

        // Read current mtime.
        let current_mtime = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());

        match (last_mtime, current_mtime) {
            (_, None) => {
                // File doesn't exist yet — keep waiting.
                idle_since = None;
            }
            (Some(prev), Some(cur)) if cur == prev => {
                // mtime unchanged — start or continue idle countdown.
                if idle_since.is_none() {
                    idle_since = Some(std::time::Instant::now());
                }
            }
            (_, Some(cur)) => {
                // File was modified — reset idle timer.
                last_mtime = Some(cur);
                idle_since = None;
            }
        }

        // Check if idle timeout has been reached.
        if let Some(since) = idle_since {
            if since.elapsed() >= IDLE_TIMEOUT {
                log::info!("[session_watcher] task={task_id} idle timeout reached, processing session");
                reg.lock().expect("registry lock").remove(&task_id);
                process_session(config, task_id, path).await;
                return;
            }
        }
    }
}

async fn process_session(config: Arc<Config>, task_id: String, session_path: PathBuf) {
    // Read and parse the session JSONL file.
    let content = match std::fs::read_to_string(&session_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[session_watcher] task={task_id} failed to read session: {e}");
            return;
        }
    };

    // Extract user/assistant messages.
    let mut conversation: Vec<(String, String)> = Vec::new(); // (role, text)
    for line in content.lines() {
        let Ok(val) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ty = val.get("type").and_then(Value::as_str).unwrap_or("");
        if ty == "user" || ty == "assistant" {
            let role = ty.to_string();
            let text = extract_message_text(&val);
            if !text.is_empty() {
                conversation.push((role, text));
            }
        }
    }

    if conversation.is_empty() {
        log::debug!("[session_watcher] task={task_id} no conversation found in session");
        return;
    }

    // Check if the last assistant message contains DONE:.
    let last_assistant = conversation
        .iter()
        .rev()
        .find(|(role, _)| role == "assistant")
        .map(|(_, text)| text.as_str())
        .unwrap_or("");
    let is_done = last_assistant.contains("DONE:");

    // Build a summary via LLM.
    let summary = match summarise_conversation(&config, &conversation, &task_id).await {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[session_watcher] task={task_id} summarisation failed: {e}");
            // Fall back: use raw last assistant message as comment.
            if last_assistant.is_empty() {
                return;
            }
            last_assistant.to_string()
        }
    };

    // Write comment to task.
    let comment = format!("**Resume session summary**\n\n{summary}");
    if let Err(e) = store::add_comment(&config, &task_id, "ai", &comment) {
        log::warn!("[session_watcher] task={task_id} failed to add comment: {e}");
        return;
    }
    log::info!("[session_watcher] task={task_id} added resume summary comment");

    // If DONE: detected, move task to the Done bucket.
    if is_done {
        match move_task_to_done(&config, &task_id) {
            Ok(true) => log::info!("[session_watcher] task={task_id} moved to Done"),
            Ok(false) => log::warn!("[session_watcher] task={task_id} no Done bucket found"),
            Err(e) => log::warn!("[session_watcher] task={task_id} failed to move to Done: {e}"),
        }
    }
}

fn extract_message_text(val: &Value) -> String {
    // Structure: { type: "user"|"assistant", message: { role, content: "..." | [...] } }
    let message = val.get("message").unwrap_or(&Value::Null);
    let content = message.get("content").unwrap_or(&Value::Null);
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|c| {
                if c.get("type").and_then(Value::as_str) == Some("text") {
                    c.get("text").and_then(Value::as_str).map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

async fn summarise_conversation(
    config: &Config,
    conversation: &[(String, String)],
    task_id: &str,
) -> anyhow::Result<String> {
    let provider = crate::openhuman::memory::chat::build_chat_provider(config)?;

    let transcript = conversation
        .iter()
        .map(|(role, text)| format!("[{role}]: {text}"))
        .collect::<Vec<_>>()
        .join("\n\n");

    let prompt = crate::openhuman::memory::chat::ChatPrompt {
        system: "You are a concise technical summariser. \
                 Given a conversation between a user and an AI assistant, \
                 produce a 2-4 sentence summary of what was accomplished."
            .to_string(),
        user: format!("Summarise this task session:\n\n{transcript}"),
        temperature: 0.3,
        kind: "session_summary",
        max_tokens: Some(256),
    };

    let summary = provider.chat_for_text(&prompt).await?;
    log::debug!("[session_watcher] task={task_id} summary={summary:?}");
    Ok(summary)
}

fn move_task_to_done(config: &Config, task_id: &str) -> anyhow::Result<bool> {
    // Get the task to find its project_id.
    let task = store::get_task(config, task_id)?;
    let buckets = store::list_buckets(config, &task.project_id)?;
    let done_bucket = buckets
        .iter()
        .find(|b| b.is_done_bucket)
        .or_else(|| buckets.iter().find(|b| b.title.to_lowercase().contains("done")));

    match done_bucket {
        None => Ok(false),
        Some(b) => {
            let patch = TaskPatch {
                bucket_id: Some(b.id.clone()),
                ..TaskPatch::default()
            };
            store::update_task(config, task_id, &patch, "ai")?;
            Ok(true)
        }
    }
}

/// Derive the claude session file path from workspace_dir and session_uuid.
/// claude stores sessions at `~/.claude/projects/<sanitized-cwd>/<uuid>.jsonl`
/// where sanitized-cwd strips the leading `/` and replaces `/` with `-`.
fn resolve_session_path(workspace_dir: &str, session_uuid: &str) -> Option<PathBuf> {
    let home = directories::UserDirs::new()?.home_dir().to_path_buf();
    // Sanitize: strip leading '/', replace '/' with '-'.
    let sanitized = workspace_dir
        .trim_start_matches('/')
        .replace('/', "-");
    let project_dir = home
        .join(".claude")
        .join("projects")
        .join(&sanitized);
    Some(project_dir.join(format!("{session_uuid}.jsonl")))
}
