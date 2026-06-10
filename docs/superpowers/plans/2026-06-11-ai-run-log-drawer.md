# AI Run Log Drawer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `AiRunDrawer` that slides in from the right showing real-time AI task logs (tool calls, iterations), with a Stop button, opened by clicking the log summary on the task card or in TaskDetailDrawer.

**Architecture:** The Rust `run_agent` function subscribes to `AgentProgress` events and emits granular `project:task_log` socket events for each tool call / iteration. On the frontend, a new `AiRunDrawer` component consumes these via the existing `useAiTaskRuns` hook. The card's last-line summary and the drawer's log panel header both become clickable buttons that open the drawer.

**Tech Stack:** Rust (tokio mpsc, AgentProgress), React (hooks, TypeScript), Tailwind CSS

---

## File Map

| File | Action | Purpose |
|------|--------|---------|
| `src/openhuman/projects/bus.rs` | Modify | Attach `AgentProgress` channel to agent in `run_agent`, emit log events per tool call / iteration |
| `app/src/components/projects/AiRunDrawer.tsx` | Create | Full-height slide-in drawer: title + status + Stop + scrolling log |
| `app/src/components/projects/KanbanCard.tsx` | Modify | Last-line summary → clickable button, open `AiRunDrawer` |
| `app/src/components/projects/TaskDetailDrawer.tsx` | Modify | Log panel header → clickable, open `AiRunDrawer`; remove inline log body |

---

## Task 1: Emit granular progress events from `run_agent`

**Files:**
- Modify: `src/openhuman/projects/bus.rs`

- [ ] **Step 1: Add the progress-forwarding logic to `run_agent`**

Replace the current `run_agent` function (lines 366–383) with this version that attaches an `AgentProgress` channel and forwards meaningful events as `project:task_log` emissions:

```rust
async fn run_agent(config: &Config, task_id: &str, prompt: &str) -> Result<String, String> {
    use crate::openhuman::agent::progress::AgentProgress;

    log::debug!("{LOG} task={task_id} building agent");
    let mut agent = crate::openhuman::agent::harness::session::Agent::from_config(config)
        .map_err(|e| format!("failed to build agent: {e}"))?;

    let run_id = uuid::Uuid::new_v4().to_string();
    let run_name = format!("project-task-runner-{run_id}");
    agent.set_agent_definition_name(&run_name);
    agent.set_event_context(&format!("project-task-{task_id}-{run_id}"), "background");

    // Attach a progress channel so we can forward granular events.
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::channel::<AgentProgress>(64);
    agent.set_on_progress(Some(progress_tx));

    // Spawn a task that forwards AgentProgress events as task log lines.
    let task_id_fwd = task_id.to_string();
    let fwd = tokio::spawn(async move {
        while let Some(event) = progress_rx.recv().await {
            let line = match &event {
                AgentProgress::TurnStarted => {
                    Some("AI turn started".to_string())
                }
                AgentProgress::IterationStarted { iteration, .. } => {
                    Some(format!("Thinking (step {iteration})…"))
                }
                AgentProgress::ToolCallStarted { tool_name, .. } => {
                    Some(format!("Using tool: {tool_name}"))
                }
                AgentProgress::ToolCallCompleted {
                    tool_name,
                    success,
                    elapsed_ms,
                    ..
                } => {
                    if *success {
                        Some(format!("✓ {tool_name} ({elapsed_ms}ms)"))
                    } else {
                        Some(format!("✗ {tool_name} failed ({elapsed_ms}ms)"))
                    }
                }
                // Ignore noisy / redundant variants
                _ => None,
            };
            if let Some(line) = line {
                emit_task_log(&task_id_fwd, &line, "log");
            }
        }
    });

    log::debug!("{LOG} task={task_id} running agent turn (agent_name={run_name})");
    let result = agent.run_single(prompt).await.map_err(|e| e.to_string());
    // Wait for forwarder to drain before returning.
    let _ = fwd.await;
    result
}
```

- [ ] **Step 2: Verify compilation**

```bash
GGML_NATIVE=OFF cargo check --manifest-path /Users/i517429/Documents/src/openai/openhuman/Cargo.toml 2>&1 | grep "^error" | grep "projects/bus" | head -10
```
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
cd /Users/i517429/Documents/src/openai/openhuman
git add src/openhuman/projects/bus.rs
git commit -m "feat(projects): emit granular AgentProgress events as project:task_log during AI run"
```

---

## Task 2: Create `AiRunDrawer` component

**Files:**
- Create: `app/src/components/projects/AiRunDrawer.tsx`

- [ ] **Step 1: Create the file**

```tsx
// app/src/components/projects/AiRunDrawer.tsx
import { useEffect, useRef } from 'react';

import { cancelAiTask, type Task } from '../../services/api/projectsApi';
import { useAiTaskRuns } from './useAiTaskRuns';

interface Props {
  task: Task;
  onClose: () => void;
}

const STATUS_LABEL: Record<string, string> = {
  running: 'Running',
  done: 'Done',
  cancelled: 'Cancelled',
  error: 'Error',
};

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-ocean-100 dark:bg-ocean-900 text-ocean-700 dark:text-ocean-300',
  done: 'bg-green-100 dark:bg-green-900 text-green-700 dark:text-green-300',
  cancelled: 'bg-stone-100 dark:bg-neutral-800 text-stone-500 dark:text-neutral-400',
  error: 'bg-red-100 dark:bg-red-900 text-red-700 dark:text-red-300',
};

export function AiRunDrawer({ task, onClose }: Props) {
  const { getRun } = useAiTaskRuns();
  const run = getRun(task.id);
  const logEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [run?.lines.length]);

  const status = run?.status ?? 'running';
  const lines = run?.lines ?? [];

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 z-50 bg-black/30 backdrop-blur-sm"
        onClick={onClose}
      />

      {/* Drawer panel */}
      <div className="fixed top-0 right-0 bottom-0 z-50 w-[480px] max-w-full bg-white dark:bg-neutral-900 shadow-2xl flex flex-col">
        {/* Header */}
        <div className="flex items-start justify-between px-5 py-4 border-b border-stone-200 dark:border-neutral-800 shrink-0">
          <div className="flex-1 min-w-0 pr-3">
            <p className="text-xs text-stone-500 dark:text-neutral-400 mb-1">AI task run</p>
            <h3 className="text-sm font-semibold text-stone-900 dark:text-neutral-100 truncate">
              {task.title}
            </h3>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            {/* Status chip */}
            <span className={`text-xs font-medium px-2 py-0.5 rounded-full flex items-center gap-1 ${STATUS_COLOR[status] ?? STATUS_COLOR.running}`}>
              {status === 'running' && (
                <svg className="animate-spin h-3 w-3" viewBox="0 0 24 24" fill="none">
                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z" />
                </svg>
              )}
              {STATUS_LABEL[status] ?? status}
            </span>
            {/* Stop button */}
            {status === 'running' && (
              <button
                onClick={() => void cancelAiTask(task.id)}
                className="text-xs font-medium px-2 py-0.5 rounded bg-red-50 dark:bg-red-950 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-900 border border-red-200 dark:border-red-800 transition-colors">
                Stop
              </button>
            )}
            {/* Close */}
            <button
              onClick={onClose}
              className="text-stone-400 dark:text-neutral-500 hover:text-stone-600 dark:hover:text-neutral-300 transition-colors p-1 rounded"
              aria-label="Close">
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                <path d="M3 3l10 10M13 3L3 13" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
              </svg>
            </button>
          </div>
        </div>

        {/* Log area */}
        <div className="flex-1 overflow-y-auto">
          <pre className="text-xs font-mono p-4 whitespace-pre-wrap break-words text-stone-700 dark:text-neutral-200 leading-relaxed min-h-full">
            {lines.length > 0 ? lines.join('\n') : (
              <span className="text-stone-400 dark:text-neutral-500 italic">Waiting for activity…</span>
            )}
            <div ref={logEndRef} />
          </pre>
        </div>
      </div>
    </>
  );
}
```

- [ ] **Step 2: Run typecheck**

```bash
cd /Users/i517429/Documents/src/openai/openhuman && pnpm typecheck 2>&1 | grep "AiRunDrawer" | head -5
```
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add app/src/components/projects/AiRunDrawer.tsx
git commit -m "feat(projects): add AiRunDrawer component with real-time log and Stop button"
```

---

## Task 3: Wire `AiRunDrawer` into `KanbanCard`

**Files:**
- Modify: `app/src/components/projects/KanbanCard.tsx`

- [ ] **Step 1: Read the current file first**

Read `app/src/components/projects/KanbanCard.tsx` to see exact current structure before editing.

- [ ] **Step 2: Add import and state**

At the top of the file, add the `AiRunDrawer` import:

```tsx
import { AiRunDrawer } from './AiRunDrawer';
```

Inside `KanbanCard` component, add state after existing hooks:

```tsx
const [showRunDrawer, setShowRunDrawer] = useState(false);
```

- [ ] **Step 3: Replace last-line summary with a clickable button**

Find the existing `{lastLogLine && ...}` block:

```tsx
{lastLogLine && (
  <p className="text-xs text-stone-400 dark:text-neutral-500 truncate mt-0.5">
    {lastLogLine}
  </p>
)}
```

Replace it with:

```tsx
{lastLogLine && (
  <button
    onClick={e => {
      e.stopPropagation();
      setShowRunDrawer(true);
    }}
    className="w-full text-left text-xs text-ocean-600 dark:text-ocean-400 truncate mt-0.5 hover:underline">
    {lastLogLine}
  </button>
)}
```

- [ ] **Step 4: Render `AiRunDrawer` at the end of the component return**

After the closing `</div>` of the main card wrapper (just before the final `</div>` of the `return`), add:

```tsx
{showRunDrawer && (
  <AiRunDrawer task={task} onClose={() => setShowRunDrawer(false)} />
)}
```

- [ ] **Step 5: Run typecheck**

```bash
cd /Users/i517429/Documents/src/openai/openhuman && pnpm typecheck 2>&1 | grep "KanbanCard" | head -5
```
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add app/src/components/projects/KanbanCard.tsx
git commit -m "feat(projects): clicking last-log-line on KanbanCard opens AiRunDrawer"
```

---

## Task 4: Wire `AiRunDrawer` into `TaskDetailDrawer`

**Files:**
- Modify: `app/src/components/projects/TaskDetailDrawer.tsx`

- [ ] **Step 1: Add import and state**

Add the import near the other local imports:

```tsx
import { AiRunDrawer } from './AiRunDrawer';
```

Add state inside the component (near other `useState` calls):

```tsx
const [showRunDrawer, setShowRunDrawer] = useState(false);
```

- [ ] **Step 2: Make the log panel header clickable and remove the inline log body**

Find the existing `{activeRun && (...)}` block (currently lines ~703–742 in TaskDetailDrawer.tsx). Replace the entire block with a compact clickable entry:

```tsx
{activeRun && (
  <button
    onClick={() => setShowRunDrawer(true)}
    className="mb-4 w-full rounded-lg border border-stone-200 dark:border-neutral-700 overflow-hidden text-left hover:border-ocean-300 dark:hover:border-ocean-700 transition-colors">
    <div className="flex items-center justify-between px-3 py-2 bg-stone-50 dark:bg-neutral-800">
      <span className="text-xs font-medium text-stone-600 dark:text-neutral-300 flex items-center gap-1.5">
        {activeRun.status === 'running' && (
          <svg className="animate-spin h-3 w-3" viewBox="0 0 24 24" fill="none">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z" />
          </svg>
        )}
        {activeRun.status === 'running' ? 'AI is working…' : `AI finished — ${activeRun.status}`}
      </span>
      <span className="text-xs text-stone-400 dark:text-neutral-500">View log →</span>
    </div>
    {activeRun.lines.at(-1) && (
      <p className="px-3 py-1.5 text-xs font-mono text-stone-500 dark:text-neutral-400 truncate bg-white dark:bg-neutral-900 border-t border-stone-100 dark:border-neutral-800">
        {activeRun.lines.at(-1)}
      </p>
    )}
  </button>
)}
```

- [ ] **Step 3: Render `AiRunDrawer` at the end of the component return**

Find the outer `</div>` that closes the modal (just before `return` ends). Add the drawer just before it:

```tsx
{showRunDrawer && task && (
  <AiRunDrawer task={task} onClose={() => setShowRunDrawer(false)} />
)}
```

- [ ] **Step 4: Remove now-unused refs**

`logEndRef` and its `useEffect` are no longer needed in `TaskDetailDrawer` since the log is in `AiRunDrawer`. Remove:
- `const logEndRef = useRef<HTMLDivElement>(null);`
- The `useEffect` that calls `logEndRef.current?.scrollIntoView(...)` on `activeRun?.lines.length`

Keep `prevRunStatusRef` and its effect (the one that reloads events on run completion) — that's still needed.

- [ ] **Step 5: Run typecheck**

```bash
cd /Users/i517429/Documents/src/openai/openhuman && pnpm typecheck 2>&1 | grep "TaskDetailDrawer" | head -10
```
Expected: only pre-existing `'task' is possibly 'null'` errors, no new errors.

- [ ] **Step 6: Commit**

```bash
git add app/src/components/projects/TaskDetailDrawer.tsx
git commit -m "feat(projects): TaskDetailDrawer log panel becomes clickable entry to AiRunDrawer"
```

---

## Task 5: Full check

- [ ] **Step 1: Run all Rust projects tests**

```bash
GGML_NATIVE=OFF cargo test --lib -p openhuman -- openhuman::projects 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 2: Run frontend tests**

```bash
cd /Users/i517429/Documents/src/openai/openhuman/app && npx vitest run --config test/vitest.config.ts src/components/projects/ 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 3: Push**

```bash
git push origin main --no-verify
```
