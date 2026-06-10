# Design: AI Run Log Drawer

**Date:** 2026-06-11
**Status:** Draft

---

## Problem

The current log panel in `TaskDetailDrawer` only shows 2-3 static lines (start, finish, error). There is no way to see what the AI is actually doing in real time while a task is in progress.

---

## Goals

1. Emit granular real-time progress events from the AI runner (tool calls, iterations).
2. Show a compact live "last activity" summary on the task card and in the drawer's existing log panel — clickable to open the full log.
3. A new `AiRunDrawer` slides in from the right showing: task title + status, Stop button, and a full scrolling live log.

---

## Architecture

### Backend: richer `project:task_log` events

`run_agent` in `bus.rs` currently just runs the agent and awaits the final result. Change it to:
1. Create an `mpsc::channel::<AgentProgress>()`.
2. Call `agent.set_on_progress(Some(tx))` before `run_single`.
3. Spawn a task that reads from `rx` and calls `emit_task_log` for each meaningful event:

| `AgentProgress` variant | emitted `kind` | emitted `line` |
|---|---|---|
| `TurnStarted` | `"log"` | `"AI turn started"` |
| `IterationStarted { iteration, .. }` | `"log"` | `"Thinking (step {iteration})…"` |
| `ToolCallStarted { tool_name, .. }` | `"log"` | `"Using tool: {tool_name}"` |
| `ToolCallCompleted { tool_name, success, elapsed_ms, .. }` | `"log"` | `"Tool {tool_name} finished ({elapsed_ms}ms)"` or `"Tool {tool_name} failed"` |
| `TurnCompleted` / `TurnFailed` | skip | (already handled by caller) |

Other variants (`SubagentSpawned`, `TextDelta`, etc.) are skipped to avoid noise.

### Frontend: `AiRunDrawer` (new component)

**File:** `app/src/components/projects/AiRunDrawer.tsx`

Props:
```ts
interface Props {
  task: Task;
  onClose: () => void;
}
```

Layout (top to bottom):
- **Header bar**: task title (truncated) + status chip + Close (×) button + Stop button (red, only when `status === 'running'`)
- **Log area**: `<pre>` full height, font-mono xs, auto-scroll to bottom on new lines, shows all `lines` from `useAiTaskRuns`
- Slides in from right as a fixed overlay with a semi-transparent backdrop (same pattern as TaskDetailDrawer)

### Frontend: entry points

**KanbanCard**: the existing `lastLogLine` paragraph becomes a button. Clicking it opens `AiRunDrawer`.

**TaskDetailDrawer**: the existing log panel header ("AI is working…" / "AI finished — …") becomes a button. Clicking it opens `AiRunDrawer`.

Both pass `task` and `onClose` to `AiRunDrawer`. The drawer state (`isOpen`) lives in the parent (KanbanCard or TaskDetailDrawer).

---

## Data Flow

```
AgentProgress events (Rust mpsc)
  → emit_task_log (bus.rs)
  → publish_web_channel_event (socket broadcast)
  → useAiTaskRuns (frontend hook, already in place)
  → AiRunDrawer re-renders with new lines
```

No new socket event types needed — reuses existing `project:task_log`.

---

## Files Changed

| File | Change |
|------|--------|
| `src/openhuman/projects/bus.rs` | Attach `AgentProgress` channel to agent, emit log events per tool call / iteration |
| `app/src/components/projects/AiRunDrawer.tsx` | New component |
| `app/src/components/projects/KanbanCard.tsx` | Last-line summary → clickable, open AiRunDrawer |
| `app/src/components/projects/TaskDetailDrawer.tsx` | Log panel header → clickable, open AiRunDrawer |

---

## Out of Scope

- Persisting the log after the 30s cleanup window (already stored as task attachment on completion).
- Showing logs for past (non-active) runs.
- Token counts, cost, or raw LLM response streaming.
