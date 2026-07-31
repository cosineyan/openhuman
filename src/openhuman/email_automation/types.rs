use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BatchParseMode {
    /// Run parse_script only on the first queued email; use its vars for the combined task.
    #[default]
    FirstOnly,
    /// Run parse_script on every queued email; merge results into an `{{items}}` list.
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAutomationRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Case-insensitive substring match on sender name or email address.
    pub sender_contains: Option<String>,
    /// Case-insensitive substring match on email subject.
    pub subject_contains: Option<String>,
    /// Case-insensitive substring match on body preview.
    pub body_contains: Option<String>,
    /// Task title template. Supports {{subject}}, {{sender}}, and any vars from parse_script.
    pub task_title_template: String,
    /// Optional task description template. Same placeholder support.
    pub task_description_template: Option<String>,
    /// Assignee for the created task. Defaults to "ai".
    pub assignee: String,
    /// Optional bucket_id override. None = first bucket (To Do).
    pub bucket_id: Option<String>,
    /// When true, if no rule matches the email is also passed to the LLM
    /// to decide whether a task should be created.
    pub llm_fallback_enabled: bool,
    /// Python script that parses the email body and returns a JSON dict.
    pub parse_script: Option<String>,
    /// When true, matching emails are queued and combined into a single task after batch_window_secs.
    #[serde(default)]
    pub batch_mode: bool,
    /// Seconds to accumulate emails before draining the queue. Default 21600 (6h).
    #[serde(default = "default_batch_window_secs")]
    pub batch_window_secs: u64,
    /// Whether to run parse_script on only the first queued email or all of them.
    #[serde(default)]
    pub batch_parse_mode: BatchParseMode,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRuleInput {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub sender_contains: Option<String>,
    pub subject_contains: Option<String>,
    pub body_contains: Option<String>,
    pub task_title_template: String,
    pub task_description_template: Option<String>,
    #[serde(default = "default_assignee")]
    pub assignee: String,
    pub bucket_id: Option<String>,
    #[serde(default)]
    pub llm_fallback_enabled: bool,
    pub parse_script: Option<String>,
    #[serde(default)]
    pub batch_mode: bool,
    #[serde(default = "default_batch_window_secs")]
    pub batch_window_secs: u64,
    #[serde(default)]
    pub batch_parse_mode: BatchParseMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RulePatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub sender_contains: Option<Option<String>>,
    pub subject_contains: Option<Option<String>>,
    pub body_contains: Option<Option<String>>,
    pub task_title_template: Option<String>,
    pub task_description_template: Option<Option<String>>,
    pub assignee: Option<String>,
    pub bucket_id: Option<Option<String>>,
    pub llm_fallback_enabled: Option<bool>,
    pub parse_script: Option<Option<String>>,
    pub batch_mode: Option<bool>,
    pub batch_window_secs: Option<u64>,
    pub batch_parse_mode: Option<BatchParseMode>,
}

/// Extracted fields from an email's body_preview.
#[derive(Debug, Clone, Default)]
pub struct EmailContext {
    pub subject: String,
    pub sender: String,
    pub body_preview: String,
    /// Full email body (may be empty if only preview is available).
    pub full_body: String,
    /// The memory tree chunk_id for the first chunk of this email.
    pub chunk_id: String,
    /// The memory tree source_id (unique per email, used for deduplication).
    pub source_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleHit {
    pub rule_id: String,
    pub rule_name: String,
    pub task_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunNowResult {
    pub emails_scanned: usize,
    pub tasks_created: usize,
    pub hits: Vec<RuleHit>,
}

/// An entry in the batch queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchQueueEntry {
    pub id: String,
    pub rule_id: String,
    pub source_id: String,
    pub email_body: String,
    pub matched_at: String,
}

fn default_true() -> bool {
    true
}

fn default_assignee() -> String {
    "ai".to_string()
}

pub fn default_batch_window_secs() -> u64 {
    21600
}

/// A lightweight summary of an email chunk for the picker UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailChunkSummary {
    pub chunk_id: String,
    pub subject: String,
    pub sender: String,
    pub date: String,
    /// First ~120 chars of the body after the prefix line.
    pub preview: String,
}
