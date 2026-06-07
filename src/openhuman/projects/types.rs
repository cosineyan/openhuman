use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
fn serialize_double_option<T, S>(
    value: &Option<Option<T>>,
    ser: S,
) -> Result<S::Ok, S::Error>
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
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
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
    pub position: Option<f64>,
    pub done: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BucketPatch {
    pub title: Option<String>,
    pub position: Option<f64>,
    pub is_done_bucket: Option<bool>,
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
        let value: TaskPatch =
            serde_json::from_str(r#"{"description": "hello"}"#).unwrap();
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

        let value: TaskPatch =
            serde_json::from_str("{\"hex_color\": \"#ff0000\"}").unwrap();
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
            position: Some(1.5),
            done: Some(false),
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
}
