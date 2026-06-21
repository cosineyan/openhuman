//! Tauri commands for the Claude Code CLI provider.
//!
//! Provides cross-platform helpers for opening a native terminal and running
//! a `claude` command inside it. Used for `claude login` (OAuth flow) and
//! `claude --resume <uuid>` (session resume).

use std::process::Command;

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
        // AppleScript `do script` wraps the command in double-quotes, so any
        // literal `"` inside the command must be escaped as `\"`.
        let escaped = cmd.replace('"', "\\\"");
        // If Terminal.app has no windows yet (first launch), `do script` would
        // open a blank window then a second one for the command. Run in the
        // front window when one exists to avoid the extra blank window.
        let script = format!(
            "tell application \"Terminal\"\nif (count of windows) is 0 then\ndo script \"{escaped}\"\nelse\ndo script \"{escaped}\" in front window\nend if\nactivate\nend tell"
        );
        Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map_err(|e| format!("failed to open Terminal.app: {e}"))?;
        return Ok("Terminal.app".into());
    }

    #[cfg(target_os = "linux")]
    {
        let terminals: &[(&str, &[&str])] = &[
            ("x-terminal-emulator", &["-e", cmd]),
            ("gnome-terminal", &["--", cmd]),
            ("konsole", &["-e", cmd]),
            ("xfce4-terminal", &["-e", cmd]),
            ("xterm", &["-e", cmd]),
        ];
        for (term, args) in terminals {
            match Command::new(term).args(*args).spawn() {
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

/// Validate that `s` is a well-formed RFC-4122 v4 UUID.
fn is_valid_uuid_v4(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    let hyphens = [8usize, 13, 18, 23];
    for (i, c) in b.iter().enumerate() {
        if hyphens.contains(&i) {
            if *c != b'-' {
                return false;
            }
        } else if !c.is_ascii_hexdigit() {
            return false;
        }
    }
    // Version nibble (index 14) must be '4'.
    // Variant nibble (index 19) must be one of 8/9/a/b.
    b[14] == b'4' && matches!(b[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')
}

/// Open the user's native terminal and run `claude login` inside it.
///
/// Returns the name of the terminal emulator launched (for UI confirmation)
/// or an error string if no terminal could be opened.
#[tauri::command]
pub fn claude_code_login_launch() -> Result<String, String> {
    open_terminal_with_command("claude login")
}

/// Open the user's native terminal and run `claude --resume <session_id>`.
///
/// `workspace_dir` must be the directory openhuman used as cwd when it ran
/// the task — claude resolves session files relative to cwd via
/// `~/.claude/projects/<sanitized-cwd>/`. The terminal is opened with
/// `cd <workspace_dir> && claude --resume <uuid>` so the session is found.
///
/// Returns the terminal emulator name on success, or an error string.
/// Fails fast with an error if `session_id` is not a valid UUID v4.
#[tauri::command]
pub fn claude_code_resume_session(
    session_id: String,
    workspace_dir: Option<String>,
) -> Result<String, String> {
    if !is_valid_uuid_v4(&session_id) {
        return Err(format!("invalid session id: {session_id}"));
    }
    let cmd = match workspace_dir.as_deref().filter(|s| !s.is_empty()) {
        Some(dir) => format!("cd \"{dir}\" && claude --resume {session_id}"),
        None => format!("claude --resume {session_id}"),
    };
    open_terminal_with_command(&cmd)
}
