//! Card-action dispatch loop (Feishu interactive-card button clicks).
//!
//! Runs independently of the conversational message pipeline: a card action is
//! a side-effect trigger (open a resume group, bind its Claude Code session),
//! not an agent turn. The only action handled today is `resume_task`, fired by
//! the "continue this task in a group" button on a completion notice.

use std::sync::Arc;

use crate::openhuman::channels::traits::{CardAction, Channel, SendMessage};
use crate::openhuman::config::Config;
use crate::openhuman::projects::store;

/// Consume card actions until the sender side (all channels) drops.
pub(crate) async fn run_card_action_loop(
    mut rx: tokio::sync::mpsc::Receiver<CardAction>,
    config: Arc<Config>,
) {
    tracing::info!("[card-action] dispatch loop started");
    while let Some(action) = rx.recv().await {
        if let Err(e) = handle_card_action(&config, &action).await {
            tracing::warn!(
                "[card-action] handler failed channel={} chat={}: {e}",
                action.channel,
                action.chat_id
            );
        }
    }
    tracing::info!("[card-action] dispatch loop ended");
}

async fn handle_card_action(config: &Arc<Config>, action: &CardAction) -> anyhow::Result<()> {
    let kind = action
        .action_value
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    match kind {
        "resume_task" => handle_resume_task(config, action).await,
        other => {
            tracing::debug!("[card-action] ignoring unknown action '{other}'");
            Ok(())
        }
    }
}

/// Open (or reuse) a Feishu group for the task, bind its CC session, and post
/// an echo of the last result so the user can keep chatting to resume it.
async fn handle_resume_task(config: &Arc<Config>, action: &CardAction) -> anyhow::Result<()> {
    let task_id = action
        .action_value
        .get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("resume_task action missing task_id"))?;

    // Build a LarkChannel from config (same pattern as notify_lark_completion) —
    // needed both to create the group and to post into it.
    let lark_cfg = config
        .channels_config
        .lark
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lark not configured — cannot open resume group"))?;
    let lark = crate::openhuman::channels::lark::LarkChannel::from_config(lark_cfg);

    // Read the task's persisted CC session + workspace from ai_plan.
    let task = store::get_task(config, task_id)
        .map_err(|e| anyhow::anyhow!("task {task_id} not found: {e}"))?;
    let (session_id, workspace_dir) = parse_session_from_ai_plan(task.ai_plan.as_deref())
        .ok_or_else(|| {
            anyhow::anyhow!("task {task_id} has no Claude Code session to resume (never ran?)")
        })?;

    // Each click opens a FRESH group. Feishu can't tell us whether the user is
    // still in a previously-opened group (they may have left it), so honoring a
    // re-click by always creating a new group is the only way to guarantee the
    // user lands somewhere they can actually chat. Clear the old chat's binding
    // so a stale group no longer resumes this session.
    if let Some(old_chat) = task
        .feishu_resume_chat_id
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        let _ = store::clear_binding(config, old_chat);
    }

    // Create a new group and invite the clicker.
    let invite: Vec<String> = if action.open_id.is_empty() {
        Vec::new()
    } else {
        vec![action.open_id.clone()]
    };
    let title = format!("继续任务:{}", truncate(&task.title, 40));
    let new_chat_id = lark.create_group(&title, &invite).await?;

    // Persist the binding both ways.
    store::bind_feishu_session(config, &new_chat_id, task_id, &session_id, &workspace_dir)?;
    store::set_task_feishu_resume_chat(config, task_id, Some(&new_chat_id))?;

    // Echo the task context + last result into the new group.
    let echo = build_resume_echo(&task);
    lark.send(&SendMessage::new(echo, new_chat_id.clone()))
        .await?;

    tracing::info!(
        "[card-action] opened resume group {new_chat_id} for task {task_id} (session {session_id})"
    );
    Ok(())
}

/// Extract `(claude_session_id, claude_workspace_dir)` from a task's ai_plan.
fn parse_session_from_ai_plan(ai_plan: Option<&str>) -> Option<(String, String)> {
    let plan = ai_plan?;
    let v: serde_json::Value = serde_json::from_str(plan).ok()?;
    let session = v.get("claude_session_id")?.as_str()?.to_string();
    let workspace = v.get("claude_workspace_dir")?.as_str()?.to_string();
    if session.is_empty() || workspace.is_empty() {
        return None;
    }
    Some((session, workspace))
}

/// Compose the first message posted into a freshly-opened resume group.
fn build_resume_echo(task: &crate::openhuman::projects::Task) -> String {
    let mut s = String::new();
    s.push_str(&format!("## ↩️ 继续任务:{}\n\n", task.title));
    if let Some(desc) = task.description.as_deref().filter(|d| !d.trim().is_empty()) {
        s.push_str(&format!("**任务描述:**\n{desc}\n\n"));
    }
    s.push_str("我已经加载了这个任务之前的 Claude Code 工作会话。直接在这个群里发消息,就能接着之前的进度继续。\n");
    s
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars).collect();
        format!("{t}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_session_from_ai_plan() {
        let plan = r#"{"claude_session_id":"uuid-xyz","claude_workspace_dir":"/ws"}"#;
        let got = parse_session_from_ai_plan(Some(plan));
        assert_eq!(got, Some(("uuid-xyz".to_string(), "/ws".to_string())));
    }

    #[test]
    fn missing_or_empty_session_yields_none() {
        assert!(parse_session_from_ai_plan(None).is_none());
        assert!(parse_session_from_ai_plan(Some("{}")).is_none());
        assert!(parse_session_from_ai_plan(Some(
            r#"{"claude_session_id":"","claude_workspace_dir":"/ws"}"#
        ))
        .is_none());
    }

    #[test]
    fn truncate_caps_length() {
        assert_eq!(truncate("hello", 40), "hello");
        assert_eq!(truncate(&"a".repeat(50), 3), "aaa…");
    }
}
