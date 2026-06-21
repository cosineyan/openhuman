//! Subprocess lifecycle for the Claude Agent SDK provider.

use std::sync::Mutex;

use anyhow::Context;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

use crate::openhuman::config::schema::claude_agent_sdk::ClaudeAgentSdkConfig;
use crate::openhuman::inference::provider::traits::Provider;

use super::protocol::{ContentBlock, SdkMessage};

pub struct ClaudeAgentSdkProvider {
    pub(super) config: ClaudeAgentSdkConfig,
    /// Optional channel for forwarding human-readable progress lines (tool names,
    /// etc.) to callers that display live task logs. Interior mutability so the
    /// sender can be injected after the provider is constructed and before it is
    /// wrapped in an `Arc`. Lines are best-effort: a full channel or dropped
    /// receiver is silently ignored.
    progress_tx: Mutex<Option<mpsc::Sender<String>>>,
}

impl ClaudeAgentSdkProvider {
    pub fn new(config: ClaudeAgentSdkConfig) -> Self {
        Self {
            config,
            progress_tx: Mutex::new(None),
        }
    }
}

#[async_trait]
impl Provider for ClaudeAgentSdkProvider {
    /// Attach a progress line sender. Emits one line per tool call the
    /// claude subprocess makes, e.g. `"Using tool: Bash"`.
    fn set_progress_tx(&self, tx: mpsc::Sender<String>) {
        if let Ok(mut guard) = self.progress_tx.lock() {
            *guard = Some(tx);
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        let model = if model.is_empty() {
            &self.config.default_model
        } else {
            model
        };

        // Prepend system prompt inline — claude -p has no separate system flag.
        let full_message = match system_prompt {
            Some(s) if !s.trim().is_empty() => {
                format!("[SYSTEM]\n{s}\n[/SYSTEM]\n\n{message}")
            }
            _ => message.to_string(),
        };

        let mut cmd = Command::new(&self.config.binary);
        cmd.arg("-p")
            .arg(&full_message)
            .arg("--model")
            .arg(model)
            .arg("--output-format")
            .arg("stream-json")
            // --verbose is required by the current claude CLI when using
            // stream-json with -p, otherwise it exits with an error.
            .arg("--verbose")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        if let Some(budget) = self.config.max_budget_usd {
            cmd.arg("--max-turns").arg("10");
            cmd.arg("--budget").arg(format!("{budget:.4}"));
        }

        tracing::debug!(
            "[claude_agent_sdk] spawning claude binary={} model={} message_len={}",
            self.config.binary,
            model,
            full_message.len()
        );

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn claude binary '{}'", self.config.binary))?;

        let stdout = child
            .stdout
            .take()
            .context("claude subprocess has no stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("claude subprocess has no stderr")?;

        // Drain stderr concurrently to prevent pipe-buffer stalls and capture failure context.
        let stderr_task = tokio::spawn(async move {
            let mut err_lines = BufReader::new(stderr).lines();
            let mut buf = String::new();
            while let Ok(Some(line)) = err_lines.next_line().await {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&line);
            }
            buf
        });

        let mut lines = BufReader::new(stdout).lines();
        let mut text_parts: Vec<String> = Vec::new();
        let mut result_text: Option<String> = None;
        let mut error_message: Option<String> = None;
        // Clone the sender out of the mutex once so we don't hold the lock
        // across await points.
        let progress_tx: Option<mpsc::Sender<String>> = self
            .progress_tx
            .lock()
            .ok()
            .and_then(|g| g.clone());

        let read_result = timeout(Duration::from_secs(120), async {
            while let Some(line) = lines.next_line().await? {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                tracing::trace!(
                    "[claude_agent_sdk] ndjson line received line_len={}",
                    line.len()
                );
                match serde_json::from_str::<SdkMessage>(&line) {
                    Ok(SdkMessage::Text { text }) => {
                        text_parts.push(text);
                    }
                    Ok(SdkMessage::Assistant { message }) => {
                        // Forward each tool_use block as a progress line so
                        // the task log shows what the claude subprocess is doing.
                        if let Some(ref tx) = progress_tx {
                            for block in &message.content {
                                if let ContentBlock::ToolUse { name } = block {
                                    let log_line = format!("Using tool: {name}");
                                    tracing::debug!("[claude_agent_sdk] {log_line}");
                                    let _ = tx.try_send(log_line);
                                }
                            }
                        }
                    }
                    Ok(SdkMessage::Result {
                        result,
                        is_error,
                        total_cost_usd,
                    }) => {
                        if let Some(cost) = total_cost_usd {
                            tracing::debug!(
                                "[claude_agent_sdk] request completed total_cost_usd={:.6}",
                                cost
                            );
                        }
                        if is_error {
                            error_message = Some(result.unwrap_or_else(|| {
                                "claude subprocess returned an error".to_string()
                            }));
                        } else {
                            result_text = result;
                        }
                    }
                    Ok(SdkMessage::Error { error }) => {
                        error_message = Some(error.message);
                    }
                    Ok(SdkMessage::Unknown) => {
                        tracing::trace!("[claude_agent_sdk] unknown ndjson message type, skipping");
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            line_len = line.len(),
                            "[claude_agent_sdk] failed to parse ndjson line"
                        );
                    }
                }
            }
            anyhow::Ok(())
        })
        .await;

        match read_result {
            Ok(inner) => inner?,
            Err(_) => {
                let _ = child.kill().await;
                anyhow::bail!("[claude_agent_sdk] subprocess timed out while reading output");
            }
        }

        let status = timeout(Duration::from_secs(30), child.wait())
            .await
            .map_err(|_| {
                anyhow::anyhow!("[claude_agent_sdk] subprocess timed out while waiting for exit")
            })??;
        let stderr_output = stderr_task.await.unwrap_or_default();
        tracing::debug!("[claude_agent_sdk] subprocess exited status={}", status);

        if let Some(err) = error_message {
            anyhow::bail!("[claude_agent_sdk] error from claude CLI: {err}");
        }

        // Use the final result message if present; otherwise join streaming text parts.
        let output = result_text
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| text_parts.join(""));

        if !status.success() && output.is_empty() {
            anyhow::bail!(
                "[claude_agent_sdk] claude subprocess exited with non-zero status {} and no output; stderr={}",
                status,
                stderr_output
            );
        }

        tracing::debug!(
            "[claude_agent_sdk] response collected output_len={}",
            output.len()
        );

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::config::schema::claude_agent_sdk::ClaudeAgentSdkConfig;

    #[test]
    fn provider_constructs_with_default_config() {
        let config = ClaudeAgentSdkConfig::default();
        let provider = ClaudeAgentSdkProvider::new(config);
        assert_eq!(provider.config.binary, "claude");
        assert_eq!(provider.config.default_model, "claude-sonnet-4-6");
    }

    #[test]
    fn config_default_disabled() {
        let config = ClaudeAgentSdkConfig::default();
        assert!(!config.enabled);
        assert!(config.max_budget_usd.is_none());
    }

    #[test]
    fn set_progress_tx_stores_sender() {
        let config = ClaudeAgentSdkConfig::default();
        let provider = ClaudeAgentSdkProvider::new(config);
        assert!(provider.progress_tx.lock().unwrap().is_none());
        let (tx, _rx) = mpsc::channel(8);
        provider.set_progress_tx(tx);
        assert!(provider.progress_tx.lock().unwrap().is_some());
    }
}
