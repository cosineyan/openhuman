//! Build the stream-json stdin payload fed to `claude --input-format stream-json`.
//!
//! The CLI consumes one JSON object per line on stdin. Each line looks
//! like:
//!   { "type":"user", "message":{"role":"user","content":[{"type":"text","text":"..."}]} }
//!
//! v1 piping policy:
//! - On a *new* CC session: send every history `ChatMessage` so claude
//!   has full context (system message is conveyed via
//!   `--append-system-prompt`, not stdin).
//! - On a `--resume` of an existing CC session: claude already has prior
//!   turns server-side; we only send the last user turn.

use serde_json::{json, Value};

use crate::openhuman::inference::provider::traits::ChatMessage;

/// Build the bytes to write to claude's stdin. Returns an empty `Vec`
/// when there is nothing to send (caller should abort).
pub fn build_stdin(messages: &[ChatMessage], is_new_session: bool) -> Vec<u8> {
    let mut out = String::new();
    let to_emit: Vec<&ChatMessage> = if is_new_session {
        // Filter system messages, then drop any leading assistant turns.
        // The CC CLI requires the first message to have role "user"; an
        // opening assistant greeting (e.g. the agent's welcome message)
        // causes exit(1) with "Expected message role 'user', got 'assistant'".
        let filtered: Vec<&ChatMessage> = messages.iter().filter(|m| m.role != "system").collect();
        log::debug!(
            "[claude-code][input_builder] is_new=true messages={} filtered={} roles={:?}",
            messages.len(),
            filtered.len(),
            filtered.iter().map(|m| m.role.as_str()).collect::<Vec<_>>()
        );
        let first_user = filtered
            .iter()
            .position(|m| m.role == "user")
            .unwrap_or(filtered.len());
        filtered[first_user..].to_vec()
    } else {
        // Resume: only the trailing user turn matters.
        messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .into_iter()
            .collect()
    };

    for msg in to_emit {
        let role = match msg.role.as_str() {
            "user" => "user",
            "assistant" => "assistant",
            // CC stdin doesn't accept `system` or `tool` rows. The system
            // prompt is plumbed via `--append-system-prompt`; tool roles
            // belong to the harness, not the CLI's input format.
            _ => continue,
        };
        // stream-json format requires `type` to match the message role.
        let line = json!({
            "type": role,
            "message": {
                "role": role,
                "content": [{"type": "text", "text": msg.content}],
            },
        });
        push_json_line(&mut out, &line);
    }

    out.into_bytes()
}

fn push_json_line(buf: &mut String, v: &Value) {
    buf.push_str(&serde_json::to_string(v).unwrap_or_default());
    buf.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> ChatMessage {
        match role {
            "system" => ChatMessage::system(content),
            "user" => ChatMessage::user(content),
            "assistant" => ChatMessage::assistant(content),
            _ => ChatMessage::tool(content),
        }
    }

    #[test]
    fn new_session_drops_leading_assistant_turns() {
        // Agent welcome message arrives as assistant before any user turn.
        let history = vec![
            msg("system", "you are helpful"),
            msg("assistant", "Hi! How can I help?"),
            msg("user", "hello"),
            msg("assistant", "hello back"),
            msg("user", "how are you?"),
        ];
        let bytes = build_stdin(&history, true);
        let s = String::from_utf8(bytes).unwrap();
        let lines: Vec<_> = s.lines().collect();
        // First emitted line must be a user turn, not the assistant greeting.
        assert!(lines[0].contains("\"hello\""), "first line: {}", lines[0]);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn new_session_pipes_full_user_history() {
        let history = vec![
            msg("system", "you are helpful"),
            msg("user", "hi"),
            msg("assistant", "hello"),
            msg("user", "how are you?"),
        ];
        let bytes = build_stdin(&history, true);
        let s = String::from_utf8(bytes).unwrap();
        let lines: Vec<_> = s.lines().collect();
        assert_eq!(lines.len(), 3); // system filtered out
        assert!(lines[0].contains("\"hi\""));
        assert!(lines[1].contains("\"hello\""));
        assert!(lines[2].contains("how are you"));
    }

    #[test]
    fn resume_pipes_only_last_user_turn() {
        let history = vec![
            msg("user", "earlier turn"),
            msg("assistant", "earlier reply"),
            msg("user", "follow-up"),
        ];
        let bytes = build_stdin(&history, false);
        let s = String::from_utf8(bytes).unwrap();
        let lines: Vec<_> = s.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"follow-up\""));
    }

    #[test]
    fn empty_history_yields_empty_bytes() {
        let bytes = build_stdin(&[], true);
        assert!(bytes.is_empty());
    }
}
