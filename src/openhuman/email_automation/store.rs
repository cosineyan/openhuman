use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::openhuman::config::Config;

use super::types::{CreateRuleInput, EmailAutomationRule, RulePatch};

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
            created_at                TEXT NOT NULL,
            updated_at                TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ear_enabled
            ON email_automation_rules(enabled);",
    )
    .context("migrate email_automation DB")?;

    // Migrate existing DBs that don't have parse_script column
    let _ = conn.execute("ALTER TABLE email_automation_rules ADD COLUMN parse_script TEXT", []);

    f(&conn)
}

fn row_to_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmailAutomationRule> {
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
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

pub fn list_rules(config: &Config) -> Result<Vec<EmailAutomationRule>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, enabled, sender_contains, subject_contains, body_contains,
                    task_title_template, task_description_template, assignee, bucket_id,
                    llm_fallback_enabled, parse_script, created_at, updated_at
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
                    llm_fallback_enabled, parse_script, created_at, updated_at
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
                    llm_fallback_enabled, parse_script, created_at, updated_at
             FROM email_automation_rules WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_rule)?;
        Ok(rows.next().transpose()?)
    })
}

pub fn create_rule(config: &Config, input: CreateRuleInput) -> Result<EmailAutomationRule> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO email_automation_rules
             (id, name, enabled, sender_contains, subject_contains, body_contains,
              task_title_template, task_description_template, assignee, bucket_id,
              llm_fallback_enabled, parse_script, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
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
