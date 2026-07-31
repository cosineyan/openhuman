use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::openhuman::config::Config;

use super::types::{
    KeywordLogic, TeamsConversation, TopicThread, TopicThreadDetail, UpdateTopicPatch,
};

fn with_connection<T>(config: &Config, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let db_path = config.workspace_dir.join("topic_threads").join("topics.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create topic_threads dir: {}", parent.display()))?;
    }
    let conn = Connection::open(&db_path)
        .with_context(|| format!("open topic_threads DB: {}", db_path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("set busy_timeout")?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("enable WAL")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS topic_threads (
            id             TEXT PRIMARY KEY,
            name           TEXT NOT NULL,
            description    TEXT NOT NULL DEFAULT '',
            keyword_logic  TEXT NOT NULL DEFAULT 'or',
            tree_id        TEXT NOT NULL,
            created_at_ms  INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS topic_keywords (
            topic_id  TEXT NOT NULL,
            keyword   TEXT NOT NULL,
            PRIMARY KEY (topic_id, keyword)
        );
        CREATE TABLE IF NOT EXISTS topic_source_pins (
            topic_id   TEXT NOT NULL,
            source_id  TEXT NOT NULL,
            PRIMARY KEY (topic_id, source_id)
        );
        CREATE TABLE IF NOT EXISTS topic_entity_pins (
            topic_id   TEXT NOT NULL,
            entity_id  TEXT NOT NULL,
            PRIMARY KEY (topic_id, entity_id)
        );
        CREATE TABLE IF NOT EXISTS topic_meeting_pins (
            topic_id      TEXT NOT NULL,
            meeting_name  TEXT NOT NULL,
            PRIMARY KEY (topic_id, meeting_name)
        );
        CREATE INDEX IF NOT EXISTS idx_topic_keywords_topic ON topic_keywords(topic_id);
        CREATE INDEX IF NOT EXISTS idx_topic_source_pins_topic ON topic_source_pins(topic_id);
        CREATE INDEX IF NOT EXISTS idx_topic_entity_pins_topic ON topic_entity_pins(topic_id);
        CREATE INDEX IF NOT EXISTS idx_topic_meeting_pins_topic ON topic_meeting_pins(topic_id);
        CREATE TABLE IF NOT EXISTS teams_conversations (
            conversation_id TEXT NOT NULL,
            source_id       TEXT NOT NULL,
            label           TEXT NOT NULL,
            chat_type       TEXT,
            last_seen_ms    INTEGER,
            PRIMARY KEY (source_id, conversation_id)
        );",
    )
    .context("migrate topic_threads DB")?;

    f(&conn)
}

fn load_list(conn: &Connection, table: &str, col: &str, topic_id: &str) -> Result<Vec<String>> {
    let sql = format!("SELECT {col} FROM {table} WHERE topic_id = ?1 ORDER BY {col} ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![topic_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn hydrate_detail(conn: &Connection, thread: TopicThread) -> Result<TopicThreadDetail> {
    let keywords = load_list(conn, "topic_keywords", "keyword", &thread.id)?;
    let source_pins = load_list(conn, "topic_source_pins", "source_id", &thread.id)?;
    let entity_pins = load_list(conn, "topic_entity_pins", "entity_id", &thread.id)?;
    let meeting_pins = load_list(conn, "topic_meeting_pins", "meeting_name", &thread.id)?;
    Ok(TopicThreadDetail {
        thread,
        keywords,
        source_pins,
        entity_pins,
        meeting_pins,
    })
}

fn row_to_thread(row: &rusqlite::Row<'_>) -> rusqlite::Result<TopicThread> {
    let logic_str: String = row.get(3)?;
    Ok(TopicThread {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        keyword_logic: KeywordLogic::parse(&logic_str),
        tree_id: row.get(4)?,
        created_at_ms: row.get(5)?,
    })
}

/// Insert a topic row + its keyword/source/entity/meeting sets.
#[allow(clippy::too_many_arguments)]
pub fn create_thread(
    config: &Config,
    id: &str,
    name: &str,
    description: &str,
    keyword_logic: KeywordLogic,
    tree_id: &str,
    created_at_ms: i64,
    keywords: &[String],
    source_ids: &[String],
    entity_ids: &[String],
    meeting_names: &[String],
) -> Result<()> {
    with_connection(config, |conn| {
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO topic_threads (id, name, description, keyword_logic, tree_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, name, description, keyword_logic.as_str(), tree_id, created_at_ms],
        )?;
        replace_sets_tx(
            &tx,
            id,
            Some(keywords),
            Some(source_ids),
            Some(entity_ids),
            Some(meeting_names),
        )?;
        tx.commit()?;
        Ok(())
    })
}

/// Replace the keyword/source/entity/meeting sets for a topic within a
/// transaction. A `None` argument leaves that set untouched.
fn replace_sets_tx(
    tx: &rusqlite::Transaction<'_>,
    topic_id: &str,
    keywords: Option<&[String]>,
    source_ids: Option<&[String]>,
    entity_ids: Option<&[String]>,
    meeting_names: Option<&[String]>,
) -> Result<()> {
    if let Some(keywords) = keywords {
        tx.execute(
            "DELETE FROM topic_keywords WHERE topic_id = ?1",
            params![topic_id],
        )?;
        for kw in keywords {
            let trimmed = kw.trim();
            if trimmed.is_empty() {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO topic_keywords (topic_id, keyword) VALUES (?1, ?2)",
                params![topic_id, trimmed],
            )?;
        }
    }
    if let Some(source_ids) = source_ids {
        tx.execute(
            "DELETE FROM topic_source_pins WHERE topic_id = ?1",
            params![topic_id],
        )?;
        for sid in source_ids {
            let trimmed = sid.trim();
            if trimmed.is_empty() {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO topic_source_pins (topic_id, source_id) VALUES (?1, ?2)",
                params![topic_id, trimmed],
            )?;
        }
    }
    if let Some(entity_ids) = entity_ids {
        tx.execute(
            "DELETE FROM topic_entity_pins WHERE topic_id = ?1",
            params![topic_id],
        )?;
        for eid in entity_ids {
            let trimmed = eid.trim();
            if trimmed.is_empty() {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO topic_entity_pins (topic_id, entity_id) VALUES (?1, ?2)",
                params![topic_id, trimmed],
            )?;
        }
    }
    if let Some(meeting_names) = meeting_names {
        tx.execute(
            "DELETE FROM topic_meeting_pins WHERE topic_id = ?1",
            params![topic_id],
        )?;
        for m in meeting_names {
            let trimmed = m.trim();
            if trimmed.is_empty() {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO topic_meeting_pins (topic_id, meeting_name) VALUES (?1, ?2)",
                params![topic_id, trimmed],
            )?;
        }
    }
    Ok(())
}

pub fn list_threads(config: &Config) -> Result<Vec<TopicThreadDetail>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, keyword_logic, tree_id, created_at_ms
               FROM topic_threads
              ORDER BY created_at_ms DESC",
        )?;
        let threads = stmt
            .query_map([], row_to_thread)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(threads.len());
        for t in threads {
            out.push(hydrate_detail(conn, t)?);
        }
        Ok(out)
    })
}

pub fn get_thread(config: &Config, id: &str) -> Result<Option<TopicThreadDetail>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, keyword_logic, tree_id, created_at_ms
               FROM topic_threads
              WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_thread)?;
        match rows.next() {
            Some(t) => Ok(Some(hydrate_detail(conn, t?)?)),
            None => Ok(None),
        }
    })
}

pub fn update_thread(config: &Config, id: &str, patch: &UpdateTopicPatch) -> Result<()> {
    with_connection(config, |conn| {
        let tx = conn.unchecked_transaction()?;
        if let Some(name) = &patch.name {
            tx.execute(
                "UPDATE topic_threads SET name = ?1 WHERE id = ?2",
                params![name, id],
            )?;
        }
        if let Some(description) = &patch.description {
            tx.execute(
                "UPDATE topic_threads SET description = ?1 WHERE id = ?2",
                params![description, id],
            )?;
        }
        if let Some(logic) = &patch.keyword_logic {
            tx.execute(
                "UPDATE topic_threads SET keyword_logic = ?1 WHERE id = ?2",
                params![logic.as_str(), id],
            )?;
        }
        replace_sets_tx(
            &tx,
            id,
            patch.keywords.as_deref(),
            patch.source_ids.as_deref(),
            patch.entity_ids.as_deref(),
            patch.meeting_names.as_deref(),
        )?;
        tx.commit()?;
        Ok(())
    })
}

/// Delete a topic and all its sets. Returns the backing `tree_id` (if the
/// topic existed) so the caller can decide whether to archive the tree.
pub fn delete_thread(config: &Config, id: &str) -> Result<Option<String>> {
    with_connection(config, |conn| {
        let tree_id: Option<String> = conn
            .query_row(
                "SELECT tree_id FROM topic_threads WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM topic_keywords WHERE topic_id = ?1",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM topic_source_pins WHERE topic_id = ?1",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM topic_entity_pins WHERE topic_id = ?1",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM topic_meeting_pins WHERE topic_id = ?1",
            params![id],
        )?;
        tx.execute("DELETE FROM topic_threads WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(tree_id)
    })
}

/// Record (or refresh) a Teams conversation's human-readable label, keyed by
/// (source_id, conversation_id). Called best-effort during Teams sync so the
/// discovery picker can list conversations by name. Idempotent.
pub fn upsert_conversation(
    config: &Config,
    source_id: &str,
    conversation_id: &str,
    label: &str,
    chat_type: Option<&str>,
    last_seen_ms: i64,
) -> Result<()> {
    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO teams_conversations
                (conversation_id, source_id, label, chat_type, last_seen_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source_id, conversation_id) DO UPDATE SET
                label = excluded.label,
                chat_type = excluded.chat_type,
                last_seen_ms = excluded.last_seen_ms",
            params![conversation_id, source_id, label, chat_type, last_seen_ms],
        )?;
        Ok(())
    })
}

/// List all recorded Teams conversations, most recently active first, for the
/// discovery picker.
pub fn list_conversations(config: &Config) -> Result<Vec<TeamsConversation>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT conversation_id, source_id, label, chat_type, last_seen_ms
               FROM teams_conversations
              ORDER BY last_seen_ms DESC, label ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let conversation_id: String = row.get(0)?;
                let source_id: String = row.get(1)?;
                Ok(TeamsConversation {
                    // pin_value is what a topic source_pin stores: matching the
                    // chunk composite id `mem_src:{source_id}:{conversation_id}::…`
                    // via the existing `mem_src:{pin}:` prefix rule.
                    pin_value: format!("{source_id}:{conversation_id}"),
                    conversation_id,
                    source_id,
                    label: row.get(2)?,
                    chat_type: row.get(3)?,
                    last_seen_ms: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}
