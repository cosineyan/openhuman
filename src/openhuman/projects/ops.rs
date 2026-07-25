use crate::openhuman::config::Config;
use crate::openhuman::projects::{store, types::*};
use crate::rpc::RpcOutcome;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
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
    pub parent_task_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BucketsWithTasks {
    pub project: Project,
    pub buckets: Vec<BucketWithTasks>,
    /// Map of task_id -> (total_subtasks, done_subtasks).
    pub subtask_counts: HashMap<String, (usize, usize)>,
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
/// Auto-archives tasks that haven't been updated in 14 days before returning.
pub fn get_board(config: &Config) -> Result<RpcOutcome<BucketsWithTasks>, String> {
    let project_id = store::ensure_default_project(config).map_err(|e| e.to_string())?;

    // Auto-archive tasks not updated for 14 days
    let archived_count = store::auto_archive_stale_tasks(config, &project_id, 14)
        .unwrap_or(0);
    if archived_count > 0 {
        log::info!("[projects] auto-archived {archived_count} stale task(s) (14-day rule)");
    }

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

    // Count subtasks per parent task
    let subtask_counts = store::count_subtasks_by_parent(config, &project_id).unwrap_or_default();

    log::debug!(
        "[projects] get_board project={project_id} buckets={} tasks={}",
        buckets_with_tasks.len(),
        all_tasks.len()
    );

    Ok(RpcOutcome::single_log(
        BucketsWithTasks {
            project,
            buckets: buckets_with_tasks,
            subtask_counts,
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
        input.parent_task_id.as_deref(),
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
    // If this is a subtask, also log a summary entry on the parent's feed
    if let Some(parent_id) = &task.parent_task_id {
        let _ = store::log_change(
            config,
            parent_id,
            actor,
            "subtask_updated",
            None,
            Some(&task.title),
        );
    }
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

/// List archived tasks for the default project with optional search and date filters.
pub fn list_archived_tasks(
    config: &Config,
    search: Option<&str>,
    created_after: Option<chrono::DateTime<chrono::Utc>>,
    created_before: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<RpcOutcome<Vec<Task>>, String> {
    let project_id = store::ensure_default_project(config).map_err(|e| e.to_string())?;
    let tasks = store::list_archived_tasks(config, &project_id, search, created_after, created_before)
        .map_err(|e| e.to_string())?;
    log::debug!("[projects] list_archived_tasks n={}", tasks.len());
    Ok(RpcOutcome::single_log(tasks, "projects: list_archived_tasks"))
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

/// List all subtasks for a given parent task.
pub fn list_subtasks(
    config: &Config,
    parent_task_id: &str,
) -> Result<RpcOutcome<Vec<Task>>, String> {
    let tasks = store::list_subtasks(config, parent_task_id).map_err(|e| e.to_string())?;
    log::debug!(
        "[projects] list_subtasks parent={parent_task_id} count={}",
        tasks.len()
    );
    Ok(RpcOutcome::single_log(
        tasks,
        format!("subtasks listed: {parent_task_id}"),
    ))
}

/// Create a subtask under a parent task.
pub fn create_subtask(
    config: &Config,
    parent_task_id: &str,
    title: &str,
    actor: &str,
) -> Result<RpcOutcome<Task>, String> {
    // Subtask inherits the parent's project and bucket
    let project_id = store::ensure_default_project(config).map_err(|e| e.to_string())?;
    let buckets = store::list_buckets(config, &project_id).map_err(|e| e.to_string())?;
    let bucket_id = buckets
        .into_iter()
        .next()
        .map(|b| b.id)
        .ok_or_else(|| "no buckets found".to_string())?;

    let task = store::create_task(
        config,
        &project_id,
        &bucket_id,
        title,
        None,
        0,
        None,
        actor,
        Some(parent_task_id),
    )
    .map_err(|e| e.to_string())?;

    // Log on the parent task's feed
    let _ = store::log_change(
        config,
        parent_task_id,
        actor,
        "subtask_added",
        None,
        Some(title),
    );

    log::debug!(
        "[projects] create_subtask id={} parent={parent_task_id}",
        task.id
    );
    Ok(RpcOutcome::single_log(task, "subtask created"))
}

/// Delete a subtask and log on the parent's feed.
pub fn delete_subtask(
    config: &Config,
    subtask_id: &str,
    actor: &str,
) -> Result<RpcOutcome<serde_json::Value>, String> {
    // Fetch subtask title + parent_id before deleting
    let subtask = store::get_task(config, subtask_id).map_err(|e| e.to_string())?;
    let parent_id = subtask.parent_task_id.clone();
    let title = subtask.title.clone();

    store::delete_task(config, subtask_id).map_err(|e| e.to_string())?;

    // Log on the parent task's feed
    if let Some(pid) = &parent_id {
        let _ = store::log_change(config, pid, actor, "subtask_removed", Some(&title), None);
    }

    let result = serde_json::json!({ "task_id": subtask_id, "deleted": true });
    Ok(RpcOutcome::single_log(
        result,
        format!("subtask deleted: {subtask_id}"),
    ))
}

/// Hard-cancel an in-flight AI task. Cancels the CancellationToken registered
/// for this task, which causes `run_ai_task` to write a comment and move to Blocked.
/// Returns `true` when a running task was found and cancelled.
pub fn cancel_ai_task(task_id: &str) -> RpcOutcome<serde_json::Value> {
    let cancelled = crate::openhuman::projects::run_registry::cancel(task_id);
    log::debug!("[projects] cancel_ai_task task={task_id} found={cancelled}");
    RpcOutcome::single_log(
        serde_json::json!({ "cancelled": cancelled }),
        format!("cancel_ai_task task={task_id} cancelled={cancelled}"),
    )
}

/// List task IDs that currently have a registered CancellationToken (i.e. are
/// actively being processed by the AI runner).
pub fn list_running_ai_tasks() -> RpcOutcome<serde_json::Value> {
    let task_ids = crate::openhuman::projects::run_registry::list_running();
    RpcOutcome::single_log(
        serde_json::json!({ "task_ids": task_ids }),
        format!("list_running_ai_tasks count={}", task_ids.len()),
    )
}
