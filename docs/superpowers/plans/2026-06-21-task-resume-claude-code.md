# Task Resume in Claude Code CLI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When an AI project task finishes (Done or Blocked via `claude-code:` provider), store the claude session UUID in `task.ai_plan` and surface a "Continue in Claude Code" panel in TaskDetailDrawer that lets the user open a native terminal with `claude --resume <uuid>` pre-loaded.

**Architecture:** (1) `bus.rs` computes the task prompt hash, looks up the claude session UUID from `~/.openhuman/users/<uid>/claude-code-sessions.json`, and writes it into `task.ai_plan` as `{"claude_session_id":"<uuid>"}`. (2) A new Tauri command `claude_code_resume_session` opens the native terminal with the resume command. (3) A new `ClaudeCodeResumeCard` React component reads `ai_plan`, shows the command, and calls the Tauri command on click.

**Tech Stack:** Rust (sha2, serde_json, existing `SessionStore`), Tauri commands, React + TypeScript, Tailwind CSS, i18n via `useT()`

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `src/openhuman/projects/bus.rs` | Modify | Add `session_id_for_prompt()` helper; write UUID to `ai_plan` at task completion |
| `app/src-tauri/src/claude_code.rs` | Modify | Extract `open_terminal_with_command()`; add `claude_code_resume_session` command |
| `app/src-tauri/src/lib.rs` | Modify | Register `claude_code_resume_session` in `invoke_handler` |
| `app/src/utils/tauriCommands/config.ts` | Modify | Add `openhumanClaudeCodeResumeSession()` wrapper |
| `app/src/components/projects/ClaudeCodeResumeCard.tsx` | Create | Resume panel component |
| `app/src/components/projects/TaskDetailDrawer.tsx` | Modify | Render `ClaudeCodeResumeCard` between run card and tab bar |
| `app/src/lib/i18n/en.ts` + all locale files | Modify | Add i18n keys |

---

## Task 1: Rust — Write session UUID to `ai_plan` in `bus.rs`

**Files:**
- Modify: `src/openhuman/projects/bus.rs`

- [ ] **Step 1: Add `session_id_for_prompt` helper to `bus.rs`**

  Add this function near the bottom of `bus.rs`, after `build_prompt`:

  ```rust
  /// Look up the claude session UUID for a given task prompt.
  ///
  /// Computes the same thread_id hash that `ClaudeCodeProvider` uses
  /// (`hash_<first-16-bytes-of-sha256-of-prompt>`) then reads the
  /// session store at `<config_dir>/claude-code-sessions.json`.
  /// Returns `None` when the provider is not claude-code, the session
  /// store does not exist, or no entry is found for this prompt.
  fn session_id_for_prompt(config: &crate::openhuman::config::Config, prompt: &str) -> Option<String> {
      // Only attempt lookup when chat_provider is claude-code:*
      let is_claude_code = config
          .chat_provider
          .as_deref()
          .map(|p| p.starts_with("claude-code:"))
          .unwrap_or(false);
      if !is_claude_code {
          return None;
      }

      // Compute thread_id: SHA-256 of prompt, first 16 bytes as hex
      use sha2::{Digest, Sha256};
      let digest = Sha256::digest(prompt.as_bytes());
      let thread_id = format!(
          "hash_{:032x}",
          u128::from_be_bytes(digest[..16].try_into().ok()?)
      );

      // Read session store: <config_dir>/claude-code-sessions.json
      let store_path = config.config_path.parent()?.join("claude-code-sessions.json");
      let content = std::fs::read_to_string(&store_path).ok()?;
      let store: serde_json::Value = serde_json::from_str(&content).ok()?;
      let uuid = store["sessions"][&thread_id].as_str()?.to_string();
      if uuid.is_empty() {
          return None;
      }
      log::debug!(
          "[projects::ai_runner] session_id_for_prompt thread_id={thread_id} uuid={uuid}"
      );
      Some(uuid)
  }
  ```

- [ ] **Step 2: Call `session_id_for_prompt` and patch `ai_plan` at task completion**

  In `run_ai_task`, the `outcome` match currently moves the task to Done/Blocked. Add the `ai_plan` write **before** the bucket move, in both the Done and Blocked arms. The change is in the `Ok(response)` arm and the `Err(err_msg)` arm.

  Find the `Ok(response) =>` arm (currently around line 221). Add immediately after `let _ = store::add_comment(...)`:

  ```rust
  // Persist claude session UUID into ai_plan so the UI can offer resume.
  if let Some(uuid) = session_id_for_prompt(&config, &prompt) {
      let plan = serde_json::json!({ "claude_session_id": uuid }).to_string();
      let _ = store::update_task(
          &config,
          &task_id,
          &crate::openhuman::projects::TaskPatch {
              ai_plan: Some(plan),
              ..crate::openhuman::projects::TaskPatch::default()
          },
          "ai",
      );
  }
  ```

  Add the **same block** in the `Err(err_msg)` arm (currently around line 263), after `let _ = store::add_comment(...)` and before the bucket move.

  Also add the same block in the `was_cancelled` arm (currently around line 205), after the `add_comment` call — so that even cancelled tasks record the UUID (the session is still valid for manual resume).

- [ ] **Step 3: Add `sha2` import to `bus.rs` top-level (it's already in `Cargo.toml`)**

  At the top of `bus.rs`, `sha2` is used inline in the function. No top-level `use` is needed since we reference `sha2::Digest` and `sha2::Sha256` with full paths inside the function. Confirm `sha2` is in `Cargo.toml`:

  ```bash
  grep "sha2" Cargo.toml
  ```
  Expected: `sha2 = "0.10"`

- [ ] **Step 4: Verify `TaskPatch` has `ai_plan` field**

  ```bash
  grep -n "ai_plan" src/openhuman/projects/types.rs
  ```
  Expected output includes: `pub ai_plan: Option<String>,` inside `struct TaskPatch`.

  If `ai_plan` is NOT in `TaskPatch` (it currently only exists in `Task`), add it now:

  In `src/openhuman/projects/types.rs`, inside `struct TaskPatch` (after the `done: Option<bool>` field):
  ```rust
  pub ai_plan: Option<String>,
  ```

  And verify `store::update_task` handles it — check `src/openhuman/projects/store.rs` for the UPDATE SQL. If `ai_plan` is not in the SET clause, add it.

- [ ] **Step 5: Cargo check**

  ```bash
  GGML_NATIVE=OFF cargo check --manifest-path Cargo.toml 2>&1 | grep "^error\[" | head -20
  ```
  Expected: no output (no errors).

- [ ] **Step 6: Commit**

  ```bash
  git add src/openhuman/projects/bus.rs src/openhuman/projects/types.rs src/openhuman/projects/store.rs
  git commit -m "feat(projects): write claude session UUID to ai_plan on task completion"
  ```

---

## Task 2: Tauri — Add `claude_code_resume_session` command

**Files:**
- Modify: `app/src-tauri/src/claude_code.rs`
- Modify: `app/src-tauri/src/lib.rs`

- [ ] **Step 1: Extract `open_terminal_with_command` helper and add resume command**

  Replace the entire contents of `app/src-tauri/src/claude_code.rs` with:

  ```rust
  //! Tauri commands for the Claude Code CLI provider.
  //!
  //! Provides cross-platform helpers for opening a native terminal and running
  //! a `claude` command inside it. Used for `claude login` (OAuth flow) and
  //! `claude --resume <uuid>` (session resume).

  use std::process::Command;

  use crate::openhuman::inference::provider::claude_code::session_store::is_uuid_v4;

  /// Open the user's native terminal and run `cmd` inside it.
  ///
  /// Platform behaviour:
  ///   - Windows: `cmd /c start "" cmd /k <cmd>`
  ///   - macOS:   `osascript` → Terminal.app `do script "<cmd>"`
  ///   - Linux:   try `x-terminal-emulator`, then `gnome-terminal`,
  ///              `konsole`, `xterm` in that order
  fn open_terminal_with_command(cmd: &str) -> Result<String, String> {
      #[cfg(target_os = "windows")]
      {
          Command::new("cmd")
              .args(["/c", "start", "", "cmd", "/k", cmd])
              .spawn()
              .map_err(|e| format!("failed to open cmd: {e}"))?;
          return Ok("cmd".into());
      }

      #[cfg(target_os = "macos")]
      {
          let script = format!(
              r#"tell application "Terminal"
      activate
      do script "{cmd}"
  end tell"#
          );
          Command::new("osascript")
              .args(["-e", &script])
              .spawn()
              .map_err(|e| format!("failed to open Terminal.app: {e}"))?;
          return Ok("Terminal.app".into());
      }

      #[cfg(target_os = "linux")]
      {
          let terminals: &[(&str, Vec<String>)] = &[
              ("x-terminal-emulator", vec!["-e".into(), cmd.into()]),
              ("gnome-terminal", vec!["--".into(), cmd.into()]),
              ("konsole", vec!["-e".into(), cmd.into()]),
              ("xfce4-terminal", vec!["-e".into(), cmd.into()]),
              ("xterm", vec!["-e".into(), cmd.into()]),
          ];
          for (term, args) in terminals {
              match Command::new(term).args(args).spawn() {
                  Ok(_) => return Ok(term.to_string()),
                  Err(_) => continue,
              }
          }
          return Err(
              "no terminal emulator found (tried x-terminal-emulator, gnome-terminal, konsole, \
               xfce4-terminal, xterm). Run the command manually.".into()
          );
      }

      #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
      {
          Err("open_terminal_with_command is not supported on this platform".into())
      }
  }

  /// Open the user's native terminal and run `claude login` inside it.
  #[tauri::command]
  pub fn claude_code_login_launch() -> Result<String, String> {
      open_terminal_with_command("claude login")
  }

  /// Open the user's native terminal and run `claude --resume <session_id>`.
  ///
  /// Returns the terminal emulator name on success, or an error string.
  /// Fails fast with "invalid session id" if `session_id` is not a valid UUID v4.
  #[tauri::command]
  pub fn claude_code_resume_session(session_id: String) -> Result<String, String> {
      if !is_uuid_v4(&session_id) {
          return Err(format!("invalid session id: {session_id}"));
      }
      open_terminal_with_command(&format!("claude --resume {session_id}"))
  }
  ```

  Note: the `use crate::openhuman::...` import path is for the Tauri shell crate (`app/src-tauri`). Verify this path compiles in step 3.

- [ ] **Step 2: Register `claude_code_resume_session` in `lib.rs`**

  In `app/src-tauri/src/lib.rs`, find the line:
  ```rust
  claude_code::claude_code_login_launch,
  ```
  Add the new command immediately after it:
  ```rust
  claude_code::claude_code_login_launch,
  claude_code::claude_code_resume_session,
  ```

- [ ] **Step 3: Cargo check the Tauri crate**

  ```bash
  GGML_NATIVE=OFF cargo check --manifest-path app/src-tauri/Cargo.toml 2>&1 | grep "^error\[" | head -20
  ```

  If the `use crate::openhuman::...` import fails (the session_store module may not be directly accessible from the Tauri shell), replace the import with an inline UUID v4 validator:

  ```rust
  // Remove the `use` import and add this inline function instead:
  fn is_valid_uuid_v4(s: &str) -> bool {
      let b = s.as_bytes();
      if b.len() != 36 { return false; }
      let hyphens = [8usize, 13, 18, 23];
      for (i, c) in b.iter().enumerate() {
          if hyphens.contains(&i) {
              if *c != b'-' { return false; }
          } else if !c.is_ascii_hexdigit() {
              return false; }
      }
      b[14] == b'4' && matches!(b[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')
  }
  ```
  And update `claude_code_resume_session` to call `is_valid_uuid_v4(&session_id)`.

  Re-run cargo check until clean.

- [ ] **Step 4: Commit**

  ```bash
  git add app/src-tauri/src/claude_code.rs app/src-tauri/src/lib.rs
  git commit -m "feat(tauri): add claude_code_resume_session command"
  ```

---

## Task 3: Frontend — Tauri command wrapper + i18n keys

**Files:**
- Modify: `app/src/utils/tauriCommands/config.ts`
- Modify: `app/src/lib/i18n/en.ts`
- Modify: all locale files (`ar`, `bn`, `de`, `es`, `fr`, `hi`, `id`, `it`, `ko`, `pl`, `pt`, `ru`, `zh-CN`)

- [ ] **Step 1: Add `openhumanClaudeCodeResumeSession` to `config.ts`**

  In `app/src/utils/tauriCommands/config.ts`, find `openhumanClaudeCodeLoginLaunch` (currently around line 327):

  ```typescript
  export async function openhumanClaudeCodeLoginLaunch(): Promise<string> {
    ...
    return await invoke<string>('claude_code_login_launch');
  }
  ```

  Add immediately after it:

  ```typescript
  /**
   * Open the user's native terminal and run `claude --resume <sessionId>`.
   * Returns the terminal emulator name on success.
   * Throws on invalid UUID or if no terminal could be opened.
   */
  export async function openhumanClaudeCodeResumeSession(
    sessionId: string
  ): Promise<string> {
    const { isTauri } = await import('../tauri');
    if (!isTauri()) {
      throw new Error('openhumanClaudeCodeResumeSession requires Tauri');
    }
    return await invoke<string>('claude_code_resume_session', { sessionId });
  }
  ```

- [ ] **Step 2: Add i18n keys to `en.ts`**

  In `app/src/lib/i18n/en.ts`, find a suitable location (e.g., near other `projects.` keys or at the end of the file before the closing `}`) and add:

  ```typescript
  'projects.resumeCard.title': 'Continue in Claude Code',
  'projects.resumeCard.openTerminal': 'Open in Terminal',
  'projects.resumeCard.opening': 'Opening…',
  'projects.resumeCard.opened': 'Terminal opened',
  'projects.resumeCard.copied': 'Copied!',
  'projects.resumeCard.copyError': 'Could not open terminal — copy the command above and run it manually',
  ```

- [ ] **Step 3: Add the same keys to all locale files**

  For each of the 13 locale files (`ar.ts`, `bn.ts`, `de.ts`, `es.ts`, `fr.ts`, `hi.ts`, `id.ts`, `it.ts`, `ko.ts`, `pl.ts`, `pt.ts`, `ru.ts`, `zh-CN.ts`), add the same keys with English values as placeholders (CI enforces parity; translations can follow):

  ```typescript
  'projects.resumeCard.title': 'Continue in Claude Code',
  'projects.resumeCard.openTerminal': 'Open in Terminal',
  'projects.resumeCard.opening': 'Opening…',
  'projects.resumeCard.opened': 'Terminal opened',
  'projects.resumeCard.copied': 'Copied!',
  'projects.resumeCard.copyError': 'Could not open terminal — copy the command above and run it manually',
  ```

- [ ] **Step 4: Verify i18n parity check passes**

  ```bash
  pnpm i18n:check 2>&1 | head -20
  ```
  Expected: no missing key errors.

- [ ] **Step 5: TypeScript check**

  ```bash
  pnpm typecheck 2>&1 | grep "config\.ts\|i18n" | head -10
  ```
  Expected: no new errors in these files.

- [ ] **Step 6: Commit**

  ```bash
  git add app/src/utils/tauriCommands/config.ts app/src/lib/i18n/
  git commit -m "feat(frontend): add claude code resume session command and i18n keys"
  ```

---

## Task 4: Frontend — `ClaudeCodeResumeCard` component

**Files:**
- Create: `app/src/components/projects/ClaudeCodeResumeCard.tsx`

- [ ] **Step 1: Create the component**

  Create `app/src/components/projects/ClaudeCodeResumeCard.tsx`:

  ```typescript
  import { useState } from 'react';

  import { openhumanClaudeCodeResumeSession } from '../../utils/tauriCommands/config';
  import { useT } from '../../lib/i18n/I18nContext';

  interface Props {
    sessionId: string;
  }

  export function ClaudeCodeResumeCard({ sessionId }: Props) {
    const { t } = useT();
    const command = `claude --resume ${sessionId}`;
    const [copyLabel, setCopyLabel] = useState<string | null>(null);
    const [openLabel, setOpenLabel] = useState<string | null>(null);
    const [openError, setOpenError] = useState<string | null>(null);

    const handleCopy = async () => {
      try {
        await navigator.clipboard.writeText(command);
        setCopyLabel(t('projects.resumeCard.copied'));
        setTimeout(() => setCopyLabel(null), 1500);
      } catch {
        // clipboard unavailable — silent
      }
    };

    const handleOpen = async () => {
      setOpenError(null);
      setOpenLabel(t('projects.resumeCard.opening'));
      try {
        await openhumanClaudeCodeResumeSession(sessionId);
        setOpenLabel(t('projects.resumeCard.opened'));
        setTimeout(() => setOpenLabel(null), 2000);
      } catch (err) {
        setOpenLabel(null);
        setOpenError(t('projects.resumeCard.copyError'));
      }
    };

    return (
      <div className="mb-4 rounded-lg border border-stone-200 dark:border-neutral-700 overflow-hidden">
        {/* Header */}
        <div className="flex items-center gap-2 px-3 py-2 bg-stone-50 dark:bg-neutral-800 border-b border-stone-200 dark:border-neutral-700">
          {/* Terminal icon */}
          <svg
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
            className="text-stone-500 dark:text-neutral-400 shrink-0">
            <rect x="1" y="1" width="12" height="12" rx="2" stroke="currentColor" strokeWidth="1.2" />
            <path d="M3.5 5L5.5 7L3.5 9" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
            <path d="M6.5 9H10" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
          <span className="text-xs font-medium text-stone-600 dark:text-neutral-300">
            {t('projects.resumeCard.title')}
          </span>
        </div>

        {/* Command row */}
        <div className="flex items-center gap-2 px-3 py-2 bg-white dark:bg-neutral-900">
          <code className="flex-1 text-xs font-mono text-stone-700 dark:text-neutral-300 truncate">
            {command}
          </code>
          <button
            type="button"
            onClick={() => void handleCopy()}
            className="shrink-0 text-xs text-stone-400 hover:text-stone-700 dark:text-neutral-500 dark:hover:text-neutral-200 transition-colors px-1.5 py-0.5 rounded">
            {copyLabel ?? (
              <svg width="13" height="13" viewBox="0 0 13 13" fill="none">
                <rect x="4.5" y="1" width="7.5" height="9" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                <path d="M1 4.5H8.5V12H1V4.5Z" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
              </svg>
            )}
          </button>
        </div>

        {/* Open button */}
        <div className="px-3 py-2 bg-stone-50 dark:bg-neutral-800 border-t border-stone-100 dark:border-neutral-800">
          <button
            type="button"
            onClick={() => void handleOpen()}
            disabled={openLabel !== null}
            className="w-full text-xs font-medium rounded-md bg-stone-900 dark:bg-neutral-100 text-white dark:text-neutral-900 py-1.5 hover:bg-stone-700 dark:hover:bg-neutral-300 disabled:opacity-60 transition-colors">
            {openLabel ?? t('projects.resumeCard.openTerminal')}
          </button>
          {openError && (
            <p className="mt-1.5 text-xs text-rose-600 dark:text-rose-400">{openError}</p>
          )}
        </div>
      </div>
    );
  }
  ```

- [ ] **Step 2: TypeScript check**

  ```bash
  pnpm typecheck 2>&1 | grep "ClaudeCodeResumeCard" | head -10
  ```
  Expected: no errors.

- [ ] **Step 3: Commit**

  ```bash
  git add app/src/components/projects/ClaudeCodeResumeCard.tsx
  git commit -m "feat(projects): add ClaudeCodeResumeCard component"
  ```

---

## Task 5: Frontend — Wire `ClaudeCodeResumeCard` into `TaskDetailDrawer`

**Files:**
- Modify: `app/src/components/projects/TaskDetailDrawer.tsx`

- [ ] **Step 1: Add helper to parse `ai_plan`**

  Near the top of `TaskDetailDrawer.tsx`, after the imports, add a helper:

  ```typescript
  /** Extract claude_session_id from task.ai_plan JSON, or null if absent/invalid. */
  function parseClaudeSessionId(aiPlan: string | null | undefined): string | null {
    if (!aiPlan) return null;
    try {
      const parsed = JSON.parse(aiPlan) as unknown;
      if (
        parsed !== null &&
        typeof parsed === 'object' &&
        'claude_session_id' in parsed &&
        typeof (parsed as Record<string, unknown>).claude_session_id === 'string' &&
        (parsed as Record<string, unknown>).claude_session_id !== ''
      ) {
        return (parsed as Record<string, unknown>).claude_session_id as string;
      }
    } catch {
      // malformed JSON — silent
    }
    return null;
  }
  ```

- [ ] **Step 2: Add import for `ClaudeCodeResumeCard`**

  In the imports section at the top of `TaskDetailDrawer.tsx`, add:

  ```typescript
  import { ClaudeCodeResumeCard } from './ClaudeCodeResumeCard';
  ```

- [ ] **Step 3: Compute display condition and render the card**

  Inside the main render, find the current bucket from `buckets` and task (there is already `bucketId` state). Add the computation after existing state/hooks:

  ```typescript
  const currentBucket = buckets.find(b => b.id === (task?.bucket_id ?? bucketId));
  const isTerminalState =
    (currentBucket?.is_done_bucket === true) ||
    (currentBucket?.title.toLowerCase().includes('block') ?? false);
  const claudeSessionId = task ? parseClaudeSessionId(task.ai_plan) : null;
  const showResumeCard =
    task?.assignee === 'ai' && claudeSessionId !== null && isTerminalState;
  ```

- [ ] **Step 4: Insert `ClaudeCodeResumeCard` between the run card and the tab bar**

  Find the comment `{/* Tab bar — icon only with count, label as tooltip */}` (currently around line 748). Insert directly above it:

  ```typescript
  {showResumeCard && claudeSessionId && (
    <ClaudeCodeResumeCard sessionId={claudeSessionId} />
  )}
  {/* Tab bar — icon only with count, label as tooltip */}
  ```

- [ ] **Step 5: TypeScript check**

  ```bash
  pnpm typecheck 2>&1 | grep "TaskDetailDrawer\|ClaudeCodeResume" | head -10
  ```
  Expected: no errors.

- [ ] **Step 6: Commit**

  ```bash
  git add app/src/components/projects/TaskDetailDrawer.tsx
  git commit -m "feat(projects): show ClaudeCodeResumeCard in TaskDetailDrawer for done/blocked AI tasks"
  ```

---

## Task 6: Integration verification

- [ ] **Step 1: Build the full app**

  ```bash
  cd app && \
  INSTALL_ROOT=../.cache/cargo-install \
  PATH="$HOME/.cargo/bin:$INSTALL_ROOT/bin:$PATH" \
  CEF_PATH="$HOME/Library/Caches/tauri-cef" \
    cargo tauri dev --config '{"build":{"devUrl":"http://localhost:1420"},"bundle":{"macOS":{"signingIdentity":null}}}' \
    > /tmp/openhuman-dev.log 2>&1 &
  ```

  Wait for `Finished 1 bundle` in the log.

- [ ] **Step 2: Run a project task to completion**

  In the openhuman UI:
  1. Create a project task, assign to AI
  2. Wait for it to move to Done or Blocked
  3. Open the TaskDetailDrawer for that task

- [ ] **Step 3: Verify Resume card appears**

  The "Continue in Claude Code" panel should be visible below the AI run log card.
  - Command text shows `claude --resume <uuid>`
  - Copy button copies to clipboard
  - "Open in Terminal" button opens Terminal.app with the command

- [ ] **Step 4: Verify `ai_plan` was written**

  ```bash
  # Find the task in the recent transcript
  ls -t ~/.openhuman/users/local-mvhklhy3jg/workspace/session_raw/ | grep project-task | head -3

  # Verify ai_plan in the RPC response (check the store directly)
  sqlite3 ~/.openhuman/users/local-mvhklhy3jg/workspace/project_tasks.db \
    "SELECT id, title, ai_plan FROM project_tasks WHERE ai_plan IS NOT NULL ORDER BY updated DESC LIMIT 3;"
  ```

  Expected: rows with `ai_plan` containing `{"claude_session_id":"<uuid>"}`.

- [ ] **Step 5: Final commit if any cleanup needed**

  ```bash
  git add -A
  git commit -m "chore(projects): task resume in claude code — integration cleanup"
  ```
