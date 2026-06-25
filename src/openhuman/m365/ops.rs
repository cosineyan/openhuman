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
/// 2. Walk up from the running executable — covers both bundled layout and
///    the dev layout where the exe is deep inside target/debug/bundle/...
pub fn resolve_m365_cli_script() -> Option<PathBuf> {
    // 1. Env override
    if let Ok(path) = std::env::var("M365_CLI_SCRIPT") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }

    // 2. Walk up from exe — works for both bundled release and dev mode.
    //    Dev exe:     .../app/src-tauri/target/debug/bundle/macos/OpenHuman.app/Contents/MacOS/OpenHuman
    //    After ~7 pops we reach the repo root, then join src/openhuman/m365/cli/m365_cli.py.
    //    Release exe: .../MacOS/OpenHuman; Tauri copies resources next to it.
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.clone();
        for _ in 0..12 {
            // Bundled release: resources land next to the binary or in a sibling dir.
            for candidate in [
                cur.join("m365_cli.py"),
                cur.join("openhuman").join("m365").join("cli").join("m365_cli.py"),
                // Dev repo layout
                cur.join("src").join("openhuman").join("m365").join("cli").join("m365_cli.py"),
            ] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
            if !cur.pop() {
                break;
            }
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

    // Ensure click is installed — install silently on first use if missing.
    let check = tokio::process::Command::new("python3")
        .args(["-c", "import click"])
        .output()
        .await;
    if check.map(|o| !o.status.success()).unwrap_or(true) {
        log::info!("[m365] click not found, installing via pip…");
        let _ = tokio::process::Command::new("python3")
            .args(["-m", "pip", "install", "click", "-q", "--user"])
            .output()
            .await;
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

    // m365-cli outputs pretty-printed JSON — parse the full stdout.
    let json_str = stdout.trim();
    serde_json::from_str(json_str).with_context(|| format!("parse m365-cli JSON: {json_str}"))
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
