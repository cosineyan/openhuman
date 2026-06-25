//! Rust wrapper for the bundled m365-cli Python tool.
//!
//! Calls `python3 <m365_cli.py> <subcommand> --json` as a subprocess and
//! parses the JSON output. The m365-cli source lives at
//! `src/openhuman/m365/cli/` and is bundled into the Tauri resource dir.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::openhuman::config::Config;

// ---------------------------------------------------------------------------
// Script resolution
// ---------------------------------------------------------------------------

/// Find the m365_cli.py entry-point.
///
/// Resolution order (stops at first hit):
/// 1. `M365_CLI_SCRIPT` env var override (testing / power users)
/// 2. Adjacent to the running executable (bundled release)
/// 3. Walk up from CWD to find the repo `src/openhuman/m365/cli/` (dev)
pub fn resolve_m365_cli_script() -> Option<PathBuf> {
    // 1. Env override
    if let Ok(path) = std::env::var("M365_CLI_SCRIPT") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }

    // 2. Next to the running binary (bundled)
    if let Ok(exe) = std::env::current_exe() {
        for dir in [
            exe.parent().map(|p| p.to_path_buf()),
            exe.parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf()),
        ]
        .into_iter()
        .flatten()
        {
            let candidate = dir
                .join("openhuman")
                .join("m365")
                .join("cli")
                .join("m365_cli.py");
            if candidate.is_file() {
                return Some(candidate);
            }
            // Tauri flattens resources: try direct sibling
            let candidate2 = dir.join("m365_cli.py");
            if candidate2.is_file() {
                return Some(candidate2);
            }
        }
    }

    // 3. Walk up from CWD to find repo root (dev mode)
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for up in 0..=8 {
        let mut base = cwd.clone();
        let mut ok = true;
        for _ in 0..up {
            if !base.pop() {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }
        let candidate = base
            .join("src")
            .join("openhuman")
            .join("m365")
            .join("cli")
            .join("m365_cli.py");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Path to the token file stored inside openhuman's workspace directory.
/// Keeps M365 credentials together with other openhuman state.
pub fn token_file_path(config: &Config) -> PathBuf {
    config.workspace_dir.join("m365").join("tokens.json")
}

// ---------------------------------------------------------------------------
// Subprocess helper
// ---------------------------------------------------------------------------

async fn run_m365_cli(args: &[&str], config: &Config) -> Result<Value> {
    let script = resolve_m365_cli_script().context(
        "m365_cli.py not found. Check bundled resources or set M365_CLI_SCRIPT env var.",
    )?;

    let token_file = token_file_path(config);
    if let Some(parent) = token_file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create m365 token dir: {}", parent.display()))?;
    }

    let output = tokio::process::Command::new("python3")
        .arg(&script)
        .args(args)
        .env("M365_TOKEN_FILE", token_file.to_string_lossy().as_ref())
        .output()
        .await
        .context("spawn python3 for m365-cli")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() && stdout.trim().is_empty() {
        anyhow::bail!("m365-cli exited {}: {}", output.status, stderr.trim());
    }

    // Parse the last non-empty line as JSON (m365-cli --json prints one object)
    let json_line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or(stdout.trim());

    serde_json::from_str(json_line).with_context(|| format!("parse m365-cli JSON: {json_line}"))
}

// ---------------------------------------------------------------------------
// Public ops
// ---------------------------------------------------------------------------

/// Return token status for graph, rest, and teams.
pub async fn token_status(config: &Config) -> Result<Value> {
    run_m365_cli(&["auth", "status", "--json"], config).await
}

/// Check whether the mcp-chrome browser extension is reachable on port 12306.
/// Returns `{ ok: bool, port: number, error?: string }`.
pub async fn mcp_chrome_status() -> Value {
    let port = std::env::var("MCP_CHROME_PORT")
        .or_else(|_| std::env::var("CHROME_MCP_PORT"))
        .unwrap_or_else(|_| "12306".to_string());
    let url = format!("http://127.0.0.1:{port}/browser");
    let body = r#"{"command":"sessions"}"#;

    let result = tokio::process::Command::new("python3")
        .arg("-c")
        .arg(format!(
            "import urllib.request, json; \
             req=urllib.request.Request('{url}',data=b'{body}',headers={{'Content-Type':'application/json'}},method='POST'); \
             r=urllib.request.urlopen(req,timeout=3); \
             print(r.read().decode())"
        ))
        .output()
        .await;

    match result {
        Ok(out) if out.status.success() => {
            serde_json::json!({ "ok": true, "port": port.parse::<u16>().unwrap_or(12306) })
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            serde_json::json!({ "ok": false, "port": port.parse::<u16>().unwrap_or(12306), "error": err })
        }
        Err(e) => {
            serde_json::json!({ "ok": false, "port": port.parse::<u16>().unwrap_or(12306), "error": e.to_string() })
        }
    }
}

/// Extract tokens from Chrome (opens Outlook tab if needed).
/// Returns updated token status.
pub async fn auth_login(config: &Config) -> Result<Value> {
    run_m365_cli(&["auth", "login", "--json"], config).await
}

/// Force re-extract tokens even if not expired.
/// Returns updated token status.
pub async fn auth_refresh(config: &Config) -> Result<Value> {
    run_m365_cli(&["auth", "refresh", "--json"], config).await
}

/// Clear cached tokens.
pub async fn auth_logout(config: &Config) -> Result<()> {
    run_m365_cli(&["auth", "logout"], config).await?;
    Ok(())
}
