# Project Task AI Progress Visibility & Cancellation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a live spinner + streaming log lines on task cards while AI is executing a project task, and allow hard-cancellation that moves the task to Blocked.

**Architecture:** A process-global `RunRegistry` (Rust) maps `task_id → AbortHandle`; the AI runner registers on spawn, publishes `project:task_log` socket events at key milestones, and handles abort by writing a "Cancelled by user" comment and moving to Blocked. A startup cleanup in `with_connection` moves any AI-assigned Doing tasks to Blocked when the process restarts. On the frontend, a `useAiTaskRuns` hook subscribes to `project:task_log` events; KanbanCard shows a spinner + last-line summary; TaskDetailDrawer shows the full scrollable log + a Stop button.

**Tech Stack:** Rust (tokio, rusqlite, socketioxide), React (hooks, socketService), TypeScript

---

## File Map

| File | Action | Purpose |
|------|--------|---------|
| `src/openhuman/projects/run_registry.rs` | Create | Global `AbortHandle` registry |
| `src/openhuman/projects/bus.rs` | Modify | Register run, emit log events, detect cancellation |
| `src/openhuman/projects/ops.rs` | Modify | `cancel_ai_task` + `list_running_ai_tasks` ops |
| `src/openhuman/projects/schemas.rs` | Modify | Two new RPC schemas + handlers |
| `src/openhuman/projects/mod.rs` | Modify | Re-export `run_registry` module |
| `src/openhuman/projects/store.rs` | Modify | Startup cleanup: AI Doing → Blocked |
| `app/src/components/projects/useAiTaskRuns.ts` | Create | Socket hook + running state |
| `app/src/components/projects/KanbanCard.tsx` | Modify | Spinner + last-line summary |
| `app/src/components/projects/TaskDetailDrawer.tsx` | Modify | Log panel + Stop button |
| `app/src/services/api/projectsApi.ts` | Modify | `cancelAiTask` + `listRunningAiTasks` |

---

## Task 1: RunRegistry — global AbortHandle store

**Files:**
- Create: `src/openhuman/projects/run_registry.rs`
- Modify: `src/openhuman/projects/mod.rs`

- [ ] **Step 1: Write the failing Rust test**

Add an inline test module at the bottom of the new file. Create the file with just the test first:

```rust
// src/openhuman/projects/run_registry.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::task;

    #[tokio::test]
    async fn register_and_cancel_removes_handle() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let handle = task::spawn(async move {
            let _ = rx.await;
        });
        let abort = handle.abort_handle();
        register("task-1", abort);
        assert!(is_running("task-1"));
        let found = cancel("task-1");
        assert!(found);
        assert!(!is_running("task-1"));
        let _ = tx.send(());
    }

    #[test]
    fn cancel_unknown_returns_false() {
        assert!(!cancel("nonexistent-task"));
    }

    #[test]
    fn list_running_reflects_state() {
        // Register a dummy handle (we won't actually spawn a task).
        // tokio::task::AbortHandle can be obtained from a spawned handle;
        // for testing create a real one.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let abort = rt.block_on(async {
            let h = tokio::task::spawn(async { tokio::time::sleep(std::time::Duration::from_secs(60)).await });
            let a = h.abort_handle();
            a.abort(); // clean up immediately
            a
        });
        register("task-list-test", abort);
        let running = list_running();
        assert!(running.contains(&"task-list-test".to_string()));
        cancel("task-list-test");
        assert!(!list_running().contains(&"task-list-test".to_string()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /path/to/repo
cargo test --manifest-path Cargo.toml -p openhuman openhuman::projects::run_registry 2>&1 | tail -20
```
Expected: compile error — module `run_registry` not found.

- [ ] **Step 3: Implement `run_registry.rs`**

```rust
// src/openhuman/projects/run_registry.rs
use std::collections::HashMap;
use std::sync::Mutex;

use tokio::task::AbortHandle;

static REGISTRY: Mutex<Option<HashMap<String, AbortHandle>>> = Mutex::new(None);

fn registry() -> std::sync::MutexGuard<'static, Option<HashMap<String, AbortHandle>>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Register an `AbortHandle` for `task_id`. Overwrites any previous entry.
pub fn register(task_id: &str, handle: AbortHandle) {
    let mut guard = registry();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(task_id.to_string(), handle);
    log::debug!("[run_registry] registered task={task_id}");
}

/// Abort the task and remove its entry. Returns `true` if a handle was found.
pub fn cancel(task_id: &str) -> bool {
    let mut guard = registry();
    if let Some(map) = guard.as_mut() {
        if let Some(handle) = map.remove(task_id) {
            handle.abort();
            log::debug!("[run_registry] cancelled task={task_id}");
            return true;
        }
    }
    false
}

/// Remove a finished task's entry without aborting.
pub fn deregister(task_id: &str) {
    let mut guard = registry();
    if let Some(map) = guard.as_mut() {
        map.remove(task_id);
    }
}

/// Return `true` if a handle for `task_id` is currently registered.
pub fn is_running(task_id: &str) -> bool {
    let guard = registry();
    guard
        .as_ref()
        .map(|m| m.contains_key(task_id))
        .unwrap_or(false)
}

/// Return all currently-registered task IDs.
pub fn list_running() -> Vec<String> {
    let guard = registry();
    guard
        .as_ref()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    // (paste the test code from Step 1 here)
    use super::*;
    use tokio::task;

    #[tokio::test]
    async fn register_and_cancel_removes_handle() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let handle = task::spawn(async move {
            let _ = rx.await;
        });
        let abort = handle.abort_handle();
        register("task-1", abort);
        assert!(is_running("task-1"));
        let found = cancel("task-1");
        assert!(found);
        assert!(!is_running("task-1"));
        let _ = tx.send(());
    }

    #[test]
    fn cancel_unknown_returns_false() {
        assert!(!cancel("nonexistent-task"));
    }
}
```

- [ ] **Step 4: Add `pub mod run_registry;` to `mod.rs`**

In `src/openhuman/projects/mod.rs`, add after the existing `pub mod bus;` line:

```rust
pub mod run_registry;
```

- [ ] **Step 5: Run tests**

```bash
GGML_NATIVE=OFF cargo test --manifest-path Cargo.toml -p openhuman openhuman::projects::run_registry 2>&1 | tail -20
```
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/openhuman/projects/run_registry.rs src/openhuman/projects/mod.rs
git commit -m "feat(projects): add RunRegistry for tracking in-flight AI task AbortHandles"
```

---

## Task 2: Startup cleanup — move stale AI-Doing tasks to Blocked on process start

**Files:**
- Modify: `src/openhuman/projects/store.rs`

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `store.rs`:

```rust
#[test]
fn startup_moves_ai_doing_tasks_to_blocked() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    // Create default project (To Do, Doing, Blocked, Done buckets).
    let project_id = ensure_default_project(&config).unwrap();
    let buckets = list_buckets(&config, &project_id).unwrap();
    let todo_bucket = buckets.iter().find(|b| b.title == "To Do").unwrap();
    let doing_bucket = buckets.iter().find(|b| b.title == "Doing").unwrap();
    let blocked_bucket = buckets.iter().find(|b| b.title == "Blocked").unwrap();

    // Create a task assigned to AI and manually move it to Doing.
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
    with_connection(&config, |conn| {
        cleanup_stale_ai_doing_tasks(conn)
    })
    .unwrap();

    // Task should now be in Blocked.
    let tasks = list_tasks(&config, &project_id, Some(&blocked_bucket.id)).unwrap();
    assert!(tasks.iter().any(|t| t.id == task.id), "task should be in Blocked");
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
GGML_NATIVE=OFF cargo test --manifest-path Cargo.toml -p openhuman startup_moves_ai_doing_tasks_to_blocked 2>&1 | tail -20
```
Expected: compile error — `cleanup_stale_ai_doing_tasks` not found.

- [ ] **Step 3: Implement `cleanup_stale_ai_doing_tasks`**

Add this function before the `#[cfg(test)]` block in `store.rs`:

```rust
/// On process startup, move any tasks that are assigned to AI and sitting in a
/// non-done "Doing"-style bucket to the Blocked bucket. This handles the case
/// where the process exited while an AI run was in flight.
pub fn cleanup_stale_ai_doing_tasks(conn: &Connection) -> Result<()> {
    // Find all projects so we can locate per-project Blocked buckets.
    let project_ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM projects")?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };

    for project_id in project_ids {
        // Find the Blocked bucket for this project.
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

        // Find AI-assigned tasks in non-done, doing-style buckets.
        let stale_task_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT t.id FROM project_tasks t \
                 JOIN project_buckets b ON b.id = t.bucket_id \
                 WHERE t.project_id = ?1 \
                   AND t.assignee = 'ai' \
                   AND b.is_done_bucket = 0 \
                   AND (LOWER(b.title) LIKE '%doing%' OR LOWER(b.title) LIKE '%in progress%') \
                   AND t.parent_task_id IS NULL",
            )?;
            stmt.query_map(params![project_id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        for task_id in stale_task_ids {
            conn.execute(
                "UPDATE project_tasks \
                 SET bucket_id = ?1, updated = datetime('now') \
                 WHERE id = ?2",
                params![blocked_id, task_id],
            )?;
            conn.execute(
                "INSERT INTO project_task_events \
                 (id, task_id, kind, actor, field, old_value, new_value, created) \
                 VALUES (lower(hex(randomblob(16))), ?1, 'comment', 'system', NULL, NULL, \
                 'Moved to Blocked after unexpected app restart — move back to To Do to retry.', \
                 datetime('now'))",
                params![task_id],
            )?;
            log::info!(
                "[projects] startup cleanup: moved stale AI task={task_id} to Blocked"
            );
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Call it from `with_connection`**

In `with_connection`, after the existing `add_column_if_missing` lines and the stale-done-flag fix, add:

```rust
    cleanup_stale_ai_doing_tasks(&conn)
        .context("Failed to clean up stale AI doing tasks")?;
```

- [ ] **Step 5: Run test**

```bash
GGML_NATIVE=OFF cargo test --manifest-path Cargo.toml -p openhuman startup_moves_ai_doing_tasks_to_blocked 2>&1 | tail -20
```
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/openhuman/projects/store.rs
git commit -m "feat(projects): move stale AI-Doing tasks to Blocked on process startup"
```

---

## Task 3: Emit `project:task_log` socket events from `bus.rs`

**Files:**
- Modify: `src/openhuman/projects/bus.rs`

The AI runner will publish `project:task_log` events via `publish_web_channel_event` with `client_id = "system"` (broadcasts to all connected clients). The `WebChannelEvent` struct has optional fields — we use `message` for the log line text and a new custom serialised field. Because `WebChannelEvent` is a shared struct we cannot add project-specific fields to it. Instead we encode the task id in `thread_id` (which is already broadcast-safe) and use `event = "project:task_log"`.

- [ ] **Step 1: Add the log-emit helper to `bus.rs`**

Add this private function near the top of the implementation section in `bus.rs` (after the `LOG` constant):

```rust
fn emit_task_log(task_id: &str, line: &str, kind: &str) {
    use crate::openhuman::channels::providers::web::event_bus::publish_web_channel_event;
    use crate::core::socketio::WebChannelEvent;
    use serde_json::json;

    // Pack the project-task-specific fields into `message` (human-readable line)
    // and a JSON `output` field that the frontend can parse structurally.
    let payload = json!({ "task_id": task_id, "line": line, "kind": kind });
    publish_web_channel_event(WebChannelEvent {
        event: "project:task_log".to_string(),
        client_id: "system".to_string(),
        thread_id: format!("project-task-{task_id}"),
        request_id: String::new(),
        message: Some(line.to_string()),
        output: Some(payload.to_string()),
        ..WebChannelEvent::default()
    });
}
```

- [ ] **Step 2: Update `run_ai_task` to register, emit, and deregister**

Replace the `tokio::spawn` call in `handle` and the `run_ai_task` function body. The full updated `run_ai_task` function:

```rust
async fn run_ai_task(
    config: Arc<Config>,
    task_id: String,
    project_id: String,
    title: String,
    description: Option<String>,
    buckets: Vec<crate::openhuman::projects::Bucket>,
) {
    let started_at = Utc::now();
    log::debug!("{LOG} picking up task={task_id} title={title:?}");

    let find_bucket = |fragment: &str| -> Option<String> {
        buckets
            .iter()
            .find(|b| b.title.to_lowercase().contains(fragment))
            .map(|b| b.id.clone())
    };

    // ── 1. Move to Doing ──────────────────────────────────────────────────
    let doing_id = match find_bucket("doing").or_else(|| find_bucket("in progress")) {
        Some(id) => id,
        None => {
            log::warn!("{LOG} task={task_id} no 'Doing' bucket found — aborting");
            return;
        }
    };

    let patch_doing = TaskPatch {
        bucket_id: Some(doing_id.clone()),
        ..TaskPatch::default()
    };
    if let Err(e) = store::update_task(&config, &task_id, &patch_doing, "ai") {
        log::error!("{LOG} task={task_id} failed to move to Doing: {e}");
        return;
    }
    let _ = store::add_comment(&config, &task_id, "ai", "Starting to work on this task…");
    emit_task_log(&task_id, "Starting to work on this task…", "log");

    // ── 2. Build prompt ───────────────────────────────────────────────────
    let prompt = build_prompt(&title, description.as_deref());

    // ── 3. Run AI ─────────────────────────────────────────────────────────
    let outcome = run_agent(&config, &task_id, &prompt).await;
    let finished_at = Utc::now();

    // ── 4. Write back ─────────────────────────────────────────────────────

    // Check if the run was cancelled (abort_handle was triggered).
    let was_cancelled = match &outcome {
        Err(msg) => msg.contains("task was cancelled") || msg.contains("JoinError"),
        Ok(_) => false,
    };

    let (status, response_text) = if was_cancelled {
        let comment = "Cancelled by user.";
        let _ = store::add_comment(&config, &task_id, "ai", comment);
        emit_task_log(&task_id, comment, "cancelled");
        match find_bucket("block") {
            Some(id) => {
                let patch = TaskPatch {
                    bucket_id: Some(id),
                    ..TaskPatch::default()
                };
                if let Err(e) = store::update_task(&config, &task_id, &patch, "ai") {
                    log::error!("{LOG} task={task_id} failed to move to Blocked: {e}");
                }
            }
            None => log::warn!("{LOG} task={task_id} no Blocked bucket for cancelled task"),
        }
        ("cancelled", comment)
    } else {
        match &outcome {
            Ok(response) => {
                let _ = store::add_comment(&config, &task_id, "ai", response);
                emit_task_log(&task_id, response, "done");
                let done_id = buckets
                    .iter()
                    .find(|b| b.is_done_bucket)
                    .map(|b| b.id.clone())
                    .or_else(|| find_bucket("done"));
                if let Some(id) = done_id {
                    let patch = TaskPatch {
                        bucket_id: Some(id),
                        ..TaskPatch::default()
                    };
                    if let Err(e) = store::update_task(&config, &task_id, &patch, "ai") {
                        log::error!("{LOG} task={task_id} failed to move to Done: {e}");
                    }
                }
                ("done", response.as_str())
            }
            Err(err_msg) => {
                log::warn!("{LOG} task={task_id} AI failed: {err_msg}");
                let comment = format!("Encountered an issue:\n\n{err_msg}");
                let _ = store::add_comment(&config, &task_id, "ai", &comment);
                emit_task_log(&task_id, &comment, "error");
                if let Some(id) = find_bucket("block") {
                    let patch = TaskPatch {
                        bucket_id: Some(id),
                        ..TaskPatch::default()
                    };
                    if let Err(e) = store::update_task(&config, &task_id, &patch, "ai") {
                        log::error!("{LOG} task={task_id} failed to move to Blocked: {e}");
                    }
                }
                ("blocked", err_msg.as_str())
            }
        }
    };

    // ── 5. Write and attach AI log ────────────────────────────────────────
    upload_ai_log(
        &config,
        &task_id,
        &title,
        description.as_deref(),
        &prompt,
        status,
        response_text,
        started_at,
        finished_at,
    );

    crate::openhuman::projects::run_registry::deregister(&task_id);
    log::debug!("{LOG} task={task_id} complete (status={status})");
    let _ = project_id;
}
```

- [ ] **Step 3: Register the AbortHandle in `handle()`**

In the `handle` method of `ProjectAiRunner`, replace the `tokio::spawn` call:

```rust
        let join = tokio::spawn(async move {
            run_ai_task(config, task_id.clone(), project_id, title, description, buckets).await;
        });
        crate::openhuman::projects::run_registry::register(&task_id, join.abort_handle());
```

- [ ] **Step 4: Verify it compiles**

```bash
GGML_NATIVE=OFF cargo check --manifest-path Cargo.toml 2>&1 | grep -E "^error" | grep "projects/bus" | head -10
```
Expected: no errors from `projects/bus.rs`.

- [ ] **Step 5: Commit**

```bash
git add src/openhuman/projects/bus.rs
git commit -m "feat(projects): emit project:task_log socket events and register AbortHandle"
```

---

## Task 4: New RPC endpoints — cancel and list running

**Files:**
- Modify: `src/openhuman/projects/ops.rs`
- Modify: `src/openhuman/projects/schemas.rs`

- [ ] **Step 1: Add ops**

At the bottom of `src/openhuman/projects/ops.rs`, before the closing brace, add:

```rust
/// Hard-cancel an in-flight AI task. Aborts the tokio task, which causes
/// `run_ai_task` to detect cancellation and move the task to Blocked.
/// Returns `true` when a running task was found and cancelled.
pub fn cancel_ai_task(task_id: &str) -> RpcOutcome<serde_json::Value> {
    let cancelled = crate::openhuman::projects::run_registry::cancel(task_id);
    log::debug!("[projects] cancel_ai_task task={task_id} found={cancelled}");
    RpcOutcome::single_log(
        serde_json::json!({ "cancelled": cancelled }),
        format!("cancel_ai_task task={task_id} cancelled={cancelled}"),
    )
}

/// List task IDs that currently have a registered AbortHandle (i.e. are
/// actively being processed by the AI runner).
pub fn list_running_ai_tasks() -> RpcOutcome<serde_json::Value> {
    let task_ids = crate::openhuman::projects::run_registry::list_running();
    RpcOutcome::single_log(
        serde_json::json!({ "task_ids": task_ids }),
        format!("list_running_ai_tasks count={}", task_ids.len()),
    )
}
```

- [ ] **Step 2: Add schemas and handlers**

In `schemas.rs`, add to `all_controller_schemas()`:

```rust
        schemas("cancel_ai_task"),
        schemas("list_running_ai_tasks"),
```

Add to `all_registered_controllers()`:

```rust
        RegisteredController {
            schema: schemas("cancel_ai_task"),
            handler: handle_cancel_ai_task,
        },
        RegisteredController {
            schema: schemas("list_running_ai_tasks"),
            handler: handle_list_running_ai_tasks,
        },
```

Add to the `schemas` match arms (before the closing `}` of the match):

```rust
        "cancel_ai_task" => ControllerSchema {
            namespace: "projects",
            function: "cancel_ai_task",
            description: "Hard-cancel an in-flight AI task. Moves task to Blocked with a cancellation comment.",
            inputs: vec![task_id_input("ID of the running AI task to cancel.")],
            outputs: vec![FieldSchema {
                name: "cancelled",
                ty: TypeSchema::Bool,
                comment: "true if the task was found and cancelled; false if it had already finished.",
                required: true,
            }],
        },
        "list_running_ai_tasks" => ControllerSchema {
            namespace: "projects",
            function: "list_running_ai_tasks",
            description: "Return the IDs of all project tasks currently being processed by the AI runner.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "task_ids",
                ty: TypeSchema::Json,
                comment: "Array of task ID strings.",
                required: true,
            }],
        },
```

Add the handlers at the bottom of the file (before the `to_json` helper):

```rust
fn handle_cancel_ai_task(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let task_id = get_str(&params, "task_id")?.to_string();
        tracing::debug!(task_id = %task_id, "[rpc][projects] cancel_ai_task entry");
        to_json(ops::cancel_ai_task(&task_id))
    })
}

fn handle_list_running_ai_tasks(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        tracing::debug!("[rpc][projects] list_running_ai_tasks entry");
        to_json(ops::list_running_ai_tasks())
    })
}
```

- [ ] **Step 3: Verify compilation**

```bash
GGML_NATIVE=OFF cargo check --manifest-path Cargo.toml 2>&1 | grep -E "^error" | grep "projects/" | head -10
```
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/openhuman/projects/ops.rs src/openhuman/projects/schemas.rs
git commit -m "feat(projects): add cancel_ai_task and list_running_ai_tasks RPC endpoints"
```

---

## Task 5: Frontend API helpers

**Files:**
- Modify: `app/src/services/api/projectsApi.ts`

- [ ] **Step 1: Add the two new functions at the bottom of `projectsApi.ts`**

```typescript
/** Hard-cancel a running AI task. Returns true if it was found and stopped. */
export async function cancelAiTask(task_id: string): Promise<{ cancelled: boolean }> {
  log('cancelAiTask task_id=%s', task_id);
  const res = await callCoreRpc<RpcEnvelope<{ cancelled: boolean }>>({
    method: 'openhuman.projects_cancel_ai_task',
    params: { task_id },
  });
  return res.result;
}

/** Return the IDs of all tasks currently being processed by the AI runner. */
export async function listRunningAiTasks(): Promise<{ task_ids: string[] }> {
  log('listRunningAiTasks');
  const res = await callCoreRpc<RpcEnvelope<{ task_ids: string[] }>>({
    method: 'openhuman.projects_list_running_ai_tasks',
    params: {},
  });
  return res.result;
}
```

- [ ] **Step 2: Run typecheck**

```bash
cd /path/to/repo && pnpm typecheck 2>&1 | grep "projectsApi" | head -10
```
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add app/src/services/api/projectsApi.ts
git commit -m "feat(projects): add cancelAiTask and listRunningAiTasks API helpers"
```

---

## Task 6: `useAiTaskRuns` hook

**Files:**
- Create: `app/src/components/projects/useAiTaskRuns.ts`

- [ ] **Step 1: Write failing test**

Create `app/src/components/projects/useAiTaskRuns.test.ts`:

```typescript
import { renderHook, act, waitFor } from '@testing-library/react';
import { vi, describe, it, expect, beforeEach } from 'vitest';

// Mock socketService
vi.mock('../../services/socketService', () => ({
  socketService: {
    on: vi.fn(),
    off: vi.fn(),
  },
}));

// Mock projectsApi
vi.mock('../../services/api/projectsApi', () => ({
  listRunningAiTasks: vi.fn().mockResolvedValue({ task_ids: ['task-existing'] }),
}));

import { socketService } from '../../services/socketService';
import { useAiTaskRuns } from './useAiTaskRuns';

describe('useAiTaskRuns', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('seeds running state from listRunningAiTasks on mount', async () => {
    const { result } = renderHook(() => useAiTaskRuns());
    await waitFor(() => {
      expect(result.current.isRunning('task-existing')).toBe(true);
    });
  });

  it('registers and deregisters socket listener', () => {
    const { unmount } = renderHook(() => useAiTaskRuns());
    expect(socketService.on).toHaveBeenCalledWith('project:task_log', expect.any(Function));
    unmount();
    expect(socketService.off).toHaveBeenCalledWith('project:task_log', expect.any(Function));
  });

  it('adds log line and marks running on log event', async () => {
    const { result } = renderHook(() => useAiTaskRuns());

    // Simulate receiving a log event.
    const listener = (socketService.on as ReturnType<typeof vi.fn>).mock.calls.find(
      ([event]) => event === 'project:task_log'
    )?.[1];

    act(() => {
      listener?.({
        output: JSON.stringify({ task_id: 'task-new', line: 'hello', kind: 'log' }),
      });
    });

    expect(result.current.isRunning('task-new')).toBe(true);
    expect(result.current.getLines('task-new')).toEqual(['hello']);
  });

  it('marks done on terminal event', async () => {
    const { result } = renderHook(() => useAiTaskRuns());

    const listener = (socketService.on as ReturnType<typeof vi.fn>).mock.calls.find(
      ([event]) => event === 'project:task_log'
    )?.[1];

    act(() => {
      listener?.({
        output: JSON.stringify({ task_id: 'task-fin', line: 'Done!', kind: 'done' }),
      });
    });

    expect(result.current.isRunning('task-fin')).toBe(false);
    expect(result.current.getRun('task-fin')?.status).toBe('done');
  });
});
```

- [ ] **Step 2: Run to verify it fails**

```bash
cd /path/to/repo/app && pnpm test src/components/projects/useAiTaskRuns.test.ts 2>&1 | tail -20
```
Expected: error — `useAiTaskRuns` not found.

- [ ] **Step 3: Implement the hook**

Create `app/src/components/projects/useAiTaskRuns.ts`:

```typescript
import { useCallback, useEffect, useRef, useState } from 'react';

import { listRunningAiTasks } from '../../services/api/projectsApi';
import { socketService } from '../../services/socketService';

export type AiTaskRunStatus = 'running' | 'done' | 'cancelled' | 'error';

export interface AiTaskRun {
  taskId: string;
  lines: string[];
  status: AiTaskRunStatus;
}

type RunMap = Map<string, AiTaskRun>;

const TERMINAL_STATUSES: AiTaskRunStatus[] = ['done', 'cancelled', 'error'];
const CLEANUP_DELAY_MS = 30_000;

export function useAiTaskRuns() {
  const [runs, setRuns] = useState<RunMap>(new Map());
  // Keep a stable ref for use inside the socket listener closure.
  const runsRef = useRef<RunMap>(runs);
  runsRef.current = runs;

  useEffect(() => {
    // Seed from backend on mount (handles reconnect / page reload).
    listRunningAiTasks()
      .then(({ task_ids }) => {
        if (task_ids.length === 0) return;
        setRuns(prev => {
          const next = new Map(prev);
          for (const id of task_ids) {
            if (!next.has(id)) {
              next.set(id, { taskId: id, lines: [], status: 'running' });
            }
          }
          return next;
        });
      })
      .catch(() => {
        // Non-fatal: the run indicators just won't show for pre-existing runs.
      });
  }, []);

  useEffect(() => {
    const listener = (data: unknown) => {
      if (!data || typeof data !== 'object') return;
      const raw = (data as Record<string, unknown>).output;
      if (typeof raw !== 'string') return;
      let parsed: { task_id?: string; line?: string; kind?: string };
      try {
        parsed = JSON.parse(raw) as typeof parsed;
      } catch {
        return;
      }
      const { task_id, line, kind } = parsed;
      if (!task_id || !line || !kind) return;

      const status: AiTaskRunStatus =
        kind === 'done' ? 'done'
        : kind === 'cancelled' ? 'cancelled'
        : kind === 'error' ? 'error'
        : 'running';

      setRuns(prev => {
        const next = new Map(prev);
        const existing = next.get(task_id);
        const run: AiTaskRun = {
          taskId: task_id,
          lines: existing ? [...existing.lines, line] : [line],
          status,
        };
        next.set(task_id, run);
        return next;
      });

      // Auto-remove terminal runs after a delay so the final state stays
      // visible briefly for the user to read.
      if (TERMINAL_STATUSES.includes(status)) {
        setTimeout(() => {
          setRuns(prev => {
            const next = new Map(prev);
            next.delete(task_id);
            return next;
          });
        }, CLEANUP_DELAY_MS);
      }
    };

    socketService.on('project:task_log', listener);
    return () => {
      socketService.off('project:task_log', listener);
    };
  }, []);

  const isRunning = useCallback(
    (taskId: string) => runs.get(taskId)?.status === 'running',
    [runs]
  );

  const getLines = useCallback(
    (taskId: string): string[] => runs.get(taskId)?.lines ?? [],
    [runs]
  );

  const getRun = useCallback(
    (taskId: string): AiTaskRun | undefined => runs.get(taskId),
    [runs]
  );

  return { isRunning, getLines, getRun, runs };
}
```

- [ ] **Step 4: Run tests**

```bash
cd /path/to/repo/app && pnpm test src/components/projects/useAiTaskRuns.test.ts 2>&1 | tail -20
```
Expected: all 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add app/src/components/projects/useAiTaskRuns.ts app/src/components/projects/useAiTaskRuns.test.ts
git commit -m "feat(projects): add useAiTaskRuns hook for live AI task log streaming"
```

---

## Task 7: KanbanCard — spinner + last-line summary

**Files:**
- Modify: `app/src/components/projects/KanbanCard.tsx`

- [ ] **Step 1: Read the current KanbanCard file**

Read `app/src/components/projects/KanbanCard.tsx` to confirm current structure before editing.

- [ ] **Step 2: Add `useAiTaskRuns` to the card**

At the top of `KanbanCard.tsx`, add the import:

```typescript
import { useAiTaskRuns } from './useAiTaskRuns';
```

Inside the main `KanbanCard` component (not `SubtaskRow`), add the hook call after existing hooks:

```typescript
const { isRunning, getLines } = useAiTaskRuns();
const aiRunning = task.assignee === 'ai' && isRunning(task.id);
const lastLogLine = aiRunning ? getLines(task.id).at(-1) : undefined;
```

- [ ] **Step 3: Update the AI badge area to show spinner**

Find the existing AI badge render (near `task.assignee === 'ai'`). Replace the badge with a spinner when running:

```tsx
{task.assignee && (
  <span className="text-xs font-medium px-1.5 py-0.5 rounded bg-ocean-100 dark:bg-ocean-900 text-ocean-700 dark:text-ocean-300 flex items-center gap-1">
    {aiRunning && (
      <svg className="animate-spin h-3 w-3" viewBox="0 0 24 24" fill="none">
        <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
        <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z" />
      </svg>
    )}
    {task.assignee === 'ai' ? 'AI' : 'ME'}
  </span>
)}
```

- [ ] **Step 4: Add last-line summary below the title**

Find the `<p>` element that renders `task.title`. Immediately after it, add:

```tsx
{lastLogLine && (
  <p className="text-xs text-stone-400 dark:text-neutral-500 truncate mt-0.5">
    {lastLogLine}
  </p>
)}
```

- [ ] **Step 5: Run typecheck**

```bash
cd /path/to/repo && pnpm typecheck 2>&1 | grep "KanbanCard" | head -10
```
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add app/src/components/projects/KanbanCard.tsx
git commit -m "feat(projects): show AI spinner and last log line on KanbanCard"
```

---

## Task 8: TaskDetailDrawer — log panel + Stop button

**Files:**
- Modify: `app/src/components/projects/TaskDetailDrawer.tsx`

- [ ] **Step 1: Add imports and hook**

At the top of `TaskDetailDrawer.tsx`, add:

```typescript
import { useRef, useEffect } from 'react'; // (may already be partially imported — merge)
import { useAiTaskRuns, type AiTaskRunStatus } from './useAiTaskRuns';
import { cancelAiTask } from '../../services/api/projectsApi';
```

Inside the component, add:

```typescript
const { getRun } = useAiTaskRuns();
const activeRun = getRun(task.id);
const logEndRef = useRef<HTMLDivElement>(null);

// Auto-scroll log panel to bottom on new lines.
useEffect(() => {
  logEndRef.current?.scrollIntoView({ behavior: 'smooth' });
}, [activeRun?.lines.length]);
```

- [ ] **Step 2: Add the log panel JSX**

Find the section in the drawer that renders comments/events (above the comment list). Insert the log panel immediately above it:

```tsx
{activeRun && (
  <div className="mb-4 rounded-lg border border-stone-200 dark:border-neutral-700 overflow-hidden">
    <div className="flex items-center justify-between px-3 py-2 bg-stone-50 dark:bg-neutral-800 border-b border-stone-200 dark:border-neutral-700">
      <span className="text-xs font-medium text-stone-600 dark:text-neutral-300 flex items-center gap-1.5">
        {activeRun.status === 'running' && (
          <svg className="animate-spin h-3 w-3" viewBox="0 0 24 24" fill="none">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z" />
          </svg>
        )}
        {activeRun.status === 'running' ? 'AI is working…' : `AI finished — ${activeRun.status}`}
      </span>
      {activeRun.status === 'running' && (
        <button
          onClick={() => {
            void cancelAiTask(task.id);
          }}
          className="text-xs text-red-600 dark:text-red-400 hover:underline font-medium"
        >
          Stop
        </button>
      )}
    </div>
    <pre className="text-xs font-mono p-3 max-h-48 overflow-y-auto whitespace-pre-wrap break-words bg-white dark:bg-neutral-900 text-stone-700 dark:text-neutral-200">
      {activeRun.lines.join('\n') || '…'}
      <div ref={logEndRef} />
    </pre>
  </div>
)}
```

- [ ] **Step 3: Run typecheck**

```bash
cd /path/to/repo && pnpm typecheck 2>&1 | grep "TaskDetailDrawer" | head -10
```
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add app/src/components/projects/TaskDetailDrawer.tsx
git commit -m "feat(projects): add AI log panel and Stop button to TaskDetailDrawer"
```

---

## Task 9: Full test pass

- [ ] **Step 1: Run all Rust tests (projects domain)**

```bash
GGML_NATIVE=OFF cargo test --manifest-path Cargo.toml -p openhuman openhuman::projects 2>&1 | tail -30
```
Expected: all tests pass.

- [ ] **Step 2: Run all frontend tests**

```bash
cd /path/to/repo/app && pnpm test 2>&1 | tail -20
```
Expected: all tests pass (including the 4 new `useAiTaskRuns` tests).

- [ ] **Step 3: Run typecheck**

```bash
cd /path/to/repo && pnpm typecheck 2>&1 | grep -v "node_modules" | head -20
```
Expected: no errors.

- [ ] **Step 4: Final commit if any loose changes**

```bash
git status
# If clean, nothing to do. If there are stray changes, commit them.
```
