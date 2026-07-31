use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::openhuman::config::Config;

use super::types::{BatchQueueEntry, CreateRuleInput, EmailAutomationRule, RulePatch};

fn with_connection<T>(config: &Config, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let db_path = config
        .workspace_dir
        .join("email_automation")
        .join("rules.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create email_automation dir: {}", parent.display()))?;
    }
    let conn = Connection::open(&db_path)
        .with_context(|| format!("open email_automation DB: {}", db_path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("set busy_timeout")?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("enable WAL")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS email_automation_rules (
            id                        TEXT PRIMARY KEY,
            name                      TEXT NOT NULL,
            enabled                   INTEGER NOT NULL DEFAULT 1,
            sender_contains           TEXT,
            subject_contains          TEXT,
            body_contains             TEXT,
            task_title_template       TEXT NOT NULL,
            task_description_template TEXT,
            assignee                  TEXT NOT NULL DEFAULT 'ai',
            bucket_id                 TEXT,
            llm_fallback_enabled      INTEGER NOT NULL DEFAULT 0,
            parse_script              TEXT,
            batch_mode                INTEGER NOT NULL DEFAULT 0,
            batch_window_secs         INTEGER NOT NULL DEFAULT 21600,
            batch_parse_mode          TEXT NOT NULL DEFAULT 'first_only',
            created_at                TEXT NOT NULL,
            updated_at                TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ear_enabled
            ON email_automation_rules(enabled);
        CREATE TABLE IF NOT EXISTS processed_emails (
            source_id    TEXT NOT NULL,
            rule_id      TEXT NOT NULL,
            task_id      TEXT NOT NULL,
            processed_at TEXT NOT NULL,
            PRIMARY KEY (source_id, rule_id)
        );
        CREATE TABLE IF NOT EXISTS email_batch_queue (
            id          TEXT PRIMARY KEY,
            rule_id     TEXT NOT NULL,
            source_id   TEXT NOT NULL UNIQUE,
            email_body  TEXT NOT NULL,
            matched_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ebq_rule_id ON email_batch_queue(rule_id);
        CREATE INDEX IF NOT EXISTS idx_ebq_matched_at ON email_batch_queue(matched_at);",
    )
    .context("migrate email_automation DB")?;

    // Migrate existing DBs
    let _ = conn.execute("ALTER TABLE email_automation_rules ADD COLUMN parse_script TEXT", []);
    let _ = conn.execute("ALTER TABLE email_automation_rules ADD COLUMN batch_mode INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE email_automation_rules ADD COLUMN batch_window_secs INTEGER NOT NULL DEFAULT 21600", []);
    let _ = conn.execute("ALTER TABLE email_automation_rules ADD COLUMN batch_parse_mode TEXT NOT NULL DEFAULT 'first_only'", []);

    f(&conn)
}

fn row_to_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmailAutomationRule> {
    let batch_parse_mode_str: String = row.get(14).unwrap_or_else(|_| "first_only".to_string());
    let batch_parse_mode = match batch_parse_mode_str.as_str() {
        "all" => super::types::BatchParseMode::All,
        _ => super::types::BatchParseMode::FirstOnly,
    };
    Ok(EmailAutomationRule {
        id: row.get(0)?,
        name: row.get(1)?,
        enabled: row.get::<_, i64>(2)? != 0,
        sender_contains: row.get(3)?,
        subject_contains: row.get(4)?,
        body_contains: row.get(5)?,
        task_title_template: row.get(6)?,
        task_description_template: row.get(7)?,
        assignee: row.get(8)?,
        bucket_id: row.get(9)?,
        llm_fallback_enabled: row.get::<_, i64>(10)? != 0,
        parse_script: row.get(11)?,
        batch_mode: row.get::<_, i64>(12).unwrap_or(0) != 0,
        batch_window_secs: row.get::<_, i64>(13).unwrap_or(21600) as u64,
        batch_parse_mode,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

pub fn list_rules(config: &Config) -> Result<Vec<EmailAutomationRule>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, enabled, sender_contains, subject_contains, body_contains,
                    task_title_template, task_description_template, assignee, bucket_id,
                    llm_fallback_enabled, parse_script, batch_mode, batch_window_secs,
                    batch_parse_mode, created_at, updated_at
             FROM email_automation_rules
             ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([], row_to_rule)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

pub fn list_enabled_rules(config: &Config) -> Result<Vec<EmailAutomationRule>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, enabled, sender_contains, subject_contains, body_contains,
                    task_title_template, task_description_template, assignee, bucket_id,
                    llm_fallback_enabled, parse_script, batch_mode, batch_window_secs,
                    batch_parse_mode, created_at, updated_at
             FROM email_automation_rules
             WHERE enabled = 1
             ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([], row_to_rule)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

pub fn get_rule(config: &Config, id: &str) -> Result<Option<EmailAutomationRule>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, enabled, sender_contains, subject_contains, body_contains,
                    task_title_template, task_description_template, assignee, bucket_id,
                    llm_fallback_enabled, parse_script, batch_mode, batch_window_secs,
                    batch_parse_mode, created_at, updated_at
             FROM email_automation_rules WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_rule)?;
        Ok(rows.next().transpose()?)
    })
}

pub fn create_rule(config: &Config, input: CreateRuleInput) -> Result<EmailAutomationRule> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let batch_parse_mode_str = match input.batch_parse_mode {
        super::types::BatchParseMode::All => "all",
        super::types::BatchParseMode::FirstOnly => "first_only",
    };
    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO email_automation_rules
             (id, name, enabled, sender_contains, subject_contains, body_contains,
              task_title_template, task_description_template, assignee, bucket_id,
              llm_fallback_enabled, parse_script, batch_mode, batch_window_secs,
              batch_parse_mode, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                id,
                input.name,
                input.enabled as i64,
                input.sender_contains,
                input.subject_contains,
                input.body_contains,
                input.task_title_template,
                input.task_description_template,
                input.assignee,
                input.bucket_id,
                input.llm_fallback_enabled as i64,
                input.parse_script,
                input.batch_mode as i64,
                input.batch_window_secs as i64,
                batch_parse_mode_str,
                now,
                now,
            ],
        )?;
        Ok(())
    })?;
    get_rule(config, &id)?.context("rule not found after create")
}

pub fn update_rule(config: &Config, id: &str, patch: RulePatch) -> Result<EmailAutomationRule> {
    let now = chrono::Utc::now().to_rfc3339();
    with_connection(config, |conn| {
        if let Some(v) = patch.name {
            conn.execute("UPDATE email_automation_rules SET name=?1, updated_at=?2 WHERE id=?3", params![v, now, id])?;
        }
        if let Some(v) = patch.enabled {
            conn.execute("UPDATE email_automation_rules SET enabled=?1, updated_at=?2 WHERE id=?3", params![v as i64, now, id])?;
        }
        if let Some(v) = patch.sender_contains {
            conn.execute("UPDATE email_automation_rules SET sender_contains=?1, updated_at=?2 WHERE id=?3", params![v, now, id])?;
        }
        if let Some(v) = patch.subject_contains {
            conn.execute("UPDATE email_automation_rules SET subject_contains=?1, updated_at=?2 WHERE id=?3", params![v, now, id])?;
        }
        if let Some(v) = patch.body_contains {
            conn.execute("UPDATE email_automation_rules SET body_contains=?1, updated_at=?2 WHERE id=?3", params![v, now, id])?;
        }
        if let Some(v) = patch.task_title_template {
            conn.execute("UPDATE email_automation_rules SET task_title_template=?1, updated_at=?2 WHERE id=?3", params![v, now, id])?;
        }
        if let Some(v) = patch.task_description_template {
            conn.execute("UPDATE email_automation_rules SET task_description_template=?1, updated_at=?2 WHERE id=?3", params![v, now, id])?;
        }
        if let Some(v) = patch.assignee {
            conn.execute("UPDATE email_automation_rules SET assignee=?1, updated_at=?2 WHERE id=?3", params![v, now, id])?;
        }
        if let Some(v) = patch.bucket_id {
            conn.execute("UPDATE email_automation_rules SET bucket_id=?1, updated_at=?2 WHERE id=?3", params![v, now, id])?;
        }
        if let Some(v) = patch.llm_fallback_enabled {
            conn.execute("UPDATE email_automation_rules SET llm_fallback_enabled=?1, updated_at=?2 WHERE id=?3", params![v as i64, now, id])?;
        }
        if let Some(v) = patch.parse_script {
            conn.execute("UPDATE email_automation_rules SET parse_script=?1, updated_at=?2 WHERE id=?3", params![v, now, id])?;
        }
        if let Some(v) = patch.batch_mode {
            conn.execute("UPDATE email_automation_rules SET batch_mode=?1, updated_at=?2 WHERE id=?3", params![v as i64, now, id])?;
        }
        if let Some(v) = patch.batch_window_secs {
            conn.execute("UPDATE email_automation_rules SET batch_window_secs=?1, updated_at=?2 WHERE id=?3", params![v as i64, now, id])?;
        }
        if let Some(v) = patch.batch_parse_mode {
            let s = match v { super::types::BatchParseMode::All => "all", _ => "first_only" };
            conn.execute("UPDATE email_automation_rules SET batch_parse_mode=?1, updated_at=?2 WHERE id=?3", params![s, now, id])?;
        }
        Ok(())
    })?;
    get_rule(config, id)?.context("rule not found after update")
}

pub fn delete_rule(config: &Config, id: &str) -> Result<()> {
    with_connection(config, |conn| {
        conn.execute("DELETE FROM email_automation_rules WHERE id = ?1", params![id])?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Processed emails history
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ProcessedEmailEntry {
    pub source_id: String,
    pub rule_id: String,
    pub rule_name: String,
    pub task_id: String,
    pub processed_at: String,
}

pub fn list_processed_emails(config: &Config, limit: usize) -> Result<Vec<ProcessedEmailEntry>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT p.source_id, p.rule_id,
                    coalesce(r.name, p.rule_id) as rule_name,
                    p.task_id, p.processed_at
             FROM processed_emails p
             LEFT JOIN email_automation_rules r ON r.id = p.rule_id
             ORDER BY p.processed_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(ProcessedEmailEntry {
                    source_id: row.get(0)?,
                    rule_id: row.get(1)?,
                    rule_name: row.get(2)?,
                    task_id: row.get(3)?,
                    processed_at: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

/// Fetch email subject, sender, to, date and body from memory chunks for display.
/// Returns (subject, from, to, date, body).
/// Email chunks use plain-text format: `[Subject: ...] [From: ...] [Date: ...] [To: ...]\nbody`
pub fn get_email_for_display(config: &Config, source_id: &str) -> Result<Option<(String, String, String, String, String)>> {
    let db_path = config.workspace_dir.join("memory_tree").join("chunks.db");
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = rusqlite::Connection::open(&db_path)
        .context("open chunks db for email display")?;

    // Collect all chunk content fields ordered by seq
    let parts: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT content FROM mem_tree_chunks
             WHERE source_id=?1 AND source_kind='email'
             ORDER BY seq_in_source ASC"
        )?;
        let mut result = Vec::new();
        let mut db_rows = stmt.query(params![source_id])?;
        while let Some(row) = db_rows.next()? {
            result.push(row.get::<_, String>(0)?);
        }
        result
    };

    if parts.is_empty() {
        return Ok(None);
    }

    let full = parts.join("");

    // Auto-detect format: HTML or plain-text bracketed headers
    let (subject, from, to, date, body) = if full.trim_start().starts_with('<') || full.contains("<html") || full.contains("<div") {
        parse_html_email(&full)
    } else {
        // Plain-text format: [Subject: ...] [From: ...] [Date: ...] [To: ...]\nbody
        let subject = extract_bracketed_header(&full, "Subject").unwrap_or_default();
        let from    = extract_bracketed_header(&full, "From").unwrap_or_default();
        let to      = extract_bracketed_header(&full, "To").unwrap_or_default();
        let date    = extract_bracketed_header(&full, "Date").unwrap_or_default();
        let body    = extract_plain_body(&full);
        (subject, from, to, date, body)
    };

    Ok(Some((subject, from, to, date, body)))
}

/// Extract value of a bracketed header: `[Key: value]` anywhere in text.
fn extract_bracketed_header(text: &str, key: &str) -> Option<String> {
    let needle = format!("[{key}: ");
    let start = text.find(&needle)?;
    let inner = &text[start + needle.len()..];
    let end = inner.find(']')?;
    let value = inner[..end].trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// Extract the email body: everything after the leading `[Key: Value]` header blocks.
fn extract_plain_body(text: &str) -> String {
    let mut pos = 0;
    let bytes = text.as_bytes();
    loop {
        while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\n' || bytes[pos] == b'\r') {
            pos += 1;
        }
        if pos >= bytes.len() || bytes[pos] != b'[' {
            break;
        }
        if let Some(close) = text[pos..].find(']') {
            pos += close + 1;
        } else {
            break;
        }
    }
    text[pos..].trim().to_string()
}

/// Extract value after an HTML label like `<b>Subject: </b>text<br>`
fn extract_html_header(html: &str, label: &str) -> Option<String> {
    let pos = html.find(label)?;
    let after = &html[pos + label.len()..];
    let after = if after.starts_with("</") {
        after.find('>').map(|i| &after[i+1..]).unwrap_or(after)
    } else {
        after
    };
    let after = after.trim_start_matches(|c: char| c == ' ' || c == '\u{00a0}');
    let end = after.find("<br").or_else(|| after.find('<')).unwrap_or(after.len());
    let value = after[..end]
        .replace("&lt;", "<").replace("&gt;", ">")
        .replace("&amp;", "&").replace("&nbsp;", " ");
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// Strip HTML tags and decode entities into plain text.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut blank_lines = 0u32;
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => { in_tag = true; }
            '>' => { in_tag = false; out.push('\n'); }
            _ if in_tag => {}
            '&' => {
                let mut entity = String::new();
                for ec in chars.by_ref() {
                    if ec == ';' { break; }
                    entity.push(ec);
                }
                let decoded = match entity.as_str() {
                    "amp" => "&", "lt" => "<", "gt" => ">",
                    "nbsp" | "#160" => " ", "quot" => "\"", "apos" => "'",
                    _ => "",
                };
                if !decoded.is_empty() { out.push_str(decoded); }
            }
            other => { out.push(other); }
        }
    }
    // Collapse multiple blank lines
    let mut result = String::new();
    for line in out.lines() {
        let t = line.trim();
        if t.is_empty() {
            blank_lines += 1;
            if blank_lines <= 1 { result.push('\n'); }
        } else {
            blank_lines = 0;
            result.push_str(t);
            result.push('\n');
        }
    }
    result.trim().to_string()
}

/// Parse HTML email: extract Subject/From/To/Date from <b>Key: </b> pattern, body from elementToProof.
fn parse_html_email(html: &str) -> (String, String, String, String, String) {
    let subject = extract_html_header(html, "<b>Subject: </b>")
        .or_else(|| extract_html_header(html, "Subject:")).unwrap_or_default();
    let from = extract_html_header(html, "<b>From: </b>")
        .or_else(|| extract_html_header(html, "From:")).unwrap_or_default();
    let to = extract_html_header(html, "<b>To: </b>")
        .or_else(|| extract_html_header(html, "To:")).unwrap_or_default();
    let date = extract_html_header(html, "<b>Date: </b>")
        .or_else(|| extract_html_header(html, "Date:")).unwrap_or_default();

    // Cut off Teams meeting footer and quoted reply
    let stop_markers = ["me-email-text", "mail-editor-reference-message-container", "ms-outlook-mobile-reference-message"];
    let trimmed = stop_markers.iter().fold(html as &str, |s, marker| {
        if let Some(pos) = s.find(marker) {
            if let Some(tag_start) = s[..pos].rfind('<') { &s[..tag_start] } else { &s[..pos] }
        } else { s }
    });
    let body = strip_html(trimmed);
    (subject, from, to, date, body)
}

// ---------------------------------------------------------------------------
// Processed emails deduplication
// ---------------------------------------------------------------------------

/// Check if an email has already been processed by a specific rule.
/// Also verifies the created task still exists in projects DB.
pub fn is_email_processed(config: &Config, source_id: &str, rule_id: &str) -> bool {
    use rusqlite::OptionalExtension;
    let result = with_connection(config, |conn| {
        let task_id: Option<String> = conn.query_row(
            "SELECT task_id FROM processed_emails WHERE source_id=?1 AND rule_id=?2",
            params![source_id, rule_id],
            |row| row.get(0),
        ).optional()?;

        if let Some(task_id) = task_id {
            // Verify the task still exists in projects DB
            let projects_db = config.workspace_dir.join("projects").join("projects.db");
            if projects_db.exists() {
                let pconn = rusqlite::Connection::open(&projects_db)?;
                let exists: bool = pconn.query_row(
                    "SELECT COUNT(*) FROM project_tasks WHERE id=?1",
                    params![task_id],
                    |row| row.get::<_, i64>(0),
                ).unwrap_or(0) > 0;
                Ok(exists)
            } else {
                Ok(true) // projects DB not found, assume task exists
            }
        } else {
            Ok(false)
        }
    });
    result.unwrap_or(false)
}

/// Mark an email as processed by a rule, storing the created task_id.
pub fn mark_email_processed(config: &Config, source_id: &str, rule_id: &str, task_id: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    with_connection(config, |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO processed_emails (source_id, rule_id, task_id, processed_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![source_id, rule_id, task_id, now],
        )?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Batch queue
// ---------------------------------------------------------------------------

/// Enqueue an email for batch processing. Ignores duplicates (same source_id).
pub fn enqueue_batch_email(config: &Config, rule_id: &str, source_id: &str, email_body: &str) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    with_connection(config, |conn| {
        conn.execute(
            "INSERT OR IGNORE INTO email_batch_queue (id, rule_id, source_id, email_body, matched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, rule_id, source_id, email_body, now],
        )?;
        Ok(())
    })
}

/// Return all queued entries for a rule whose first matched_at is older than window_secs.
/// "Older than window" = the earliest matched_at for the rule is past the window.
pub fn pop_ready_batch_entries(config: &Config, rule_id: &str, window_secs: u64) -> Result<Vec<BatchQueueEntry>> {
    with_connection(config, |conn| {
        // Check if oldest entry for this rule has passed the window
        let oldest: Option<String> = conn.query_row(
            "SELECT MIN(matched_at) FROM email_batch_queue WHERE rule_id = ?1",
            params![rule_id],
            |row| row.get(0),
        ).ok().flatten();

        let Some(oldest_str) = oldest else { return Ok(vec![]); };

        let oldest_time = chrono::DateTime::parse_from_rfc3339(&oldest_str)
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let elapsed = chrono::Utc::now().signed_duration_since(oldest_time);
        if elapsed.num_seconds() < window_secs as i64 {
            return Ok(vec![]);
        }

        // Fetch all entries for this rule
        let mut stmt = conn.prepare(
            "SELECT id, rule_id, source_id, email_body, matched_at
             FROM email_batch_queue WHERE rule_id = ?1
             ORDER BY matched_at ASC",
        )?;
        let entries = stmt.query_map(params![rule_id], |row| {
            Ok(BatchQueueEntry {
                id: row.get(0)?,
                rule_id: row.get(1)?,
                source_id: row.get(2)?,
                email_body: row.get(3)?,
                matched_at: row.get(4)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(entries)
    })
}

/// Delete batch queue entries by their ids (after successful task creation).
pub fn delete_batch_entries(config: &Config, ids: &[String]) -> Result<()> {
    if ids.is_empty() { return Ok(()); }
    with_connection(config, |conn| {
        for id in ids {
            conn.execute("DELETE FROM email_batch_queue WHERE id = ?1", params![id])?;
        }
        Ok(())
    })
}

/// List all rule_ids that have at least one entry in the batch queue.
pub fn list_batch_rule_ids(config: &Config) -> Result<Vec<String>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT rule_id FROM email_batch_queue",
        )?;
        let ids = stmt.query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    })
}
