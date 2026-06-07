use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub bucket_id: Option<String>,
    pub priority: Option<i64>,
    pub due_date: Option<Option<DateTime<Utc>>>,
    pub hex_color: Option<Option<String>>,
    pub position: Option<f64>,
    pub done: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketPatch {
    pub title: Option<String>,
    pub position: Option<f64>,
    pub is_done_bucket: Option<bool>,
}
