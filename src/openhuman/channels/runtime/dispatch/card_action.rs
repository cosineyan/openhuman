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

    // Read the prior Claude Code conversation for this session so the new group
    // shows what was already done (not just a static "session loaded" note).
    let conversation = crate::openhuman::projects::session_watcher::read_session_conversation(
        &workspace_dir,
        &session_id,
    );

    // First message: task header + how-to-continue note.
    let echo = build_resume_echo(&task, !conversation.is_empty());
    lark.send(&SendMessage::new(echo, new_chat_id.clone()))
        .await?;

    // Second message: the prior conversation transcript (truncated to fit the
    // Feishu card size). Skipped when there's nothing to show.
    if let Some(transcript) = build_transcript_echo(&conversation) {
        if let Err(e) = lark
            .send(&SendMessage::new(transcript, new_chat_id.clone()))
            .await
        {
            // Non-fatal: the group + binding already exist, the user can still
            // resume by chatting. Just log that the history echo failed.
            tracing::warn!("[card-action] transcript echo failed for task {task_id}: {e}");
        }
    }

    tracing::info!(
        "[card-action] opened resume group {new_chat_id} for task {task_id} (session {session_id}, {} prior turns)",
        conversation.len()
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
/// When `has_history` is true, the prior conversation is posted separately
/// right after, so the note points at it; otherwise it stands alone.
fn build_resume_echo(task: &crate::openhuman::projects::Task, has_history: bool) -> String {
    let mut s = String::new();
    s.push_str(&format!("## ↩️ 继续任务:{}\n\n", task.title));
    if let Some(desc) = task.description.as_deref().filter(|d| !d.trim().is_empty()) {
        s.push_str(&format!("**任务描述:**\n{desc}\n\n"));
    }
    if has_history {
        s.push_str("下面是这个任务之前的 Claude Code 工作记录。直接在这个群里发消息,就能接着之前的进度继续。\n");
    } else {
        s.push_str("我已经加载了这个任务之前的 Claude Code 工作会话。直接在这个群里发消息,就能接着之前的进度继续。\n");
    }
    s
}

/// Max characters of prior conversation to echo into the resume group. Feishu
/// interactive cards have a payload ceiling; keep the transcript comfortably
/// under it, preferring the most RECENT turns (that's what the user needs to
/// continue). Mirrors the truncation budget used for completion notices.
const TRANSCRIPT_ECHO_MAX_CHARS: usize = 3000;

/// Render prior `(role, text)` turns into a Feishu-markdown transcript, keeping
/// the most recent turns within [`TRANSCRIPT_ECHO_MAX_CHARS`]. Returns `None`
/// when there's nothing to show.
fn build_transcript_echo(conversation: &[(String, String)]) -> Option<String> {
    if conversation.is_empty() {
        return None;
    }

    // Render each turn, then keep the most recent ones that fit the budget.
    let rendered: Vec<String> = conversation
        .iter()
        .map(|(role, text)| {
            let who = if role == "assistant" {
                "**🤖 Claude**"
            } else {
                "**🧑 你**"
            };
            format!("{who}\n{}", text.trim())
        })
        .collect();

    let mut selected: Vec<&String> = Vec::new();
    let mut used = 0usize;
    let mut dropped_older = false;
    for chunk in rendered.iter().rev() {
        // +2 accounts for the "\n\n" separator between turns.
        let cost = chunk.chars().count() + 2;
        if used + cost > TRANSCRIPT_ECHO_MAX_CHARS && !selected.is_empty() {
            dropped_older = true;
            break;
        }
        used += cost;
        selected.push(chunk);
    }
    selected.reverse();

    let mut out = String::from("### 📜 之前的对话\n\n");
    if dropped_older {
        out.push_str("_（较早的内容已省略，仅显示最近部分）_\n\n");
    }
    out.push_str(
        &selected
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n\n"),
    );
    Some(out)
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

    #[test]
    fn transcript_echo_none_for_empty() {
        assert!(build_transcript_echo(&[]).is_none());
    }

    #[test]
    fn transcript_echo_renders_roles() {
        let convo = vec![
            ("user".to_string(), "hi there".to_string()),
            ("assistant".to_string(), "hello back".to_string()),
        ];
        let out = build_transcript_echo(&convo).expect("some");
        assert!(out.contains("📜 之前的对话"));
        assert!(out.contains("**🧑 你**"));
        assert!(out.contains("hi there"));
        assert!(out.contains("**🤖 Claude**"));
        assert!(out.contains("hello back"));
        // Short conversation → no truncation notice.
        assert!(!out.contains("较早的内容已省略"));
    }

    #[test]
    fn transcript_echo_truncates_keeping_recent() {
        // Many long turns: only the most recent fit, older ones dropped with a note.
        let convo: Vec<(String, String)> = (0..50)
            .map(|i| {
                let role = if i % 2 == 0 { "user" } else { "assistant" };
                (role.to_string(), format!("turn-{i} ").repeat(40))
            })
            .collect();
        let out = build_transcript_echo(&convo).expect("some");
        assert!(out.chars().count() <= TRANSCRIPT_ECHO_MAX_CHARS + 200);
        assert!(out.contains("较早的内容已省略"));
        // The very last turn must be present; an early one must be dropped.
        assert!(out.contains("turn-49"));
        assert!(!out.contains("turn-0 "));
    }
}
