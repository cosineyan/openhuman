use crate::openhuman::config::Config;
use crate::openhuman::projects::{
    Bucket, BucketPatch, Project, ProjectTaskRun, Task, TaskAttachment, TaskEvent, TaskEventKind,
    TaskPatch,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::sync::OnceLock;
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
CREATE TABLE IF NOT EXISTS project_task_runs (
    run_id         TEXT PRIMARY KEY,
    task_id        TEXT NOT NULL,
    task_title     TEXT NOT NULL,
    model          TEXT,
    profile_id     TEXT,
    tier           TEXT,
    fallback_steps INTEGER NOT NULL DEFAULT 1,
    fallback_used  INTEGER NOT NULL DEFAULT 0,
    started_at     TEXT NOT NULL,
    finished_at    TEXT,
    duration_ms    INTEGER NOT NULL DEFAULT 0,
    status         TEXT NOT NULL,
    created        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_task_runs_task ON project_task_runs(task_id, started_at);
CREATE INDEX IF NOT EXISTS idx_task_runs_started ON project_task_runs(started_at);
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
    add_column_if_missing(&conn, "project_tasks", "parent_task_id", "TEXT")?;
    add_column_if_missing(
        &conn,
        "project_tasks",
        "archived",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(&conn, "project_tasks", "archived_at", "TEXT")?;
    add_column_if_missing(&conn, "project_tasks", "settings_profile", "TEXT")?;
    add_column_if_missing(&conn, "project_tasks", "model", "TEXT")?;
    add_column_if_missing(&conn, "project_tasks", "fallback_direction", "TEXT")?;
    add_column_if_missing(&conn, "project_tasks", "fallback_end", "TEXT")?;

    // Fix tasks that are marked done=1 but live in a non-done bucket — can
    // happen when a task was moved back from a done bucket via a code path that
    // predated the auto-clear logic in update_task.
    conn.execute(
        "UPDATE project_tasks
         SET done = 0, done_at = NULL, updated = datetime('now')
         WHERE done = 1
           AND bucket_id IN (
               SELECT id FROM project_buckets WHERE is_done_bucket = 0
           )",
        [],
    )
    .context("Failed to fix stale done flags")?;

    // Run once per process lifetime — guard with OnceLock so it doesn't fire
    // on every DB call and incorrectly move actively-running AI tasks to Blocked.
    static STARTUP_CLEANUP_DONE: OnceLock<()> = OnceLock::new();
    if STARTUP_CLEANUP_DONE.get().is_none() {
        cleanup_stale_ai_doing_tasks(&conn).context("Failed to clean up stale AI doing tasks")?;
        let _ = STARTUP_CLEANUP_DONE.set(());
    }

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
    let archived_at_raw: Option<String> = row.get(18)?;
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
        parent_task_id: row.get(16)?,
        created: parse_rfc3339(&created_raw).map_err(sql_err)?,
        updated: parse_rfc3339(&updated_raw).map_err(sql_err)?,
        archived: row.get::<_, i64>(17).unwrap_or(0) != 0,
        archived_at: archived_at_raw
            .as_deref()
            .map(parse_rfc3339)
            .transpose()
            .map_err(sql_err)?,
        settings_profile: row.get(19).unwrap_or(None),
        model: row.get(20).unwrap_or(None),
        fallback_direction: row.get(21).unwrap_or(None),
        fallback_end: row.get(22).unwrap_or(None),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_rfc3339(raw: &str) -> Result<DateTime<Utc>> {
    // Primary: RFC3339 (e.g. "2026-06-21T12:17:57Z") — written by current code.
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Ok(parsed.with_timezone(&Utc));
    }
    // Fallback: SQLite CURRENT_TIMESTAMP format "YYYY-MM-DD HH:MM:SS" — treat as UTC.
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Ok(naive.and_utc());
    }
    anyhow::bail!("Invalid RFC3339 timestamp in projects DB: {raw}")
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

/// List all project ids. Used by the throttle scheduler to scan every board.
pub fn list_project_ids(config: &Config) -> Result<Vec<String>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare("SELECT id FROM projects")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
    })
}

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

pub fn get_task(config: &Config, task_id: &str) -> Result<Task> {
    with_connection(config, |conn| {
        conn.query_row(
            "SELECT id, project_id, bucket_id, title, description,
                    done, done_at, priority, due_date, hex_color,
                    position, idx, created, updated,
                    assignee, ai_plan, parent_task_id,
                    archived, archived_at, settings_profile, model, fallback_direction, fallback_end
             FROM project_tasks WHERE id = ?1",
            params![task_id],
            row_to_task,
        )
        .with_context(|| format!("Task '{task_id}' not found"))
    })
}

pub fn list_tasks(config: &Config, project_id: &str, bucket_id: Option<&str>) -> Result<Vec<Task>> {
    with_connection(config, |conn| {
        let mut tasks = Vec::new();
        if let Some(bid) = bucket_id {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, bucket_id, title, description,
                        done, done_at, priority, due_date, hex_color,
                        position, idx, created, updated,
                        assignee, ai_plan, parent_task_id,
                    archived, archived_at, settings_profile, model, fallback_direction, fallback_end
                 FROM project_tasks
                 WHERE project_id = ?1 AND bucket_id = ?2 AND parent_task_id IS NULL
                   AND (archived = 0 OR archived IS NULL)
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
                        assignee, ai_plan, parent_task_id,
                    archived, archived_at, settings_profile, model, fallback_direction, fallback_end
                 FROM project_tasks
                 WHERE project_id = ?1 AND parent_task_id IS NULL
                   AND (archived = 0 OR archived IS NULL)
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

/// List archived tasks for a project, optionally filtered by search text and created date range.
pub fn list_archived_tasks(
    config: &Config,
    project_id: &str,
    search: Option<&str>,
    created_after: Option<DateTime<Utc>>,
    created_before: Option<DateTime<Utc>>,
) -> Result<Vec<Task>> {
    with_connection(config, |conn| {
        let search_pat = search.map(|s| format!("%{}%", s.to_lowercase()));
        let after_str = created_after.map(|d| d.to_rfc3339());
        let before_str = created_before.map(|d| d.to_rfc3339());

        // Build params in order: project_id always first, then optional ones in declaration order
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_id.to_string())];

        let mut sql = "SELECT id, project_id, bucket_id, title, description,
                    done, done_at, priority, due_date, hex_color,
                    position, idx, created, updated,
                    assignee, ai_plan, parent_task_id,
                    archived, archived_at, settings_profile, model, fallback_direction, fallback_end
             FROM project_tasks
             WHERE project_id = ?1 AND archived = 1 AND parent_task_id IS NULL"
            .to_string();

        let mut idx = 2usize;

        if let Some(ref pat) = search_pat {
            sql.push_str(&format!(
                " AND (lower(title) LIKE ?{idx} OR lower(coalesce(description,'')) LIKE ?{idx})"
            ));
            bound.push(Box::new(pat.clone()));
            idx += 1;
        }
        if let Some(ref after) = after_str {
            sql.push_str(&format!(" AND created >= ?{idx}"));
            bound.push(Box::new(after.clone()));
            idx += 1;
        }
        if let Some(ref before) = before_str {
            sql.push_str(&format!(" AND created <= ?{idx}"));
            bound.push(Box::new(before.clone()));
        }

        sql.push_str(" ORDER BY archived_at DESC");

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(bound.iter().map(|b| b.as_ref())),
            row_to_task,
        )?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row?);
        }
        Ok(tasks)
    })
}

/// Mark tasks as archived if they haven't been updated for more than `stale_days` days.
/// Returns the number of tasks archived.
pub fn auto_archive_stale_tasks(
    config: &Config,
    project_id: &str,
    stale_days: u32,
) -> Result<usize> {
    with_connection(config, |conn| {
        let threshold = (Utc::now() - chrono::Duration::days(stale_days as i64)).to_rfc3339();
        let now_str = Utc::now().to_rfc3339();
        let count = conn.execute(
            "UPDATE project_tasks
             SET archived = 1, archived_at = ?1, updated = ?1
             WHERE project_id = ?2
               AND (archived = 0 OR archived IS NULL)
               AND done = 1
               AND updated < ?3
               AND parent_task_id IS NULL",
            params![now_str, project_id, threshold],
        )?;
        Ok(count)
    })
}

pub fn list_subtasks(config: &Config, parent_task_id: &str) -> Result<Vec<Task>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, bucket_id, title, description,
                    done, done_at, priority, due_date, hex_color,
                    position, idx, created, updated,
                    assignee, ai_plan, parent_task_id,
                    archived, archived_at, settings_profile, model, fallback_direction, fallback_end
             FROM project_tasks
             WHERE parent_task_id = ?1
             ORDER BY created ASC",
        )?;
        let rows = stmt.query_map(params![parent_task_id], row_to_task)?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row?);
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
    parent_task_id: Option<&str>,
    settings_profile: Option<&str>,
    model: Option<&str>,
    fallback_direction: Option<&str>,
    fallback_end: Option<&str>,
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
              position, idx, created, updated, parent_task_id,
              settings_profile, model, fallback_direction, fallback_end)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, NULL, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                parent_task_id,
                settings_profile,
                model,
                fallback_direction,
                fallback_end,
            ],
        )
        .context("Failed to insert task")?;

        let task = conn.query_row(
            "SELECT id, project_id, bucket_id, title, description,
                    done, done_at, priority, due_date, hex_color,
                    position, idx, created, updated,
                    assignee, ai_plan, parent_task_id,
                    archived, archived_at, settings_profile, model, fallback_direction, fallback_end
             FROM project_tasks WHERE id = ?1",
            params![task_id],
            row_to_task,
        )?;
        Ok(task)
    })?;

    // Log a "created" event outside the connection closure.
    let _ = log_change(config, &task.id, actor, "created", None, Some(&task.title));

    // If assigned to AI, signal the AI runner via event bus.
    if task.assignee.as_deref() == Some("ai") {
        crate::core::event_bus::publish_global(
            crate::core::event_bus::DomainEvent::ProjectTaskAssignedToAi {
                task_id: task.id.clone(),
                project_id: task.project_id.clone(),
                bucket_id: task.bucket_id.clone(),
                title: task.title.clone(),
                description: task.description.clone(),
            },
        );
    }

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
                        assignee, ai_plan, parent_task_id,
                    archived, archived_at, settings_profile, model, fallback_direction, fallback_end
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
        // When a task moves to a different bucket without an explicit position,
        // place it at the top (min position - 1) so it's immediately visible.
        let new_position = if let Some(p) = patch.position {
            p
        } else if new_bucket_id != task.bucket_id {
            let min_pos: f64 = conn
                .query_row(
                    "SELECT COALESCE(MIN(position), 0.0) FROM project_tasks WHERE bucket_id = ?1",
                    params![new_bucket_id],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);
            min_pos - 1.0
        } else {
            task.position
        };

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

        // Resolve ai_plan: explicit patch wins, else keep existing value.
        let new_ai_plan: Option<&str> = match &patch.ai_plan {
            Some(s) => Some(s.as_str()),
            None => task.ai_plan.as_deref(),
        };

        // Resolve archived: explicit patch wins, else keep existing value.
        let new_archived = patch.archived.unwrap_or(task.archived);
        let new_archived_at: Option<DateTime<Utc>> = if new_archived && !task.archived {
            Some(now) // just archived
        } else if !new_archived {
            None // un-archived
        } else {
            task.archived_at
        };
        let archived_at_str = new_archived_at.map(|d| d.to_rfc3339());

        // Resolve settings_profile/model (double-option: set / clear / keep).
        let new_settings_profile: Option<String> = match &patch.settings_profile {
            Some(Some(v)) => Some(v.clone()),
            Some(None) => None,
            None => task.settings_profile.clone(),
        };
        let new_model: Option<String> = match &patch.model {
            Some(Some(v)) => Some(v.clone()),
            Some(None) => None,
            None => task.model.clone(),
        };
        let new_fallback_direction: Option<String> = match &patch.fallback_direction {
            Some(Some(v)) => Some(v.clone()),
            Some(None) => None,
            None => task.fallback_direction.clone(),
        };
        let new_fallback_end: Option<String> = match &patch.fallback_end {
            Some(Some(v)) => Some(v.clone()),
            Some(None) => None,
            None => task.fallback_end.clone(),
        };

        conn.execute(
            "UPDATE project_tasks
             SET bucket_id = ?1, title = ?2, description = ?3,
                 done = ?4, done_at = ?5, priority = ?6, due_date = ?7,
                 hex_color = ?8, position = ?9, updated = ?10,
                 ai_plan = ?12, archived = ?13, archived_at = ?14,
                 settings_profile = ?15, model = ?16,
                 fallback_direction = ?17, fallback_end = ?18
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
                new_ai_plan,
                if new_archived { 1_i64 } else { 0 },
                archived_at_str,
                new_settings_profile,
                new_model,
                new_fallback_direction,
                new_fallback_end,
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
                    assignee, ai_plan, parent_task_id,
                    archived, archived_at, settings_profile, model, fallback_direction, fallback_end
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

    // Trigger the AI runner when:
    // (a) assignee just became "ai" (new assignment), OR
    // (b) assignee was already "ai" and bucket just changed (e.g. moved back to To Do).
    // Never fire when actor="ai" — the AI runner's own moves should not retrigger pickup.
    // The bus.rs handler also filters to only act when the target bucket is "To Do".
    if actor != "ai" {
        let ai_newly_assigned =
            updated.assignee.as_deref() == Some("ai") && old_task.assignee.as_deref() != Some("ai");
        let ai_bucket_changed = updated.assignee.as_deref() == Some("ai")
            && old_task.assignee.as_deref() == Some("ai")
            && patch.bucket_id.is_some()
            && updated.bucket_id != old_task.bucket_id;

        if ai_newly_assigned || ai_bucket_changed {
            crate::core::event_bus::publish_global(
                crate::core::event_bus::DomainEvent::ProjectTaskAssignedToAi {
                    task_id: task_id.to_string(),
                    project_id: updated.project_id.clone(),
                    bucket_id: updated.bucket_id.clone(),
                    title: updated.title.clone(),
                    description: updated.description.clone(),
                },
            );
        }
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

/// Return a map of { parent_task_id -> (total, done) } for all tasks in a project.
pub fn count_subtasks_by_parent(
    config: &Config,
    project_id: &str,
) -> Result<HashMap<String, (usize, usize)>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT parent_task_id, COUNT(*), SUM(done) FROM project_tasks
             WHERE project_id = ?1 AND parent_task_id IS NOT NULL
             GROUP BY parent_task_id",
        )?;
        let mut map = HashMap::new();
        let rows = stmt.query_map(rusqlite::params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as usize,
            ))
        })?;
        for row in rows {
            let (parent_id, total, done) = row?;
            map.insert(parent_id, (total, done));
        }
        Ok(map)
    })
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
// Startup cleanup
// ---------------------------------------------------------------------------

/// On process startup, move any tasks that are assigned to AI and sitting in a
/// non-done "Doing"-style bucket to the Blocked bucket. This handles the case
/// where the process exited while an AI run was in flight.
pub fn cleanup_stale_ai_doing_tasks(conn: &Connection) -> Result<()> {
    let project_ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM projects")?;
        let collected = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        collected
    };

    for project_id in project_ids {
        let blocked_id: Option<String> = conn
            .query_row(
                "SELECT id FROM project_buckets \
                 WHERE project_id = ?1 AND LOWER(title) LIKE '%block%' AND is_done_bucket = 0 \
                 LIMIT 1",
                params![project_id],
                |row| row.get(0),
            )
            .optional()?;

        let Some(blocked_id) = blocked_id else {
            continue;
        };

        let stale_task_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT t.id FROM project_tasks t \
                 JOIN project_buckets b ON b.id = t.bucket_id \
                 WHERE t.project_id = ?1 \
                   AND t.assignee = 'ai' \
                   AND b.is_done_bucket = 0 \
                   AND (LOWER(b.title) LIKE '%doing%' OR LOWER(b.title) LIKE '%in progress%') \
                   AND t.parent_task_id IS NULL -- subtasks share their parent's bucket; skip",
            )?;
            let collected = stmt
                .query_map(params![project_id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            collected
        };

        // Compute the batch timestamp once so all events in this pass share it.
        let now_str = Utc::now().to_rfc3339();

        for task_id in stale_task_ids {
            // Read the current bucket_id before moving, so we can record a change event.
            let old_bucket_id: String = conn.query_row(
                "SELECT bucket_id FROM project_tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )?;

            conn.execute(
                "UPDATE project_tasks \
                 SET bucket_id = ?1, updated = ?2 \
                 WHERE id = ?3",
                params![blocked_id, now_str, task_id],
            )?;

            // Record a change event for the bucket_id field (mirrors update_task behaviour).
            conn.execute(
                "INSERT INTO project_task_events \
                 (id, task_id, kind, actor, field, old_value, new_value, body, created) \
                 VALUES (lower(hex(randomblob(16))), ?1, 'change', 'system', 'bucket_id', ?2, ?3, NULL, ?4)",
                params![task_id, old_bucket_id, blocked_id, now_str],
            )?;

            // Record a human-readable comment explaining why the task was moved.
            conn.execute(
                "INSERT INTO project_task_events \
                 (id, task_id, kind, actor, field, old_value, new_value, body, created) \
                 VALUES (lower(hex(randomblob(16))), ?1, 'comment', 'system', NULL, NULL, NULL, \
                 'Moved to Blocked after unexpected app restart — move back to To Do to retry.', \
                 ?2)",
                params![task_id, now_str],
            )?;
            log::info!("[projects] startup cleanup: moved stale AI task={task_id} to Blocked");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// AI run history (project_task_runs)
// ---------------------------------------------------------------------------

fn row_to_task_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectTaskRun> {
    let started_raw: String = row.get(8)?;
    let finished_raw: Option<String> = row.get(9)?;
    let started_at = parse_rfc3339(&started_raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, e.into())
    })?;
    let finished_at = match finished_raw {
        Some(raw) => Some(parse_rfc3339(&raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, e.into())
        })?),
        None => None,
    };
    Ok(ProjectTaskRun {
        run_id: row.get(0)?,
        task_id: row.get(1)?,
        task_title: row.get(2)?,
        model: row.get(3)?,
        profile_id: row.get(4)?,
        tier: row.get(5)?,
        fallback_steps: row.get(6)?,
        fallback_used: row.get(7)?,
        started_at,
        finished_at,
        duration_ms: row.get(10)?,
        status: row.get(11)?,
    })
}

/// Insert a `running` placeholder row at the start of an AI run. `finished_at`
/// is left NULL and `duration_ms` 0 until `finish_task_run` is called.
pub fn insert_running_run(config: &Config, run: &ProjectTaskRun) -> Result<()> {
    with_connection(config, |conn| {
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO project_task_runs \
             (run_id, task_id, task_title, model, profile_id, tier, \
              fallback_steps, fallback_used, started_at, finished_at, duration_ms, status, created) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, 0, 'running', ?10)",
            params![
                run.run_id,
                run.task_id,
                run.task_title,
                run.model,
                run.profile_id,
                run.tier,
                run.fallback_steps,
                run.fallback_used,
                run.started_at.to_rfc3339(),
                now,
            ],
        )
        .context("Failed to insert running task run")?;
        log::debug!(
            "[projects] insert_running_run run_id={} task={}",
            run.run_id,
            run.task_id
        );
        Ok(())
    })
}

/// Update a run row to a terminal status. `model`/`fallback_used` reflect the
/// attempt that actually ran (fallback winner). No-op if the row is missing.
pub fn finish_task_run(
    config: &Config,
    run_id: &str,
    status: &str,
    finished_at: DateTime<Utc>,
    duration_ms: i64,
    model: Option<&str>,
    fallback_used: i64,
) -> Result<()> {
    with_connection(config, |conn| {
        let affected = conn
            .execute(
                "UPDATE project_task_runs \
                 SET status = ?1, finished_at = ?2, duration_ms = ?3, \
                     model = COALESCE(?4, model), fallback_used = ?5 \
                 WHERE run_id = ?6",
                params![
                    status,
                    finished_at.to_rfc3339(),
                    duration_ms,
                    model,
                    fallback_used,
                    run_id,
                ],
            )
            .context("Failed to finish task run")?;
        log::debug!(
            "[projects] finish_task_run run_id={run_id} status={status} duration_ms={duration_ms} affected={affected}"
        );
        Ok(())
    })
}

/// List AI runs whose `started_at` falls in `[since, until]` (either bound
/// optional), newest first, capped at `limit`. Uses the dynamic-SQL pattern
/// from `list_archived_tasks`.
pub fn list_task_runs(
    config: &Config,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<ProjectTaskRun>> {
    with_connection(config, |conn| {
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        let mut sql = "SELECT run_id, task_id, task_title, model, profile_id, tier, \
                    fallback_steps, fallback_used, started_at, finished_at, duration_ms, status \
             FROM project_task_runs \
             WHERE 1 = 1"
            .to_string();

        let mut idx = 1usize;
        if let Some(since) = since {
            sql.push_str(&format!(" AND started_at >= ?{idx}"));
            bound.push(Box::new(since.to_rfc3339()));
            idx += 1;
        }
        if let Some(until) = until {
            sql.push_str(&format!(" AND started_at <= ?{idx}"));
            bound.push(Box::new(until.to_rfc3339()));
            idx += 1;
        }
        sql.push_str(&format!(" ORDER BY started_at DESC LIMIT ?{idx}"));
        bound.push(Box::new(limit.max(0)));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(bound.iter().map(|b| b.as_ref())),
            row_to_task_run,
        )?;
        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    })
}

/// List AI runs for a single task, newest first, capped at `limit`.
pub fn list_runs_for_task(
    config: &Config,
    task_id: &str,
    limit: i64,
) -> Result<Vec<ProjectTaskRun>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT run_id, task_id, task_title, model, profile_id, tier, \
                    fallback_steps, fallback_used, started_at, finished_at, duration_ms, status \
             FROM project_task_runs \
             WHERE task_id = ?1 \
             ORDER BY started_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![task_id, limit.max(0)], row_to_task_run)?;
        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    })
}

/// On startup, mark any run still in `running` (the process exited mid-run) as
/// `interrupted` so history reflects reality. Mirrors `cleanup_stale_ai_doing_tasks`.
pub fn cleanup_stale_running_task_runs(config: &Config) -> Result<()> {
    with_connection(config, |conn| {
        let now = Utc::now().to_rfc3339();
        let affected = conn
            .execute(
                "UPDATE project_task_runs \
                 SET status = 'interrupted', finished_at = COALESCE(finished_at, ?1) \
                 WHERE status = 'running'",
                params![now],
            )
            .context("Failed to clean up stale running task runs")?;
        if affected > 0 {
            log::info!(
                "[projects] startup cleanup: marked {affected} stale running run(s) as interrupted"
            );
        }
        Ok(())
    })
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
            None,
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
            None,
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
            None,
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
            None,
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

    #[test]
    fn startup_moves_ai_doing_tasks_to_blocked() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let project_id = ensure_default_project(&config).unwrap();
        let buckets = list_buckets(&config, &project_id).unwrap();
        let todo_bucket = buckets.iter().find(|b| b.title == "To Do").unwrap();
        let doing_bucket = buckets.iter().find(|b| b.title == "Doing").unwrap();
        let blocked_bucket = buckets.iter().find(|b| b.title == "Blocked").unwrap();

        let task = create_task(
            &config,
            &project_id,
            &todo_bucket.id,
            "AI task",
            None,
            0,
            None,
            "me",
            None,
        )
        .unwrap();
        let patch = TaskPatch {
            bucket_id: Some(doing_bucket.id.clone()),
            assignee: Some(Some("ai".to_string())),
            ..TaskPatch::default()
        };
        update_task(&config, &task.id, &patch, "me").unwrap();

        // Run the cleanup directly.
        with_connection(&config, |conn| cleanup_stale_ai_doing_tasks(conn)).unwrap();

        // Task should now be in Blocked.
        let tasks = list_tasks(&config, &project_id, Some(&blocked_bucket.id)).unwrap();
        assert!(
            tasks.iter().any(|t| t.id == task.id),
            "task should be in Blocked"
        );

        // A system comment event should have been recorded.
        let events = list_events(&config, &task.id).unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.kind == TaskEventKind::Comment && e.actor == "system"),
            "should have a system comment event"
        );
        // A change event for bucket_id should also have been recorded.
        assert!(
            events.iter().any(|e| {
                e.kind == TaskEventKind::Change
                    && e.actor == "system"
                    && e.field.as_deref() == Some("bucket_id")
                    && e.new_value.as_deref() == Some(&blocked_bucket.id)
            }),
            "should have a system change event for bucket_id"
        );
    }

    fn sample_run(run_id: &str, task_id: &str, started_at: DateTime<Utc>) -> ProjectTaskRun {
        ProjectTaskRun {
            run_id: run_id.to_string(),
            task_id: task_id.to_string(),
            task_title: "Do the thing".to_string(),
            model: Some("claude-opus-latest".to_string()),
            profile_id: Some("hyperspace".to_string()),
            tier: Some("opus".to_string()),
            fallback_steps: 2,
            fallback_used: 0,
            started_at,
            finished_at: None,
            duration_ms: 0,
            status: "running".to_string(),
        }
    }

    #[test]
    fn task_run_insert_finish_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        ensure_default_project(&config).unwrap();

        let started = Utc::now();
        let run = sample_run("run-1", "task-1", started);
        insert_running_run(&config, &run).unwrap();

        // Still running: model = start model, no finished_at.
        let runs = list_runs_for_task(&config, "task-1", 10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "running");
        assert!(runs[0].finished_at.is_none());
        assert_eq!(runs[0].fallback_used, 0);

        let finished = started + chrono::Duration::milliseconds(4200);
        finish_task_run(
            &config,
            "run-1",
            "done",
            finished,
            4200,
            Some("claude-sonnet-latest"),
            1,
        )
        .unwrap();

        let runs = list_runs_for_task(&config, "task-1", 10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "done");
        assert_eq!(runs[0].duration_ms, 4200);
        assert_eq!(runs[0].fallback_used, 1);
        // finish_task_run's model overrides the start model (fallback winner).
        assert_eq!(runs[0].model.as_deref(), Some("claude-sonnet-latest"));
        assert!(runs[0].finished_at.is_some());
    }

    #[test]
    fn finish_task_run_preserves_model_when_none() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        ensure_default_project(&config).unwrap();

        let run = sample_run("run-x", "task-x", Utc::now());
        insert_running_run(&config, &run).unwrap();
        // Pass model = None → COALESCE keeps the original start model.
        finish_task_run(&config, "run-x", "error", Utc::now(), 100, None, 0).unwrap();

        let runs = list_runs_for_task(&config, "task-x", 10).unwrap();
        assert_eq!(runs[0].model.as_deref(), Some("claude-opus-latest"));
        assert_eq!(runs[0].status, "error");
    }

    #[test]
    fn list_task_runs_filters_by_started_at_window() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        ensure_default_project(&config).unwrap();

        let now = Utc::now();
        let today = sample_run("r-today", "t1", now);
        let old = sample_run("r-old", "t2", now - chrono::Duration::days(3));
        insert_running_run(&config, &today).unwrap();
        insert_running_run(&config, &old).unwrap();

        // Window = last 24h → only today's run.
        let since = now - chrono::Duration::days(1);
        let recent = list_task_runs(&config, Some(since), None, 200).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].run_id, "r-today");

        // No bounds → both, newest first.
        let all = list_task_runs(&config, None, None, 200).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].run_id, "r-today");
        assert_eq!(all[1].run_id, "r-old");

        // Limit honoured.
        let one = list_task_runs(&config, None, None, 1).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].run_id, "r-today");
    }

    #[test]
    fn cleanup_stale_running_marks_interrupted() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        ensure_default_project(&config).unwrap();

        let started = Utc::now();
        insert_running_run(&config, &sample_run("r-live", "t1", started)).unwrap();
        insert_running_run(&config, &sample_run("r-done", "t2", started)).unwrap();
        finish_task_run(&config, "r-done", "done", started, 50, None, 0).unwrap();

        cleanup_stale_running_task_runs(&config).unwrap();

        let runs = list_task_runs(&config, None, None, 200).unwrap();
        let live = runs.iter().find(|r| r.run_id == "r-live").unwrap();
        let done = runs.iter().find(|r| r.run_id == "r-done").unwrap();
        assert_eq!(live.status, "interrupted");
        assert!(live.finished_at.is_some());
        // Already-terminal rows are untouched.
        assert_eq!(done.status, "done");
    }
}
