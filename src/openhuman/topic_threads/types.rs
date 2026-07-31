use serde::{Deserialize, Serialize};

/// How multiple keywords combine when matching a chunk body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KeywordLogic {
    /// Any single keyword hit marks the chunk as relevant.
    #[default]
    Or,
    /// Every keyword must appear for the chunk to be relevant.
    And,
}

impl KeywordLogic {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeywordLogic::Or => "or",
            KeywordLogic::And => "and",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "and" => KeywordLogic::And,
            _ => KeywordLogic::Or,
        }
    }
}

/// A user-defined topic. Each topic owns a `mem_tree_trees` row of
/// `kind = 'topic'`; matching chunks are routed into that tree and rolled up
/// through the normal seal → summary pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicThread {
    pub id: String,
    pub name: String,
    pub description: String,
    pub keyword_logic: KeywordLogic,
    /// FK into `mem_tree_trees.id` (the backing topic tree).
    pub tree_id: String,
    pub created_at_ms: i64,
}

/// A topic plus its matching dimensions (keywords, pinned sources, pinned
/// entities). This is the shape returned to callers and used by the matcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicThreadDetail {
    #[serde(flatten)]
    pub thread: TopicThread,
    pub keywords: Vec<String>,
    /// Pinned source ids — a chunk from any of these is auto-included, no
    /// content analysis needed. Format matches `chunk.metadata.source_id`.
    pub source_pins: Vec<String>,
    /// Pinned canonical entity ids (`kind:surface`) — a chunk referencing any
    /// of these is included.
    pub entity_pins: Vec<String>,
    /// Pinned meeting-name substrings — a transcript whose `[Meeting: X]`
    /// contains any of these is included.
    pub meeting_pins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTopicInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub keyword_logic: KeywordLogic,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub entity_ids: Vec<String>,
    #[serde(default)]
    pub meeting_names: Vec<String>,
    /// When set (7/14/30), backfill historical chunks within N days right
    /// after creating the topic.
    #[serde(default)]
    pub backfill_days: Option<u32>,
}

/// Partial patch for `update_thread`. A `None` field is left unchanged; the
/// list fields, when `Some`, fully replace the stored set.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateTopicPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub keyword_logic: Option<KeywordLogic>,
    pub keywords: Option<Vec<String>>,
    pub source_ids: Option<Vec<String>>,
    pub entity_ids: Option<Vec<String>>,
    pub meeting_names: Option<Vec<String>>,
}

/// One node in a topic's timeline, with the full summary body hydrated from
/// disk (the SQLite `content` column is only a ≤500-char preview).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicTimelineNode {
    pub summary_id: String,
    pub level: u32,
    pub time_range_start_ms: i64,
    pub time_range_end_ms: i64,
    pub body: String,
}

/// A Teams conversation (1:1 or group chat) discovered during sync, for the
/// pin picker. `pin_value` is the exact string to store as a topic source_pin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamsConversation {
    pub conversation_id: String,
    pub source_id: String,
    pub label: String,
    pub chat_type: Option<String>,
    pub last_seen_ms: Option<i64>,
    /// `{source_id}:{conversation_id}` — matches chunks via the existing
    /// `mem_src:{pin}:` source-pin prefix rule.
    pub pin_value: String,
}

/// A person / email entity for the people picker. `entity_id` is the exact
/// string to store as a topic entity_pin (`person:first-last` or `email:addr`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonEntity {
    pub entity_id: String,
    pub surface: String,
    pub kind: String,
    pub count: u64,
}

/// A distinct meeting name discovered from transcripts, for the meeting picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingInfo {
    pub meeting_name: String,
    pub count: u64,
    pub last_seen_ms: Option<i64>,
}

/// Result of a backfill run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillResult {
    /// Chunks scanned in the time window.
    pub scanned: u64,
    /// Chunks that matched the topic's rules.
    pub matched: u64,
    /// AppendBuffer jobs newly enqueued (deduped ones not counted).
    pub enqueued: u64,
}
