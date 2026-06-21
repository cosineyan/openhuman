//! Wire types for the `claude --output-format stream-json` NDJSON protocol.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SdkMessage {
    /// Streaming text delta or final text from the assistant.
    Text {
        text: String,
    },
    /// Terminal result frame: contains the final answer and cost metadata.
    Result {
        result: Option<String>,
        #[serde(rename = "is_error")]
        is_error: bool,
        #[serde(default)]
        total_cost_usd: Option<f64>,
    },
    /// Protocol-level error (e.g. API failure surfaced by the CLI).
    Error {
        error: SdkError,
    },
    /// An assistant turn message. May contain text blocks and/or tool_use blocks.
    Assistant {
        message: AssistantMessage,
    },
    /// Anything else (system, tool_result, content_block_delta, etc.) — ignored.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text from the assistant.
    Text { text: String },
    /// A tool invocation request.
    ToolUse { name: String },
    /// Anything else (thinking, redacted_thinking, …).
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct SdkError {
    pub message: String,
}
