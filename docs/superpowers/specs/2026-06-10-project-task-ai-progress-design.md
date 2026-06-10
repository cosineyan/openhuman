# Design: Project Task AI Progress Visibility & Cancellation

**Date:** 2026-06-10
**Status:** Draft

---

## Problem

When AI picks up a project task, the task card moves to the "Doing" bucket and shows an "AI" badge — but there is no indication that the agent is actively running, no way to see what it's doing, and no way to stop it.

---

## Goals

1. Show a **running indicator** on task cards while AI is executing.
2. Surface **streaming log lines** from the AI run (card: last line summary; drawer: full log).
3. Allow the user to **hard-cancel** a running AI task (immediate abort → Blocked + comment).

---

## Architecture

### Backend: `RunRegistry` (new, `src/openhuman/projects/run_registry.rs`)

A process-global `Mutex<HashMap<task_id, AbortHandle>>` singleton.

```rust
pub fn register(task_id: &str, handle: AbortHandle);
pub fn cancel(task_id: &str) -> bool;     // returns true if a run was found and aborted
pub fn is_running(task_id: &str) -> bool;
pub fn list_running() -> Vec<String>;
```

- `bus.rs` calls `register()` after `tokio::spawn`, stores the `AbortHandle` from `tokio::task::spawn` (via `JoinHandle::abort_handle()`).
- On abort, the spawned task gets a `JoinError::is_cancelled()` — `run_ai_task` detects cancellation in the `outcome` and writes a "Cancelled by user" comment then moves task to Blocked.

### Backend: New socket event `project:task_log`

Rather than repurposing `WebChannelEvent` (which is chat-turn-specific), publish a lightweight dedicated event via `publish_web_channel_event` with `client_id = "system"` (broadcast to all connected clients):

```json
{
  "event": "project:task_log",
  "client_id": "system",
  "thread_id": "",
  "request_id": "",
  "project_task_id": "<task_id>",
  "line": "<log text>",
  "kind": "log" | "done" | "cancelled" | "error"
}
```

`bus.rs` emits this event after each meaningful agent progress point (turn start, tool call, completion). "Meaningful" = not every token; rather:
- `kind: log` — one line when AI starts, one per tool call, one with the final response summary
- `kind: done` | `kind: cancelled` | `kind: error` — terminal events

### Backend: New RPC `openhuman.projects_cancel_ai_task`

```
Input:  { task_id: string }
Output: { cancelled: boolean }
```

Handler calls `RunRegistry::cancel(task_id)`. If found: aborts, returns `{ cancelled: true }`. If not found (already finished): returns `{ cancelled: false }`.

### Backend: New RPC `openhuman.projects_list_running_ai_tasks`

```
Input:  {}
Output: { task_ids: string[] }
```

Used by the frontend on reconnect to restore the running indicator for in-flight tasks.

---

## Frontend

### `useAiTaskRuns` hook (new, `app/src/components/projects/useAiTaskRuns.ts`)

Subscribes to the `project:task_log` socket event. Maintains:

```ts
type AiTaskRun = {
  taskId: string;
  lines: string[];          // all log lines received
  status: 'running' | 'done' | 'cancelled' | 'error';
};
Map<taskId, AiTaskRun>
```

On mount: calls `projects_list_running_ai_tasks` RPC to seed the running set (handles reconnect case). Clears a run entry 30 seconds after a terminal event.

### KanbanCard changes

- If `task.assignee === 'ai'` and `runs.has(task.id)`: show a small spinning indicator next to the AI badge.
- Show the last log line below the title, truncated to one line (`max-w-full truncate text-xs text-stone-400`).

### TaskDetailDrawer changes

- When run is active: show a "Running" section above the comments list with:
  - All log lines in a scrollable `<pre>` block (auto-scrolls to bottom as lines arrive).
  - A "Stop" button (red, with confirmation: "Stop AI and move to Blocked?") that calls `projects_cancel_ai_task`.
- When run ends: the section stays visible with final status until user dismisses or closes drawer.

---

## Data Flow

```
bus.rs: tokio::spawn(run_ai_task)
  → RunRegistry::register(task_id, abort_handle)
  → [per progress point] publish_web_channel_event(project:task_log { kind: log })
  → on completion/cancel/error: publish_web_channel_event(project:task_log { kind: done|cancelled|error })
  → RunRegistry::deregister(task_id)

Frontend socketService
  → on('project:task_log') → useAiTaskRuns updates state
  → KanbanCard re-renders (spinner + last line)
  → TaskDetailDrawer re-renders (log panel)

User clicks "Stop"
  → callCoreRpc(projects_cancel_ai_task, { task_id })
  → RunRegistry::cancel → AbortHandle::abort()
  → run_ai_task detects cancellation → writes comment → moves to Blocked
  → emits project:task_log { kind: cancelled }
```

---

## Error Handling

- **Core restarts** while task is running: `AbortHandle` is lost. Task stays in "Doing" bucket. Frontend detects no `project:task_log` events on reconnect + `list_running_ai_tasks` returns empty → no spinner shown. User can manually move task back to To Do and re-assign to AI.
- **Cancel called after completion**: RPC returns `{ cancelled: false }`, frontend shows "Task already finished" toast.
- **Socket disconnect mid-run**: lines buffer in `useAiTaskRuns`; on reconnect the `list_running_ai_tasks` call restores the running state but prior log lines are lost (no replay — acceptable given the log is also written as a task attachment on completion).

---

## Files Changed

| File | Change |
|------|--------|
| `src/openhuman/projects/run_registry.rs` | New — global `AbortHandle` registry |
| `src/openhuman/projects/bus.rs` | Register run, emit `project:task_log` events, detect cancellation |
| `src/openhuman/projects/ops.rs` | New `cancel_ai_task` and `list_running_ai_tasks` ops |
| `src/openhuman/projects/schemas.rs` | New RPC schemas + handlers |
| `src/openhuman/projects/mod.rs` | Re-export `run_registry` |
| `src/core/event_bus/events.rs` | No change — uses existing `publish_web_channel_event` |
| `app/src/components/projects/useAiTaskRuns.ts` | New hook |
| `app/src/components/projects/KanbanCard.tsx` | Spinner + last-line summary |
| `app/src/components/projects/TaskDetailDrawer.tsx` | Log panel + Stop button |
| `app/src/services/api/projectsApi.ts` | `cancelAiTask`, `listRunningAiTasks` |
| `app/src/services/socketService.ts` | Subscribe to `project:task_log` event |

---

## Out of Scope

- Log replay after socket reconnect (logs are stored as task attachments post-completion).
- Soft cancel / "pause and resume".
- Per-tool-call granularity streaming (too noisy; milestone-level log lines only).
