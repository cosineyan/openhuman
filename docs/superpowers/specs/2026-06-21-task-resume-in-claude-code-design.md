# Task Resume in Claude Code CLI

**Date:** 2026-06-21  
**Status:** Approved for implementation

## Overview

When an AI-assigned project task finishes (Done or Blocked), the user can open a native terminal window with `claude --resume <session-uuid>` pre-loaded, allowing them to continue the task interactively in their local Claude Code CLI environment — with full session history, skills, and plugins intact.

## Background

Project tasks run via `bus.rs` → `Agent::from_config` → `ClaudeCodeProvider`, which spawns `claude -p` with a fixed `--session-id`. The session UUID is persisted in `~/.openhuman/users/<uid>/claude-code-sessions.json` (keyed by a hash of the first user message). The `Task.ai_plan` field is currently unused (reserved for Phase 4).

## Architecture

### 1. Data Layer — Write session UUID to `ai_plan` (Rust, `bus.rs`)

After the AI turn completes and before moving the task to Done/Blocked, `bus.rs` computes the `thread_id` for the task's prompt and looks up the claude session UUID from the session store. It writes the UUID into `task.ai_plan` as a JSON object.

**`thread_id` computation:** `build_prompt(title, description)` produces the full prompt string. The `thread_id` is `hash_<sha256_truncated>` of the first user message — which for project tasks is the entire prompt (there is no prior history). This can be computed directly in `bus.rs` using the same SHA-256 logic as `thread_key_from_messages` in `claude_code/mod.rs`.

**`ai_plan` format:**
```json
{ "claude_session_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890" }
```

Future Phase 4 fields can be added to the same JSON object without conflict.

**Session UUID access:** Add `get_session_id_for_prompt(prompt: &str) -> Option<String>` to `bus.rs` (local helper, not on the `Provider` trait). It computes the hash and reads `claude-code-sessions.json` directly — same path logic as `SessionStore::open`.

This write only happens when `chat_provider` is `claude-code:*`. For other providers, `ai_plan` is left unchanged (no UUID to store).

**Affected files:**
- `src/openhuman/projects/bus.rs` — add `get_session_id_for_prompt`, call it at task completion, patch `ai_plan`
- `src/openhuman/projects/types.rs` — verify `TaskPatch.ai_plan` supports `Option<Option<String>>` (double-option for null/absent distinction; already present)

### 2. Tauri Command Layer — `claude_code_resume_session`

Extract the shared terminal-opening logic from `claude_code_login_launch` into a private `open_terminal_with_command(cmd: &str)` helper, then add a new command:

```rust
#[tauri::command]
pub fn claude_code_resume_session(session_id: String) -> Result<String, String> {
    if !is_uuid_v4(&session_id) {
        return Err("invalid session id".into());
    }
    open_terminal_with_command(&format!("claude --resume {session_id}"))
}
```

Platform behaviour (identical to existing login launch):
- **macOS:** `osascript` → `Terminal.app do script "claude --resume <uuid>"`
- **Windows:** `cmd /c start "" cmd /k claude --resume <uuid>`
- **Linux:** tries `x-terminal-emulator`, `gnome-terminal`, `konsole`, `xfce4-terminal`, `xterm` in order

`is_uuid_v4` is imported from `src/openhuman/inference/provider/claude_code/session_store.rs` (already public).

Register the new command in `lib.rs` alongside `claude_code_login_launch`.

**Affected files:**
- `app/src-tauri/src/claude_code.rs` — extract helper, add `claude_code_resume_session`
- `app/src-tauri/src/lib.rs` — register new command
- `app/src/utils/tauriCommands/config.ts` — add `openhumanClaudeCodeResumeSession(sessionId: string)`

### 3. Frontend UI — Resume area in TaskDetailDrawer

**Display condition** (all must be true):
- `task.assignee === 'ai'`
- `task.ai_plan` parses as valid JSON containing a non-empty `claude_session_id` string
- Task is in a terminal state: `bucket.is_done_bucket === true` OR `bucket.title.toLowerCase().includes('block')`

**Layout** (rendered between the AI run log card and the tab bar, same visual weight as the run card):

```
┌──────────────────────────────────────────────────────┐
│  Continue in Claude Code                             │
│                                                      │
│  claude --resume a1b2c3d4-e5f6-...   [Copy]         │
│                                                      │
│             [Open in Terminal]                       │
└──────────────────────────────────────────────────────┘
```

- Header: small label "Continue in Claude Code" with a terminal icon
- Command line: `font-mono text-xs`, full UUID shown, copy button on the right
  - Copy writes to clipboard via `navigator.clipboard.writeText`
  - Button briefly shows "Copied!" (1.5 s) then reverts
- "Open in Terminal" button: calls `openhumanClaudeCodeResumeSession(uuid)`
  - Success: button briefly shows "Terminal opened" (2 s)
  - Error: inline error text below button ("Could not open terminal — copy the command above and run it manually")
- Component: `ClaudeCodeResumeCard` — new file `app/src/components/projects/ClaudeCodeResumeCard.tsx`
- i18n: add keys to `en.ts` and all locale files (`ar`, `bn`, `de`, `es`, `fr`, `hi`, `id`, `it`, `ko`, `pl`, `pt`, `ru`, `zh-CN`)

**Affected files:**
- `app/src/components/projects/ClaudeCodeResumeCard.tsx` — new component
- `app/src/components/projects/TaskDetailDrawer.tsx` — render `ClaudeCodeResumeCard` in the right place
- `app/src/lib/i18n/locales/en.ts` + all locale files

### 4. Error Handling

| Situation | Behaviour |
|---|---|
| `ai_plan` missing or not valid JSON | Resume area not shown; silent |
| `ai_plan` JSON has no `claude_session_id` | Resume area not shown; silent |
| Session UUID present but expired server-side | Terminal opens, user sees claude's error in terminal; no UI pre-check needed |
| `claude` binary not found / terminal open fails | Inline error: "Could not open terminal — copy the command above and run it manually" |
| Task re-assigned to AI and run again | `ai_plan` overwritten with new UUID; Resume area shows latest session |
| Provider is not `claude-code:*` | `ai_plan` not written; Resume area not shown |

## Data Flow Summary

```
bus.rs task completes
  → compute thread_id = sha256(build_prompt(title, desc))
  → read claude-code-sessions.json → get UUID
  → store::update_task ai_plan = {"claude_session_id": "<uuid>"}
  → move task to Done/Blocked bucket

TaskDetailDrawer renders
  → task.ai_plan parsed → claude_session_id extracted
  → bucket is done/blocked + assignee=ai
  → ClaudeCodeResumeCard shown

User clicks "Open in Terminal"
  → invoke('claude_code_resume_session', { sessionId: uuid })
  → Tauri opens Terminal.app with `claude --resume <uuid>`
  → claude CLI loads full session history
  → user continues interactively
```

## Out of Scope

- Detecting whether the claude session is still valid before showing the button (network call, not worth it)
- Supporting non-claude-code providers (native openhuman agent has no resume concept)
- Adding resume to KanbanCard or AiRunDrawer (can be added later without design changes)
