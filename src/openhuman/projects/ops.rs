use crate::openhuman::config::Config;
use crate::openhuman::projects::{store, types::*};
use crate::rpc::RpcOutcome;
use serde::{Deserialize, Serialize};
use serde_json;
// ---------------------------------------------------------------------------
// Input / output shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateTaskInput {
    pub title: String,
    pub description: Option<String>,
    pub bucket_id: Option<String>,
    pub priority: Option<i64>,
    pub due_date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BucketsWithTasks {
    pub project: Project,
    pub buckets: Vec<BucketWithTasks>,
}

#[derive(Debug, Serialize)]
pub struct BucketWithTasks {
    pub bucket: Bucket,
    pub tasks: Vec<Task>,
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// Return the full Kanban board for the default project, with tasks grouped
/// by bucket and ordered by position within each bucket.
pub fn get_board(config: &Config) -> Result<RpcOutcome<BucketsWithTasks>, String> {
    let project_id = store::ensure_default_project(config).map_err(|e| e.to_string())?;

    let project = store::get_project(config, &project_id).map_err(|e| e.to_string())?;

    let buckets = store::list_buckets(config, &project_id).map_err(|e| e.to_string())?;
    let all_tasks = store::list_tasks(config, &project_id, None).map_err(|e| e.to_string())?;

    let buckets_with_tasks: Vec<BucketWithTasks> = buckets
        .into_iter()
        .map(|bucket| {
            let tasks: Vec<Task> = all_tasks
                .iter()
                .filter(|t| t.bucket_id == bucket.id)
                .cloned()
                .collect();
            BucketWithTasks { bucket, tasks }
        })
        .collect();

    log::debug!(
        "[projects] get_board project={project_id} buckets={} tasks={}",
        buckets_with_tasks.len(),
        all_tasks.len()
    );

    Ok(RpcOutcome::single_log(
        BucketsWithTasks {
            project,
            buckets: buckets_with_tasks,
        },
        "projects board loaded",
    ))
}

/// Create a new task in the default project.
///
/// If `bucket_id` is not specified the task is placed in the first bucket
/// (To Do, position 1000.0).
pub fn create_task(
    config: &Config,
    input: CreateTaskInput,
    actor: &str,
) -> Result<RpcOutcome<Task>, String> {
    let project_id = store::ensure_default_project(config).map_err(|e| e.to_string())?;

    // Resolve bucket: use provided or fall back to the first (lowest position) bucket.
    let bucket_id = if let Some(bid) = input.bucket_id {
        bid
    } else {
        let buckets = store::list_buckets(config, &project_id).map_err(|e| e.to_string())?;
        buckets
            .into_iter()
            .next()
            .map(|b| b.id)
            .ok_or_else(|| "no buckets found in default project".to_string())?
    };

    // Parse optional due_date string as RFC 3339 / ISO 8601.
    let due_date = input
        .due_date
        .as_deref()
        .map(|s| {
            s.parse::<chrono::DateTime<chrono::Utc>>()
                .map_err(|e| format!("invalid due_date '{s}': {e}"))
        })
        .transpose()?;

    let task = store::create_task(
        config,
        &project_id,
        &bucket_id,
        &input.title,
        input.description.as_deref(),
        input.priority.unwrap_or(0),
        due_date,
        actor,
    )
    .map_err(|e| e.to_string())?;

    log::debug!(
        "[projects] create_task id={} bucket={bucket_id} title={:?}",
        task.id,
        task.title
    );

    Ok(RpcOutcome::single_log(task, "task created"))
}

/// Apply a partial patch to an existing task.
pub fn update_task(
    config: &Config,
    task_id: &str,
    patch: TaskPatch,
    actor: &str,
) -> Result<RpcOutcome<Task>, String> {
    let task = store::update_task(config, task_id, &patch, actor).map_err(|e| e.to_string())?;
    log::debug!("[projects] update_task id={task_id}");
    Ok(RpcOutcome::single_log(
        task,
        format!("task updated: {task_id}"),
    ))
}

/// Move a task to a different bucket, optionally setting its position.
///
/// Internally this is a `TaskPatch` with `bucket_id` and optionally `position`.
pub fn move_task(
    config: &Config,
    task_id: &str,
    bucket_id: &str,
    position: Option<f64>,
    actor: &str,
) -> Result<RpcOutcome<Task>, String> {
    let patch = TaskPatch {
        bucket_id: Some(bucket_id.to_string()),
        position,
        ..TaskPatch::default()
    };
    let task = store::update_task(config, task_id, &patch, actor).map_err(|e| e.to_string())?;
    log::debug!("[projects] move_task id={task_id} bucket={bucket_id}");
    Ok(RpcOutcome::single_log(
        task,
        format!("task moved: {task_id} → {bucket_id}"),
    ))
}

/// Delete a task by id.
pub fn delete_task(config: &Config, task_id: &str) -> Result<RpcOutcome<()>, String> {
    store::delete_task(config, task_id).map_err(|e| e.to_string())?;
    log::debug!("[projects] delete_task id={task_id}");
    Ok(RpcOutcome::single_log(
        (),
        format!("task deleted: {task_id}"),
    ))
}

/// Apply a partial patch to a bucket (rename, reorder, change done-status).
pub fn update_bucket(
    config: &Config,
    bucket_id: &str,
    patch: BucketPatch,
) -> Result<RpcOutcome<Bucket>, String> {
    let bucket = store::update_bucket(config, bucket_id, &patch).map_err(|e| e.to_string())?;
    log::debug!("[projects] update_bucket id={bucket_id}");
    Ok(RpcOutcome::single_log(
        bucket,
        format!("bucket updated: {bucket_id}"),
    ))
}

/// Return all events (changes + comments) for a task, ordered by time.
pub fn list_task_events(
    config: &Config,
    task_id: &str,
) -> Result<RpcOutcome<Vec<TaskEvent>>, String> {
    let events = store::list_events(config, task_id).map_err(|e| e.to_string())?;
    log::debug!(
        "[projects] list_task_events task_id={task_id} count={}",
        events.len()
    );
    Ok(RpcOutcome::single_log(
        events,
        format!("events listed: {task_id}"),
    ))
}

/// Add a plain-text comment to a task.
pub fn add_comment(
    config: &Config,
    task_id: &str,
    actor: &str,
    body: &str,
) -> Result<RpcOutcome<TaskEvent>, String> {
    let event = store::add_comment(config, task_id, actor, body).map_err(|e| e.to_string())?;
    log::debug!("[projects] add_comment task_id={task_id} actor={actor}");
    Ok(RpcOutcome::single_log(
        event,
        format!("comment added: {task_id}"),
    ))
}

/// Attach a file (given its absolute path) to a task.
pub fn add_attachment(
    config: &Config,
    task_id: &str,
    src_path: &str,
    uploaded_by: &str,
) -> Result<RpcOutcome<TaskAttachment>, String> {
    let path = std::path::Path::new(src_path);
    let att =
        store::add_attachment(config, task_id, path, uploaded_by).map_err(|e| e.to_string())?;
    log::debug!(
        "[projects] add_attachment task_id={task_id} filename={} by={uploaded_by}",
        att.filename
    );
    Ok(RpcOutcome::single_log(
        att,
        format!("attachment added: {task_id}"),
    ))
}

/// List all attachments for a task.
pub fn list_attachments(
    config: &Config,
    task_id: &str,
) -> Result<RpcOutcome<Vec<TaskAttachment>>, String> {
    let atts = store::list_attachments(config, task_id).map_err(|e| e.to_string())?;
    log::debug!(
        "[projects] list_attachments task_id={task_id} count={}",
        atts.len()
    );
    Ok(RpcOutcome::single_log(
        atts,
        format!("attachments listed: {task_id}"),
    ))
}

/// Delete an attachment by id (removes DB row and file).
pub fn delete_attachment(
    config: &Config,
    attachment_id: &str,
) -> Result<RpcOutcome<serde_json::Value>, String> {
    store::delete_attachment(config, attachment_id).map_err(|e| e.to_string())?;
    let result = serde_json::json!({ "attachment_id": attachment_id, "deleted": true });
    Ok(RpcOutcome::single_log(
        result,
        format!("attachment deleted: {attachment_id}"),
    ))
}
