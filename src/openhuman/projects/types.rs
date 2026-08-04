use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ---------------------------------------------------------------------------
// Task events (change feed + comments)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    Change,
    Comment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskEvent {
    pub id: String,
    pub task_id: String,
    pub kind: TaskEventKind,
    /// "me" | "ai"
    pub actor: String,
    /// For kind=Change: the field name that changed (e.g. "bucket_id", "priority").
    pub field: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    /// For kind=Comment: the plain-text body.
    pub body: Option<String>,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Task attachments
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskAttachment {
    pub id: String,
    pub task_id: String,
    pub filename: String,
    pub mime_type: String,
    /// Path relative to workspace_dir.
    pub rel_path: String,
    pub size_bytes: i64,
    /// "me" | "ai"
    pub uploaded_by: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// double_option helpers — distinguish absent (→ None) from null (→ Some(None))
// ---------------------------------------------------------------------------

/// Deserialize: absent → `None`, `null` → `Some(None)`, value → `Some(Some(v))`.
fn deserialize_double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    // We use `Option<T>` to let serde map `null` → `None` and value → `Some`.
    // The outer `Option` (absent vs present) is handled by `#[serde(default)]`.
    let inner: Option<T> = Option::deserialize(de)?;
    Ok(Some(inner))
}

/// Serialize: `None` → field omitted (handled by `#[serde(skip_serializing_if)]`),
/// `Some(None)` → `null`, `Some(Some(v))` → `v`.
fn serialize_double_option<T, S>(value: &Option<Option<T>>, ser: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    match value {
        None => ser.serialize_none(),
        Some(inner) => inner.serialize(ser),
    }
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bucket {
    pub id: String,
    pub project_id: String,
    pub title: String,
    /// Float position for ordering — same pattern as Vikunja.
    pub position: f64,
    /// When true, tasks moved here are automatically marked done=true.
    pub is_done_bucket: bool,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub bucket_id: String,
    pub title: String,
    pub description: Option<String>,
    pub done: bool,
    pub done_at: Option<DateTime<Utc>>,
    pub priority: i64,
    pub due_date: Option<DateTime<Utc>>,
    pub hex_color: Option<String>,
    pub position: f64,
    pub index: i64,
    /// `'me'` = human, `'ai'` = AI assignee, `None` = unassigned.
    pub assignee: Option<String>,
    /// Reserved for orchestrator: JSON blob describing which agent/skill will
    /// execute the task. Always `None` in Phase 1.
    pub ai_plan: Option<String>,
    /// Parent task id — `None` means this is a top-level task.
    pub parent_task_id: Option<String>,
    /// Claude Code settings-profile id (from the `claude_profiles` registry)
    /// this task's AI run should launch with. `None` → default provider auth.
    #[serde(default)]
    pub settings_profile: Option<String>,
    /// Model tier alias (`opus`/`sonnet`/`haiku`/`default`) or a concrete model
    /// id for this task's AI run. Resolved against the profile at pickup.
    #[serde(default)]
    pub model: Option<String>,
    /// Fallback ladder direction: `"up"` / `"down"` / `None` = fallback off.
    /// When set, a failed startup steps along the global ladder from the start
    /// step (settings_profile+model) toward `fallback_end`.
    #[serde(default)]
    pub fallback_direction: Option<String>,
    /// Fallback terminus, encoded `"<profile_id>:<tier>"`. `None` = walk to the
    /// ladder boundary in `fallback_direction`.
    #[serde(default)]
    pub fallback_end: Option<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    /// When true this task is hidden from the active board and shown only in the Archived view.
    #[serde(default)]
    pub archived: bool,
    pub archived_at: Option<DateTime<Utc>>,
    /// Feishu resume group already opened for this task, if any. Set when the
    /// user clicks the completion card's "resume in group" button; reused on
    /// subsequent clicks so we don't create a duplicate group.
    #[serde(default)]
    pub feishu_resume_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskPatch {
    pub title: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<Option<String>>,
    pub bucket_id: Option<String>,
    pub priority: Option<i64>,
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub due_date: Option<Option<DateTime<Utc>>>,
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub hex_color: Option<Option<String>>,
    /// `Some(Some("me"))` / `Some(Some("ai"))` = set. `Some(None)` = clear. `None` = no change.
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub assignee: Option<Option<String>>,
    pub position: Option<f64>,
    pub done: Option<bool>,
    pub ai_plan: Option<String>,
    pub archived: Option<bool>,
    /// `Some(Some(id))` = set profile, `Some(None)` = clear, `None` = no change.
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub settings_profile: Option<Option<String>>,
    /// `Some(Some(model))` = set, `Some(None)` = clear, `None` = no change.
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub model: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub fallback_direction: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub fallback_end: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BucketPatch {
    pub title: Option<String>,
    pub position: Option<f64>,
    pub is_done_bucket: Option<bool>,
}

// ---------------------------------------------------------------------------
// AI run history (one row per AI task run)
// ---------------------------------------------------------------------------

/// A persisted record of one AI project-task run. Written when a run starts
/// (`status="running"`) and updated to a terminal status when it finishes, so
/// crashed/interrupted runs leave a trace. `task_title`/`model` are denormalized
/// snapshots so history survives task rename/deletion. Mirrors cron's `CronRun`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectTaskRun {
    pub run_id: String,
    pub task_id: String,
    /// Denormalized task title at run time.
    pub task_title: String,
    /// Concrete model that actually ran (e.g. "claude-opus-latest"), if known.
    pub model: Option<String>,
    /// Settings-profile id the run started with, if any.
    pub profile_id: Option<String>,
    /// Tier alias the run started with (opus/sonnet/haiku/default), if any.
    pub tier: Option<String>,
    /// Number of attempts in the configured fallback chain (1 = no fallback).
    pub fallback_steps: i64,
    /// Index of the attempt that actually ran (0 = start, >0 = fell back).
    pub fallback_used: i64,
    pub started_at: DateTime<Utc>,
    /// None while still running.
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: i64,
    /// running | done | blocked | cancelled | error | interrupted
    pub status: String,
}

/// Binds a Feishu group chat to a task's Claude Code session so messages sent
/// in that group resume (`--resume`) the same CC session. Keyed by chat_id
/// (the reverse lookup dispatch needs: it only has the inbound chat_id).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeishuSessionBinding {
    pub chat_id: String,
    pub task_id: String,
    pub claude_session_id: String,
    pub claude_workspace_dir: String,
    pub created: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TaskPatch: absent field → None (outer None), JSON null → Some(None).
    #[test]
    fn task_patch_absent_vs_null_description() {
        // Absent field deserialises to None (outer).
        let absent: TaskPatch = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.description, None);

        // Explicit JSON null deserialises to Some(None).
        let null: TaskPatch = serde_json::from_str(r#"{"description": null}"#).unwrap();
        assert_eq!(null.description, Some(None));

        // A string value deserialises to Some(Some("...")).
        let value: TaskPatch = serde_json::from_str(r#"{"description": "hello"}"#).unwrap();
        assert_eq!(value.description, Some(Some("hello".to_string())));
    }

    #[test]
    fn task_patch_absent_vs_null_due_date() {
        let absent: TaskPatch = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.due_date, None);

        let null: TaskPatch = serde_json::from_str(r#"{"due_date": null}"#).unwrap();
        assert_eq!(null.due_date, Some(None));
    }

    #[test]
    fn task_patch_absent_vs_null_hex_color() {
        let absent: TaskPatch = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.hex_color, None);

        let null: TaskPatch = serde_json::from_str(r#"{"hex_color": null}"#).unwrap();
        assert_eq!(null.hex_color, Some(None));

        let value: TaskPatch = serde_json::from_str("{\"hex_color\": \"#ff0000\"}").unwrap();
        assert_eq!(value.hex_color, Some(Some("#ff0000".to_string())));
    }

    /// TaskPatch roundtrip: serialise then deserialise preserves values.
    #[test]
    fn task_patch_serde_roundtrip() {
        let original = TaskPatch {
            title: Some("Do the thing".to_string()),
            description: Some(Some("Details here".to_string())),
            bucket_id: None,
            priority: Some(3),
            due_date: Some(None),
            hex_color: Some(Some("#aabbcc".to_string())),
            assignee: None,
            position: Some(1.5),
            done: Some(false),
            ai_plan: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: TaskPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    /// BucketPatch: missing fields default to None.
    #[test]
    fn bucket_patch_missing_fields() {
        let patch: BucketPatch = serde_json::from_str("{}").unwrap();
        assert_eq!(patch, BucketPatch::default());
        assert_eq!(patch.title, None);
        assert_eq!(patch.position, None);
        assert_eq!(patch.is_done_bucket, None);
    }

    /// BucketPatch roundtrip.
    #[test]
    fn bucket_patch_serde_roundtrip() {
        let original = BucketPatch {
            title: Some("Backlog".to_string()),
            position: Some(2.0),
            is_done_bucket: Some(true),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: BucketPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn task_patch_absent_vs_null_assignee() {
        let absent: TaskPatch = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.assignee, None);

        let null: TaskPatch = serde_json::from_str(r#"{"assignee": null}"#).unwrap();
        assert_eq!(null.assignee, Some(None));

        let me_val: TaskPatch = serde_json::from_str(r#"{"assignee": "me"}"#).unwrap();
        assert_eq!(me_val.assignee, Some(Some("me".to_string())));

        let ai_val: TaskPatch = serde_json::from_str(r#"{"assignee": "ai"}"#).unwrap();
        assert_eq!(ai_val.assignee, Some(Some("ai".to_string())));
    }
}
