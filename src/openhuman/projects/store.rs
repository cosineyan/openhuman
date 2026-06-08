use crate::openhuman::config::Config;
use crate::openhuman::projects::{
    Bucket, BucketPatch, Project, Task, TaskAttachment, TaskEvent, TaskEventKind, TaskPatch,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS projects (
    id      TEXT PRIMARY KEY,
    title   TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_buckets (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title          TEXT NOT NULL,
    position       REAL NOT NULL DEFAULT 0.0,
    is_done_bucket INTEGER NOT NULL DEFAULT 0,
    created        TEXT NOT NULL,
    updated        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_buckets_project ON project_buckets(project_id, position);

CREATE TABLE IF NOT EXISTS project_tasks (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    bucket_id   TEXT NOT NULL REFERENCES project_buckets(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    description TEXT,
    done        INTEGER NOT NULL DEFAULT 0,
    done_at     TEXT,
    priority    INTEGER NOT NULL DEFAULT 0,
    due_date    TEXT,
    hex_color   TEXT,
    position    REAL NOT NULL DEFAULT 0.0,
    idx         INTEGER NOT NULL DEFAULT 0,
    created     TEXT NOT NULL,
    updated     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_bucket  ON project_tasks(bucket_id, position);
CREATE INDEX IF NOT EXISTS idx_tasks_project ON project_tasks(project_id);

CREATE TABLE IF NOT EXISTS project_task_events (
    id        TEXT PRIMARY KEY,
    task_id   TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    kind      TEXT NOT NULL,
    actor     TEXT NOT NULL,
    field     TEXT,
    old_value TEXT,
    new_value TEXT,
    body      TEXT,
    created   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_task_events ON project_task_events(task_id, created);

CREATE TABLE IF NOT EXISTS project_task_attachments (
    id           TEXT PRIMARY KEY,
    task_id      TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    filename     TEXT NOT NULL,
    mime_type    TEXT NOT NULL,
    rel_path     TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL DEFAULT 0,
    uploaded_by  TEXT NOT NULL DEFAULT 'me',
    created      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_attachments_task ON project_task_attachments(task_id, created);
";

// ---------------------------------------------------------------------------
// Connection helper
// ---------------------------------------------------------------------------

pub fn with_connection<T>(config: &Config, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let db_path = config.workspace_dir.join("projects").join("projects.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create projects directory: {}", parent.display())
        })?;
    }

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Failed to open projects DB: {}", db_path.display()))?;

    conn.execute_batch(SCHEMA)
        .context("Failed to initialize projects schema")?;

    add_column_if_missing(&conn, "project_tasks", "assignee", "TEXT")?;
    add_column_if_missing(&conn, "project_tasks", "ai_plan", "TEXT")?;

    f(&conn)
}

// ---------------------------------------------------------------------------
// Row mappers (private)
// ---------------------------------------------------------------------------

fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let created_raw: String = row.get(2)?;
    let updated_raw: String = row.get(3)?;
    Ok(Project {
        id: row.get(0)?,
        title: row.get(1)?,
        created: parse_rfc3339(&created_raw).map_err(sql_err)?,
        updated: parse_rfc3339(&updated_raw).map_err(sql_err)?,
    })
}

fn row_to_bucket(row: &rusqlite::Row<'_>) -> rusqlite::Result<Bucket> {
    let created_raw: String = row.get(5)?;
    let updated_raw: String = row.get(6)?;
    Ok(Bucket {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        position: row.get(3)?,
        is_done_bucket: row.get::<_, i64>(4)? != 0,
        created: parse_rfc3339(&created_raw).map_err(sql_err)?,
        updated: parse_rfc3339(&updated_raw).map_err(sql_err)?,
    })
}

fn row_to_task_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskEvent> {
    let kind_str: String = row.get(2)?;
    let created_raw: String = row.get(8)?;
    let kind = match kind_str.as_str() {
        "comment" => TaskEventKind::Comment,
        _ => TaskEventKind::Change,
    };
    Ok(TaskEvent {
        id: row.get(0)?,
        task_id: row.get(1)?,
        kind,
        actor: row.get(3)?,
        field: row.get(4)?,
        old_value: row.get(5)?,
        new_value: row.get(6)?,
        body: row.get(7)?,
        created: parse_rfc3339(&created_raw).map_err(sql_err)?,
    })
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let done_at_raw: Option<String> = row.get(6)?;
    let due_date_raw: Option<String> = row.get(8)?;
    let created_raw: String = row.get(12)?;
    let updated_raw: String = row.get(13)?;
    Ok(Task {
        id: row.get(0)?,
        project_id: row.get(1)?,
        bucket_id: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        done: row.get::<_, i64>(5)? != 0,
        done_at: done_at_raw
            .as_deref()
            .map(parse_rfc3339)
            .transpose()
            .map_err(sql_err)?,
        priority: row.get(7)?,
        due_date: due_date_raw
            .as_deref()
            .map(parse_rfc3339)
            .transpose()
            .map_err(sql_err)?,
        hex_color: row.get(9)?,
        position: row.get(10)?,
        index: row.get(11)?,
        assignee: row.get(14)?,
        ai_plan: row.get(15)?,
        created: parse_rfc3339(&created_raw).map_err(sql_err)?,
        updated: parse_rfc3339(&updated_raw).map_err(sql_err)?,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_rfc3339(raw: &str) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(raw)
        .with_context(|| format!("Invalid RFC3339 timestamp in projects DB: {raw}"))?;
    Ok(parsed.with_timezone(&Utc))
}

fn sql_err(err: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(err.into())
}

// ---------------------------------------------------------------------------
// Default project
// ---------------------------------------------------------------------------

/// Idempotently ensures a default project and its four default buckets exist.
/// Returns the project id (existing or newly created).
pub fn ensure_default_project(config: &Config) -> Result<String> {
    with_connection(config, |conn| {
        // Check for existing default project.
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM projects WHERE title = 'Project' OR title = 'Default' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            // Migrate old "Default" title to "Project"
            conn.execute(
                "UPDATE projects SET title = 'Project' WHERE id = ?1 AND title = 'Default'",
                params![id],
            )
            .ok();
            log::debug!("[projects] default project already exists id={id}");
            return Ok(id);
        }

        let now = Utc::now();
        let project_id = Uuid::new_v4().to_string();

        conn.execute(
            "INSERT INTO projects (id, title, created, updated) VALUES (?1, 'Project', ?2, ?3)",
            params![project_id, now.to_rfc3339(), now.to_rfc3339()],
        )
        .context("Failed to insert default project")?;

        log::debug!("[projects] created default project id={project_id}");

        // Insert the four default buckets.
        let buckets = [
            ("To Do", 1000.0_f64, 0_i64),
            ("Doing", 2000.0, 0),
            ("Blocked", 3000.0, 0),
            ("Done", 4000.0, 1),
        ];

        for (title, position, is_done) in &buckets {
            let bucket_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO project_buckets
                 (id, project_id, title, position, is_done_bucket, created, updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    bucket_id,
                    project_id,
                    title,
                    position,
                    is_done,
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )
            .with_context(|| format!("Failed to insert default bucket '{title}'"))?;
        }

        Ok(project_id)
    })
}

/// Load a project by id.
pub fn get_project(config: &Config, project_id: &str) -> Result<Project> {
    with_connection(config, |conn| {
        conn.query_row(
            "SELECT id, title, created, updated FROM projects WHERE id = ?1",
            params![project_id],
            row_to_project,
        )
        .with_context(|| format!("Project '{project_id}' not found"))
    })
}

// ---------------------------------------------------------------------------
// Buckets
// ---------------------------------------------------------------------------

pub fn list_buckets(config: &Config, project_id: &str) -> Result<Vec<Bucket>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, title, position, is_done_bucket, created, updated
             FROM project_buckets
             WHERE project_id = ?1
             ORDER BY position ASC",
        )?;
        let rows = stmt.query_map(params![project_id], row_to_bucket)?;
        let mut buckets = Vec::new();
        for row in rows {
            buckets.push(row?);
        }
        Ok(buckets)
    })
}

pub fn update_bucket(config: &Config, bucket_id: &str, patch: &BucketPatch) -> Result<Bucket> {
    let now = Utc::now();
    with_connection(config, |conn| {
        // Read current
        let bucket: Bucket = conn
            .query_row(
                "SELECT id, project_id, title, position, is_done_bucket, created, updated
                 FROM project_buckets WHERE id = ?1",
                params![bucket_id],
                row_to_bucket,
            )
            .with_context(|| format!("Bucket '{bucket_id}' not found"))?;

        let new_title = patch.title.as_deref().unwrap_or(&bucket.title);
        let new_position = patch.position.unwrap_or(bucket.position);
        let new_is_done = patch
            .is_done_bucket
            .map(|v| if v { 1_i64 } else { 0 })
            .unwrap_or(if bucket.is_done_bucket { 1 } else { 0 });

        conn.execute(
            "UPDATE project_buckets
             SET title = ?1, position = ?2, is_done_bucket = ?3, updated = ?4
             WHERE id = ?5",
            params![
                new_title,
                new_position,
                new_is_done,
                now.to_rfc3339(),
                bucket_id
            ],
        )
        .context("Failed to update bucket")?;

        let updated = conn.query_row(
            "SELECT id, project_id, title, position, is_done_bucket, created, updated
             FROM project_buckets WHERE id = ?1",
            params![bucket_id],
            row_to_bucket,
        )?;
        Ok(updated)
    })
}

// ---------------------------------------------------------------------------
// Task events (change feed + comments)
// ---------------------------------------------------------------------------

pub fn log_change(
    config: &Config,
    task_id: &str,
    actor: &str,
    field: &str,
    old_value: Option<&str>,
    new_value: Option<&str>,
) -> Result<()> {
    let now = Utc::now();
    let id = Uuid::new_v4().to_string();
    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO project_task_events (id, task_id, kind, actor, field, old_value, new_value, body, created)
             VALUES (?1, ?2, 'change', ?3, ?4, ?5, ?6, NULL, ?7)",
            params![id, task_id, actor, field, old_value, new_value, now.to_rfc3339()],
        )
        .context("Failed to insert change event")?;
        Ok(())
    })
}

pub fn add_comment(config: &Config, task_id: &str, actor: &str, body: &str) -> Result<TaskEvent> {
    let now = Utc::now();
    let id = Uuid::new_v4().to_string();
    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO project_task_events (id, task_id, kind, actor, field, old_value, new_value, body, created)
             VALUES (?1, ?2, 'comment', ?3, NULL, NULL, NULL, ?4, ?5)",
            params![id, task_id, actor, body, now.to_rfc3339()],
        )
        .context("Failed to insert comment event")?;

        let event = conn.query_row(
            "SELECT id, task_id, kind, actor, field, old_value, new_value, body, created
             FROM project_task_events WHERE id = ?1",
            params![id],
            row_to_task_event,
        )?;
        Ok(event)
    })
}

pub fn list_events(config: &Config, task_id: &str) -> Result<Vec<TaskEvent>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, task_id, kind, actor, field, old_value, new_value, body, created
             FROM project_task_events
             WHERE task_id = ?1
             ORDER BY created ASC",
        )?;
        let rows = stmt.query_map(params![task_id], row_to_task_event)?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    })
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

pub fn list_tasks(config: &Config, project_id: &str, bucket_id: Option<&str>) -> Result<Vec<Task>> {
    with_connection(config, |conn| {
        let mut tasks = Vec::new();
        if let Some(bid) = bucket_id {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, bucket_id, title, description,
                        done, done_at, priority, due_date, hex_color,
                        position, idx, created, updated,
                        assignee, ai_plan
                 FROM project_tasks
                 WHERE project_id = ?1 AND bucket_id = ?2
                 ORDER BY position ASC",
            )?;
            let rows = stmt.query_map(params![project_id, bid], row_to_task)?;
            for row in rows {
                tasks.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, bucket_id, title, description,
                        done, done_at, priority, due_date, hex_color,
                        position, idx, created, updated,
                        assignee, ai_plan
                 FROM project_tasks
                 WHERE project_id = ?1
                 ORDER BY position ASC",
            )?;
            let rows = stmt.query_map(params![project_id], row_to_task)?;
            for row in rows {
                tasks.push(row?);
            }
        }
        Ok(tasks)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn create_task(
    config: &Config,
    project_id: &str,
    bucket_id: &str,
    title: &str,
    description: Option<&str>,
    priority: i64,
    due_date: Option<DateTime<Utc>>,
    actor: &str,
) -> Result<Task> {
    let now = Utc::now();
    let task_id = Uuid::new_v4().to_string();

    let task = with_connection(config, |conn| {
        // Compute next position (max + 1000.0) and next idx (max + 1)
        let max_pos: f64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), 0.0) FROM project_tasks WHERE bucket_id = ?1",
                params![bucket_id],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        let next_pos = max_pos + 1000.0;

        let max_idx: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(idx), 0) FROM project_tasks WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let next_idx = max_idx + 1;

        let due_str = due_date.map(|d| d.to_rfc3339());

        conn.execute(
            "INSERT INTO project_tasks
             (id, project_id, bucket_id, title, description,
              done, done_at, priority, due_date, hex_color,
              position, idx, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, NULL, ?6, ?7, NULL, ?8, ?9, ?10, ?11)",
            params![
                task_id,
                project_id,
                bucket_id,
                title,
                description,
                priority,
                due_str,
                next_pos,
                next_idx,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )
        .context("Failed to insert task")?;

        let task = conn.query_row(
            "SELECT id, project_id, bucket_id, title, description,
                    done, done_at, priority, due_date, hex_color,
                    position, idx, created, updated,
                    assignee, ai_plan
             FROM project_tasks WHERE id = ?1",
            params![task_id],
            row_to_task,
        )?;
        Ok(task)
    })?;

    // Log a "created" event outside the connection closure.
    let _ = log_change(config, &task.id, actor, "created", None, Some(&task.title));

    Ok(task)
}

pub fn update_task(config: &Config, task_id: &str, patch: &TaskPatch, actor: &str) -> Result<Task> {
    let now = Utc::now();

    let result = with_connection(config, |conn| {
        // Read current task.
        let task: Task = conn
            .query_row(
                "SELECT id, project_id, bucket_id, title, description,
                        done, done_at, priority, due_date, hex_color,
                        position, idx, created, updated,
                        assignee, ai_plan
                 FROM project_tasks WHERE id = ?1",
                params![task_id],
                row_to_task,
            )
            .with_context(|| format!("Task '{task_id}' not found"))?;

        // Resolve patched bucket_id.
        let new_bucket_id = patch.bucket_id.as_deref().unwrap_or(&task.bucket_id);

        // Check if the target bucket is a done bucket (for auto-done logic).
        let target_is_done_bucket: bool = conn
            .query_row(
                "SELECT is_done_bucket FROM project_buckets WHERE id = ?1",
                params![new_bucket_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|v| v != 0)
            .unwrap_or(false);

        // Check if the source bucket was a done bucket (for auto-undone logic).
        let source_is_done_bucket: bool = conn
            .query_row(
                "SELECT is_done_bucket FROM project_buckets WHERE id = ?1",
                params![task.bucket_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|v| v != 0)
            .unwrap_or(false);

        let new_title = patch.title.as_deref().unwrap_or(&task.title);

        let new_description: Option<&str> = match &patch.description {
            None => task.description.as_deref(),
            Some(None) => None,
            Some(Some(s)) => Some(s.as_str()),
        };

        let new_priority = patch.priority.unwrap_or(task.priority);
        let new_position = patch.position.unwrap_or(task.position);

        let new_due_date: Option<DateTime<Utc>> = match &patch.due_date {
            None => task.due_date,
            Some(None) => None,
            Some(Some(d)) => Some(*d),
        };

        let new_hex_color: Option<&str> = match &patch.hex_color {
            None => task.hex_color.as_deref(),
            Some(None) => None,
            Some(Some(s)) => Some(s.as_str()),
        };

        // done field: explicit patch wins; moving to done bucket auto-sets; moving away auto-clears.
        let new_done = if let Some(explicit) = patch.done {
            explicit
        } else if target_is_done_bucket && new_bucket_id != task.bucket_id {
            true
        } else if source_is_done_bucket && new_bucket_id != task.bucket_id && !target_is_done_bucket
        {
            false
        } else {
            task.done
        };

        // done_at: set to now when transitioning to done, clear when undone.
        let new_done_at: Option<DateTime<Utc>> = if new_done && !task.done {
            Some(now)
        } else if !new_done {
            None
        } else {
            task.done_at
        };

        let due_str = new_due_date.map(|d| d.to_rfc3339());
        let done_at_str = new_done_at.map(|d| d.to_rfc3339());

        conn.execute(
            "UPDATE project_tasks
             SET bucket_id = ?1, title = ?2, description = ?3,
                 done = ?4, done_at = ?5, priority = ?6, due_date = ?7,
                 hex_color = ?8, position = ?9, updated = ?10
             WHERE id = ?11",
            params![
                new_bucket_id,
                new_title,
                new_description,
                if new_done { 1_i64 } else { 0 },
                done_at_str,
                new_priority,
                due_str,
                new_hex_color,
                new_position,
                now.to_rfc3339(),
                task_id,
            ],
        )
        .context("Failed to update task")?;

        if let Some(assignee_val) = &patch.assignee {
            let now_str = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE project_tasks SET assignee = ?1, updated = ?2 WHERE id = ?3",
                params![assignee_val, now_str, task_id],
            )?;
        }

        log::debug!("[projects] update_task id={task_id} bucket={new_bucket_id} done={new_done}");

        let updated = conn.query_row(
            "SELECT id, project_id, bucket_id, title, description,
                    done, done_at, priority, due_date, hex_color,
                    position, idx, created, updated,
                    assignee, ai_plan
             FROM project_tasks WHERE id = ?1",
            params![task_id],
            row_to_task,
        )?;
        Ok((task, updated))
    })?;

    // Log field changes outside the connection closure.
    let (old_task, updated) = result;
    if patch.title.is_some() && old_task.title != updated.title {
        let _ = log_change(
            config,
            task_id,
            actor,
            "title",
            Some(&old_task.title),
            Some(&updated.title),
        );
    }
    if patch.description.is_some() && old_task.description != updated.description {
        let _ = log_change(config, task_id, actor, "description", None, None);
    }
    if patch.bucket_id.is_some() && old_task.bucket_id != updated.bucket_id {
        let _ = log_change(
            config,
            task_id,
            actor,
            "bucket_id",
            Some(&old_task.bucket_id),
            Some(&updated.bucket_id),
        );
    }
    if patch.priority.is_some() && old_task.priority != updated.priority {
        let _ = log_change(
            config,
            task_id,
            actor,
            "priority",
            Some(&old_task.priority.to_string()),
            Some(&updated.priority.to_string()),
        );
    }
    if patch.done.is_some() && old_task.done != updated.done {
        let _ = log_change(
            config,
            task_id,
            actor,
            "done",
            Some(&old_task.done.to_string()),
            Some(&updated.done.to_string()),
        );
    }
    if patch.due_date.is_some() {
        let old_val = old_task.due_date.as_ref().map(|d| d.to_rfc3339());
        let new_val = updated.due_date.as_ref().map(|d| d.to_rfc3339());
        if old_val != new_val {
            let _ = log_change(
                config,
                task_id,
                actor,
                "due_date",
                old_val.as_deref(),
                new_val.as_deref(),
            );
        }
    }
    if patch.assignee.is_some() && old_task.assignee != updated.assignee {
        let _ = log_change(
            config,
            task_id,
            actor,
            "assignee",
            old_task.assignee.as_deref(),
            updated.assignee.as_deref(),
        );
    }

    Ok(updated)
}

pub fn delete_task(config: &Config, task_id: &str) -> Result<()> {
    let changed = with_connection(config, |conn| {
        conn.execute("DELETE FROM project_tasks WHERE id = ?1", params![task_id])
            .context("Failed to delete task")
    })?;

    if changed == 0 {
        anyhow::bail!("Task '{task_id}' not found");
    }

    log::debug!("[projects] delete_task id={task_id}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Attachments
// ---------------------------------------------------------------------------

fn row_to_attachment(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskAttachment> {
    let created_raw: String = row.get(7)?;
    Ok(TaskAttachment {
        id: row.get(0)?,
        task_id: row.get(1)?,
        filename: row.get(2)?,
        mime_type: row.get(3)?,
        rel_path: row.get(4)?,
        size_bytes: row.get(5)?,
        uploaded_by: row.get(6)?,
        created: parse_rfc3339(&created_raw).map_err(sql_err)?,
    })
}

/// Copy `src_path` into the workspace attachments dir and register it in the DB.
/// Returns the new attachment record.
pub fn add_attachment(
    config: &Config,
    task_id: &str,
    src_path: &std::path::Path,
    uploaded_by: &str,
) -> Result<TaskAttachment> {
    // Validate: source must exist and be a file.
    anyhow::ensure!(
        src_path.exists(),
        "source file not found: {}",
        src_path.display()
    );
    anyhow::ensure!(
        src_path.is_file(),
        "source path is not a file: {}",
        src_path.display()
    );

    // Validate: source must NOT be inside the workspace dir (prevent loops).
    let canonical_src = src_path
        .canonicalize()
        .with_context(|| format!("cannot canonicalize {}", src_path.display()))?;
    let canonical_ws = config
        .workspace_dir
        .canonicalize()
        .unwrap_or_else(|_| config.workspace_dir.clone());
    anyhow::ensure!(
        !canonical_src.starts_with(&canonical_ws),
        "cannot attach files from inside the workspace directory"
    );

    let filename = src_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid filename"))?
        .to_string();

    let id = Uuid::new_v4().to_string();
    let now = Utc::now();

    // Destination: workspace/projects/attachments/{task_id}/{id}-{filename}
    let dest_dir = config
        .workspace_dir
        .join("projects")
        .join("attachments")
        .join(task_id);
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("failed to create attachments dir: {}", dest_dir.display()))?;

    let dest_filename = format!("{}-{}", id, filename);
    let dest_path = dest_dir.join(&dest_filename);
    std::fs::copy(&canonical_src, &dest_path)
        .with_context(|| format!("failed to copy attachment to {}", dest_path.display()))?;

    let size_bytes = dest_path.metadata().map(|m| m.len() as i64).unwrap_or(0);

    // rel_path relative to workspace_dir
    let rel_path = format!("projects/attachments/{}/{}", task_id, dest_filename);

    let mime_type = mime_guess::from_path(&filename)
        .first_or_octet_stream()
        .to_string();

    let att = with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO project_task_attachments
             (id, task_id, filename, mime_type, rel_path, size_bytes, uploaded_by, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                task_id,
                filename,
                mime_type,
                rel_path,
                size_bytes,
                uploaded_by,
                now.to_rfc3339()
            ],
        )
        .context("Failed to insert attachment")?;

        let att = conn.query_row(
            "SELECT id, task_id, filename, mime_type, rel_path, size_bytes, uploaded_by, created
             FROM project_task_attachments WHERE id = ?1",
            params![id],
            row_to_attachment,
        )?;
        Ok(att)
    })?;

    // Log an "attached" change event outside the connection closure.
    let _ = log_change(
        config,
        task_id,
        uploaded_by,
        "attachment",
        None,
        Some(&att.filename),
    );

    Ok(att)
}

pub fn list_attachments(config: &Config, task_id: &str) -> Result<Vec<TaskAttachment>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, task_id, filename, mime_type, rel_path, size_bytes, uploaded_by, created
             FROM project_task_attachments
             WHERE task_id = ?1
             ORDER BY created ASC",
        )?;
        let rows = stmt.query_map(params![task_id], row_to_attachment)?;
        let mut atts = Vec::new();
        for row in rows {
            atts.push(row?);
        }
        Ok(atts)
    })
}

pub fn delete_attachment(config: &Config, attachment_id: &str) -> Result<()> {
    // Fetch task_id, filename, rel_path before deleting the DB row.
    let row: Option<(String, String, String)> = with_connection(config, |conn| {
        conn.query_row(
            "SELECT task_id, filename, rel_path FROM project_task_attachments WHERE id = ?1",
            params![attachment_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .context("Failed to query attachment")
    })?;

    let Some((task_id, filename, rel)) = row else {
        anyhow::bail!("Attachment '{}' not found", attachment_id);
    };

    // Delete from DB first.
    with_connection(config, |conn| {
        conn.execute(
            "DELETE FROM project_task_attachments WHERE id = ?1",
            params![attachment_id],
        )
        .context("Failed to delete attachment row")?;
        Ok(())
    })?;

    // Best-effort file deletion — don't fail the RPC if the file is already gone.
    let file_path = config.workspace_dir.join(&rel);
    if file_path.exists() {
        let _ = std::fs::remove_file(&file_path);
    }

    // Log the removal to the change feed.
    let _ = log_change(
        config,
        &task_id,
        "me",
        "attachment_removed",
        Some(&filename),
        None,
    );

    log::debug!("[projects] delete_attachment id={attachment_id} file={filename}");
    Ok(())
}

/// Return the absolute path of an attachment (for AI reading).
pub fn attachment_abs_path(
    config: &Config,
    attachment_id: &str,
) -> Result<(TaskAttachment, std::path::PathBuf)> {
    with_connection(config, |conn| {
        let att = conn.query_row(
            "SELECT id, task_id, filename, mime_type, rel_path, size_bytes, uploaded_by, created
             FROM project_task_attachments WHERE id = ?1",
            params![attachment_id],
            row_to_attachment,
        )
        .with_context(|| format!("Attachment '{}' not found", attachment_id))?;
        let abs = config.workspace_dir.join(&att.rel_path);
        Ok((att, abs))
    })
}

// ---------------------------------------------------------------------------
// Migration helpers
// ---------------------------------------------------------------------------

fn add_column_if_missing(conn: &Connection, table: &str, name: &str, sql_type: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let col_name: String = row.get(1)?;
        if col_name == name {
            return Ok(());
        }
    }
    drop(rows);
    drop(stmt);
    match conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {name} {sql_type}"),
        [],
    ) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(err, Some(ref msg)))
            if msg.contains("duplicate column name") =>
        {
            log::debug!("Column {table}.{name} already exists (concurrent migration): {err}");
            Ok(())
        }
        Err(e) => Err(e).with_context(|| format!("Failed to add {table}.{name}")),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> Config {
        let config = Config {
            workspace_dir: tmp.path().join("workspace"),
            action_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Config::default()
        };
        std::fs::create_dir_all(&config.workspace_dir).unwrap();
        config
    }

    #[test]
    fn ensure_default_project_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let id1 = ensure_default_project(&config).unwrap();
        let id2 = ensure_default_project(&config).unwrap();
        assert_eq!(id1, id2, "second call must return the same project id");
    }

    #[test]
    fn default_project_has_four_buckets() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let project_id = ensure_default_project(&config).unwrap();
        let buckets = list_buckets(&config, &project_id).unwrap();

        assert_eq!(buckets.len(), 4, "expected 4 default buckets");

        let done_count = buckets.iter().filter(|b| b.is_done_bucket).count();
        assert_eq!(done_count, 1, "exactly one bucket should be a done bucket");
    }

    #[test]
    fn create_and_list_task() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let project_id = ensure_default_project(&config).unwrap();
        let buckets = list_buckets(&config, &project_id).unwrap();
        let todo_bucket = buckets.iter().find(|b| b.title == "To Do").unwrap();

        let task = create_task(
            &config,
            &project_id,
            &todo_bucket.id,
            "Write the store",
            Some("All CRUD operations"),
            0,
            None,
            "me",
        )
        .unwrap();

        assert_eq!(task.title, "Write the store");
        assert!(!task.done, "newly created task must not be done");

        let listed = list_tasks(&config, &project_id, None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, task.id);
    }

    #[test]
    fn move_task_to_done_bucket_marks_done() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let project_id = ensure_default_project(&config).unwrap();
        let buckets = list_buckets(&config, &project_id).unwrap();
        let todo_bucket = buckets.iter().find(|b| b.title == "To Do").unwrap();
        let done_bucket = buckets.iter().find(|b| b.is_done_bucket).unwrap();

        let task = create_task(
            &config,
            &project_id,
            &todo_bucket.id,
            "Task to complete",
            None,
            0,
            None,
            "me",
        )
        .unwrap();
        assert!(!task.done);

        let patch = TaskPatch {
            bucket_id: Some(done_bucket.id.clone()),
            ..TaskPatch::default()
        };
        let updated = update_task(&config, &task.id, &patch, "me").unwrap();

        assert!(
            updated.done,
            "task moved to done bucket must be marked done"
        );
        assert!(
            updated.done_at.is_some(),
            "done_at must be set when task is completed"
        );
    }

    #[test]
    fn assignee_defaults_to_none_on_create() {
        let tmp = TempDir::new().unwrap();
        let cfg = test_config(&tmp);
        let project_id = ensure_default_project(&cfg).unwrap();
        let buckets = list_buckets(&cfg, &project_id).unwrap();
        let task = create_task(
            &cfg,
            &project_id,
            &buckets[0].id,
            "Test",
            None,
            0,
            None,
            "me",
        )
        .unwrap();
        assert_eq!(task.assignee, None);
        assert_eq!(task.ai_plan, None);
    }

    #[test]
    fn update_task_sets_and_clears_assignee() {
        let tmp = TempDir::new().unwrap();
        let cfg = test_config(&tmp);
        let project_id = ensure_default_project(&cfg).unwrap();
        let buckets = list_buckets(&cfg, &project_id).unwrap();
        let task = create_task(
            &cfg,
            &project_id,
            &buckets[0].id,
            "Test",
            None,
            0,
            None,
            "me",
        )
        .unwrap();

        // Set to 'me'
        let patched = update_task(
            &cfg,
            &task.id,
            &TaskPatch {
                assignee: Some(Some("me".to_string())),
                ..TaskPatch::default()
            },
            "me",
        )
        .unwrap();
        assert_eq!(patched.assignee, Some("me".to_string()));

        // Change to 'ai'
        let patched2 = update_task(
            &cfg,
            &task.id,
            &TaskPatch {
                assignee: Some(Some("ai".to_string())),
                ..TaskPatch::default()
            },
            "me",
        )
        .unwrap();
        assert_eq!(patched2.assignee, Some("ai".to_string()));

        // Clear it
        let cleared = update_task(
            &cfg,
            &task.id,
            &TaskPatch {
                assignee: Some(None),
                ..TaskPatch::default()
            },
            "me",
        )
        .unwrap();
        assert_eq!(cleared.assignee, None);
    }
}
