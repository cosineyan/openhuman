use anyhow::Result;
use uuid::Uuid;

use crate::openhuman::config::Config;
use crate::openhuman::memory_store::chunks::types::Chunk;
use crate::openhuman::memory_store::content::read as content_read;
use crate::openhuman::memory_tree::score::store as score_store;
use crate::rpc::RpcOutcome;

use super::store;
use super::types::{
    BackfillResult, CreateTopicInput, KeywordLogic, MeetingInfo, PersonEntity, TeamsConversation,
    TopicThreadDetail, TopicTimelineNode, UpdateTopicPatch,
};

/// Create a topic thread: mint an id, get-or-create the backing topic tree,
/// then persist the topic + its matching dimensions. If `backfill_days` is set,
/// runs a historical backfill right after creating.
pub async fn create_thread_rpc(
    config: &Config,
    input: CreateTopicInput,
) -> Result<RpcOutcome<TopicThreadDetail>, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("topic name is required".to_string());
    }
    let topic_id = Uuid::new_v4().to_string();
    let scope = format!("topic:{topic_id}");
    let created_at_ms = chrono::Utc::now().timestamp_millis();

    // Create the backing topic tree so AppendBuffer(Topic) jobs have a target.
    let tree = crate::openhuman::memory_tree::tree::TreeFactory::topic(scope.clone())
        .get_or_create(config)
        .map_err(|e| format!("create topic tree: {e}"))?;

    log::info!(
        "[topic_threads] create topic id={topic_id} name={name:?} tree_id={} keywords={} sources={} entities={} meetings={} backfill_days={:?}",
        tree.id,
        input.keywords.len(),
        input.source_ids.len(),
        input.entity_ids.len(),
        input.meeting_names.len(),
        input.backfill_days,
    );

    store::create_thread(
        config,
        &topic_id,
        name,
        input.description.trim(),
        input.keyword_logic,
        &tree.id,
        created_at_ms,
        &input.keywords,
        &input.source_ids,
        &input.entity_ids,
        &input.meeting_names,
    )
    .map_err(|e| format!("persist topic: {e}"))?;

    // Optional historical backfill right after creation.
    if let Some(days) = input.backfill_days {
        if days > 0 {
            if let Err(e) = backfill_topic(config, &topic_id, days).await {
                log::warn!("[topic_threads] initial backfill failed topic={topic_id}: {e}");
            }
        }
    }

    let detail = store::get_thread(config, &topic_id)
        .map_err(|e| format!("reload topic: {e}"))?
        .ok_or_else(|| "topic vanished after create".to_string())?;
    Ok(RpcOutcome::single_log(
        detail,
        format!("created topic {topic_id}"),
    ))
}

pub fn list_threads_rpc(config: &Config) -> Result<RpcOutcome<Vec<TopicThreadDetail>>, String> {
    let threads = store::list_threads(config).map_err(|e| format!("list topics: {e}"))?;
    Ok(RpcOutcome::single_log(
        threads,
        "listed topic threads".to_string(),
    ))
}

pub fn get_thread_rpc(
    config: &Config,
    id: &str,
) -> Result<RpcOutcome<Option<TopicThreadDetail>>, String> {
    let thread = store::get_thread(config, id).map_err(|e| format!("get topic: {e}"))?;
    Ok(RpcOutcome::single_log(thread, format!("got topic {id}")))
}

pub fn update_thread_rpc(
    config: &Config,
    id: &str,
    patch: UpdateTopicPatch,
) -> Result<RpcOutcome<TopicThreadDetail>, String> {
    store::update_thread(config, id, &patch).map_err(|e| format!("update topic: {e}"))?;
    let detail = store::get_thread(config, id)
        .map_err(|e| format!("reload topic: {e}"))?
        .ok_or_else(|| format!("topic {id} not found"))?;
    Ok(RpcOutcome::single_log(
        detail,
        format!("updated topic {id}"),
    ))
}

pub fn delete_thread_rpc(config: &Config, id: &str) -> Result<RpcOutcome<()>, String> {
    let tree_id = store::delete_thread(config, id).map_err(|e| format!("delete topic: {e}"))?;
    log::info!("[topic_threads] deleted topic id={id} backing_tree={tree_id:?}");
    // The backing tree + its summaries are intentionally left in place — a
    // future "wipe topic data" action can archive/delete them explicitly.
    Ok(RpcOutcome::single_log((), format!("deleted topic {id}")))
}

/// Build a topic's timeline: every summary node in the backing tree, newest
/// (and highest-level) first, with the full body hydrated from disk.
pub fn timeline_rpc(
    config: &Config,
    topic_id: &str,
) -> Result<RpcOutcome<Vec<TopicTimelineNode>>, String> {
    let detail = store::get_thread(config, topic_id)
        .map_err(|e| format!("get topic: {e}"))?
        .ok_or_else(|| format!("topic {topic_id} not found"))?;

    let summaries = crate::openhuman::memory_store::trees::store::list_summaries_by_tree(
        config,
        &detail.thread.tree_id,
    )
    .map_err(|e| format!("list summaries: {e}"))?;

    let mut nodes = Vec::with_capacity(summaries.len());
    for s in summaries {
        let body =
            match crate::openhuman::memory_store::content::read::read_summary_body(config, &s.id) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!(
                        "[topic_threads] timeline: failed to read summary body id={} err={e}",
                        s.id
                    );
                    s.content.clone() // fall back to the preview
                }
            };
        nodes.push(TopicTimelineNode {
            summary_id: s.id,
            level: s.level,
            time_range_start_ms: s.time_range_start.timestamp_millis(),
            time_range_end_ms: s.time_range_end.timestamp_millis(),
            body,
        });
    }
    Ok(RpcOutcome::single_log(
        nodes,
        format!("topic {topic_id} timeline"),
    ))
}

/// Discovery: list Teams conversations (1:1 + group chats) recorded during
/// sync, for the pin picker. Each carries a human-readable `label` and a
/// `pin_value` ready to store as a topic source_pin.
pub fn discover_conversations_rpc(
    config: &Config,
) -> Result<RpcOutcome<Vec<TeamsConversation>>, String> {
    let convos =
        store::list_conversations(config).map_err(|e| format!("list conversations: {e}"))?;
    let n = convos.len();
    Ok(RpcOutcome::single_log(
        convos,
        format!("discovered {n} teams conversations"),
    ))
}

/// Discovery: list person + email entities (for the people picker), ranked by
/// mention count. Delegates to the existing `top_entities` query — no new SQL.
pub async fn discover_people_rpc(
    config: &Config,
    limit: u32,
) -> Result<RpcOutcome<Vec<PersonEntity>>, String> {
    use crate::openhuman::memory::read_rpc::top_entities_rpc;

    let people = top_entities_rpc(config, Some("person".to_string()), limit).await?;
    let emails = top_entities_rpc(config, Some("email".to_string()), limit).await?;

    let mut out: Vec<PersonEntity> = people
        .value
        .into_iter()
        .chain(emails.value)
        .map(|e| PersonEntity {
            entity_id: e.entity_id,
            surface: e.surface,
            kind: e.kind,
            count: e.count as u64,
        })
        .collect();
    // Highest mention count first so the most-relevant people surface at the top.
    out.sort_by(|a, b| b.count.cmp(&a.count));
    let n = out.len();
    Ok(RpcOutcome::single_log(
        out,
        format!("discovered {n} people/email entities"),
    ))
}

/// Parse a Teams chat conversation id (`19:...@thread.v2` or
/// `19:...@unq.gbl.spaces`) out of a pasted Teams deep link. Handles both the
/// `/l/chat/<id>/...` share form and URL-encoded ids.
pub fn parse_teams_chat_link(url: &str) -> Option<String> {
    // The conversation id always starts with `19:` and ends at `@thread.v2`
    // or `@unq.gbl.spaces`. Decode the two escape sequences that show up in
    // Teams links (`%3A` = ':', `%40` = '@') before scanning.
    let decoded = url
        .replace("%3A", ":")
        .replace("%3a", ":")
        .replace("%40", "@");
    let start = decoded.find("19:")?;
    let tail = &decoded[start..];
    for suffix in ["@thread.v2", "@unq.gbl.spaces"] {
        if let Some(end) = tail.find(suffix) {
            return Some(tail[..end + suffix.len()].to_string());
        }
    }
    None
}

/// Resolve a pasted Teams chat link into a ready-to-store conversation pin:
/// parse the conversation id, find the Teams memory source, fetch the chat's
/// real label from Graph, record it in `teams_conversations`, and return the
/// `TeamsConversation` (its `pin_value` is what the UI stores as a source pin).
pub async fn resolve_chat_link_rpc(
    config: &Config,
    url: &str,
) -> Result<RpcOutcome<TeamsConversation>, String> {
    let conversation_id = parse_teams_chat_link(url)
        .ok_or_else(|| "Could not find a Teams conversation id in that link.".to_string())?;

    // Find the Teams messages memory source to key the pin under.
    let sources = crate::openhuman::memory_sources::registry::list_enabled_by_kind(
        crate::openhuman::memory_sources::types::SourceKind::TeamsMessages,
    )
    .await
    .map_err(|e| format!("list Teams sources: {e}"))?;
    let source = sources.into_iter().next().ok_or_else(|| {
        "No Teams Messages source is connected. Connect Teams in SAP Systems first.".to_string()
    })?;

    // Fetch the real label from Graph (best-effort — fall back to the id).
    let (label, chat_type) =
        match crate::openhuman::memory_sources::readers::m365::resolve_chat_label(
            config,
            &conversation_id,
        )
        .await
        {
            Ok((label, ct)) => (label, Some(ct)),
            Err(e) => {
                log::warn!("[topic_threads] resolve_chat_label failed for {conversation_id}: {e}");
                (conversation_id.clone(), None)
            }
        };

    let now_ms = chrono::Utc::now().timestamp_millis();
    store::upsert_conversation(
        config,
        &source.id,
        &conversation_id,
        &label,
        chat_type.as_deref(),
        now_ms,
    )
    .map_err(|e| format!("record conversation: {e}"))?;

    let convo = TeamsConversation {
        pin_value: format!("{}:{}", source.id, conversation_id),
        conversation_id,
        source_id: source.id,
        label: label.clone(),
        chat_type,
        last_seen_ms: Some(now_ms),
    };
    Ok(RpcOutcome::single_log(
        convo,
        format!("resolved teams chat link -> {label}"),
    ))
}

/// Discovery: list distinct meeting names parsed from transcript chunks, ranked
/// by occurrence, for the meeting picker.
pub fn discover_meetings_rpc(config: &Config) -> Result<RpcOutcome<Vec<MeetingInfo>>, String> {
    use crate::openhuman::memory_store::chunks::store::{list_chunks, ListChunksQuery};
    use crate::openhuman::memory_store::chunks::types::SourceKind;
    use std::collections::HashMap;

    let query = ListChunksQuery {
        source_kind: Some(SourceKind::Transcript),
        source_id: None,
        owner: None,
        since_ms: None,
        until_ms: None,
        limit: Some(10_000),
        source_scope: None,
        exclude_dropped: true,
    };
    let chunks = list_chunks(config, &query).map_err(|e| format!("list transcripts: {e}"))?;

    // (count, last_seen_ms) keyed by meeting name.
    let mut agg: HashMap<String, (u64, i64)> = HashMap::new();
    for c in &chunks {
        let lower = c.content.to_lowercase();
        if let Some(name_lower) = extract_meeting_name(&lower) {
            // Recover the original-case name from the raw content at the same span.
            let name = extract_meeting_name_raw(&c.content).unwrap_or(name_lower);
            let ts = c.metadata.timestamp.timestamp_millis();
            let e = agg.entry(name).or_insert((0, ts));
            e.0 += 1;
            if ts > e.1 {
                e.1 = ts;
            }
        }
    }
    let mut out: Vec<MeetingInfo> = agg
        .into_iter()
        .map(|(meeting_name, (count, last))| MeetingInfo {
            meeting_name,
            count,
            last_seen_ms: Some(last),
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count));
    let n = out.len();
    Ok(RpcOutcome::single_log(
        out,
        format!("discovered {n} meetings"),
    ))
}

/// Original-case variant of `extract_meeting_name` for display.
fn extract_meeting_name_raw(body: &str) -> Option<String> {
    let lower = body.to_lowercase();
    let start = lower.find("[meeting:")? + "[meeting:".len();
    let rest = &body[start..];
    let end = rest.find(']')?;
    Some(rest[..end].trim().to_string())
}

/// Backfill RPC wrapper.
pub async fn backfill_topic_rpc(
    config: &Config,
    topic_id: &str,
    days: u32,
) -> Result<RpcOutcome<BackfillResult>, String> {
    let result = backfill_topic(config, topic_id, days).await?;
    Ok(RpcOutcome::single_log(
        result.clone(),
        format!(
            "backfilled topic {topic_id} ({} days): scanned={} matched={} enqueued={}",
            days, result.scanned, result.matched, result.enqueued
        ),
    ))
}

/// Scan historical chunks in the last `days` and route matches into the topic
/// tree. Pages through `list_chunks` with a `until_ms` cursor because a single
/// window can exceed `MAX_LIST_LIMIT`. Reuses the exact same `chunk_matches_topic`
/// matcher as the live ingest path, so backfill and real-time stay consistent.
async fn backfill_topic(
    config: &Config,
    topic_id: &str,
    days: u32,
) -> Result<BackfillResult, String> {
    use crate::openhuman::memory_store::chunks::store::{list_chunks, ListChunksQuery};

    let detail = store::get_thread(config, topic_id)
        .map_err(|e| format!("get topic: {e}"))?
        .ok_or_else(|| format!("topic {topic_id} not found"))?;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let since_ms = now_ms - (days as i64) * 86_400_000;
    const PAGE: usize = 2000;

    let mut scanned: u64 = 0;
    let mut matched: u64 = 0;
    let mut enqueued: u64 = 0;
    let mut cursor_until: Option<i64> = None;

    loop {
        let query = ListChunksQuery {
            source_kind: None,
            source_id: None,
            owner: None,
            since_ms: Some(since_ms),
            until_ms: cursor_until,
            limit: Some(PAGE),
            source_scope: None,
            exclude_dropped: true,
        };
        let chunks = list_chunks(config, &query).map_err(|e| format!("list_chunks: {e}"))?;
        if chunks.is_empty() {
            break;
        }
        let page_len = chunks.len();
        // Rows come back timestamp_ms DESC; the last one is the oldest in this page.
        let oldest = chunks
            .last()
            .map(|c| c.metadata.timestamp.timestamp_millis())
            .unwrap_or(since_ms);

        for chunk in &chunks {
            scanned += 1;
            let entity_ids =
                score_store::list_entity_ids_for_node(config, &chunk.id).unwrap_or_default();
            let body = content_read::read_chunk_body(config, &chunk.id)
                .unwrap_or_else(|_| chunk.content.clone());
            let body_lower = body.to_lowercase();
            if !chunk_matches_topic(&detail, chunk, &entity_ids, &body_lower) {
                continue;
            }
            matched += 1;
            // Append directly to the topic tree's L0 buffer (synchronous, no job
            // queue). `append_to_buffer` dedups by item id, so re-running backfill
            // is safe. We force-seal after the scan so the timeline populates
            // immediately even below the token threshold.
            match crate::openhuman::memory_tree::tree::bucket_seal::append_to_buffer(
                config,
                &detail.thread.tree_id,
                0,
                &chunk.id,
                chunk.token_count as i64,
                chunk.metadata.timestamp,
            ) {
                Ok(()) => enqueued += 1,
                Err(e) => log::warn!("[topic_threads] backfill append_to_buffer failed: {e}"),
            }
        }

        if page_len < PAGE {
            break;
        }
        // Next page: strictly older than the oldest row we just saw. Subtract 1ms
        // to avoid re-fetching the boundary row (dedupe would catch it anyway).
        cursor_until = Some(oldest - 1);
        if cursor_until.unwrap() < since_ms {
            break;
        }
    }

    // Force-seal the topic tree so the timeline populates immediately, even
    // when the backfilled content is below the normal token threshold. Without
    // this the buffer would wait for the 50k-token gate or the 7-day stale
    // flush. Best-effort — a flush failure still leaves the chunks buffered for
    // the next natural seal.
    if enqueued > 0 {
        let strategy =
            crate::openhuman::memory_tree::tree::TreeFactory::topic(format!("topic:{topic_id}"))
                .label_strategy(config);
        match crate::openhuman::memory_tree::tree::flush::force_flush_tree(
            config,
            &detail.thread.tree_id,
            Some(chrono::Utc::now()),
            &strategy,
        )
        .await
        {
            Ok(sealed) => log::info!(
                "[topic_threads] backfill force-flush topic={topic_id} seals_fired={}",
                sealed.len()
            ),
            Err(e) => {
                log::warn!("[topic_threads] backfill force-flush failed topic={topic_id}: {e}")
            }
        }
    }

    log::info!(
        "[topic_threads] backfill topic={topic_id} days={days} scanned={scanned} matched={matched} enqueued={enqueued}"
    );
    Ok(BackfillResult {
        scanned,
        matched,
        enqueued,
    })
}

/// For an admitted chunk, check every topic's matching rules and enqueue an
/// `AppendBuffer(Topic)` job for each match. Best-effort and synchronous:
/// callers ignore the error so a matcher failure never fails the extract job.
pub fn maybe_link_chunk_to_topics(
    config: &Config,
    chunk: &Chunk,
    entity_ids: &[String],
    body: &str,
) -> Result<()> {
    let threads = store::list_threads(config)?;
    if threads.is_empty() {
        return Ok(());
    }

    let body_lower = body.to_lowercase();

    for thread in &threads {
        if !chunk_matches_topic(thread, chunk, entity_ids, &body_lower) {
            continue;
        }
        let payload = crate::openhuman::memory_queue::types::AppendBufferPayload {
            node: crate::openhuman::memory_queue::types::NodeRef::Leaf {
                chunk_id: chunk.id.clone(),
            },
            target: crate::openhuman::memory_queue::types::AppendTarget::Topic {
                tree_id: thread.thread.tree_id.clone(),
            },
        };
        let job = crate::openhuman::memory_queue::types::NewJob::append_buffer(&payload)?;
        match crate::openhuman::memory_queue::store::enqueue(config, &job) {
            Ok(Some(_)) => {
                log::info!(
                    "[topic_threads] linked chunk {} → topic {} (tree {})",
                    &chunk.id[..chunk.id.len().min(16)],
                    thread.thread.id,
                    thread.thread.tree_id,
                );
            }
            Ok(None) => { /* already queued — dedupe key suppressed it */ }
            Err(e) => {
                log::warn!(
                    "[topic_threads] enqueue AppendBuffer(Topic) failed chunk={} topic={}: {e}",
                    &chunk.id[..chunk.id.len().min(16)],
                    thread.thread.id,
                );
            }
        }
    }
    Ok(())
}

/// Pure matcher: OR across the three dimensions (pinned source, pinned entity,
/// keyword). `body_lower` must already be lowercased.
fn chunk_matches_topic(
    thread: &TopicThreadDetail,
    chunk: &Chunk,
    entity_ids: &[String],
    body_lower: &str,
) -> bool {
    // 1. Pinned source — cheapest, no content analysis. Memory-source chunks
    //    carry a per-item composite source_id of the form
    //    `mem_src:{source_id}:{item_id}`, so a pinned base source id matches
    //    every item under it via the `mem_src:{pin}:` prefix. We also accept an
    //    exact match so callers can pin a fully-qualified id directly.
    let chunk_source = chunk.metadata.source_id.as_str();
    if thread.source_pins.iter().any(|pin| {
        chunk_source == pin
            || chunk_source.starts_with(&format!("mem_src:{pin}:"))
            || chunk_source == format!("mem_src:{pin}")
    }) {
        return true;
    }
    // 2. Pinned entity — any overlap between the chunk's entities and the pins.
    if !thread.entity_pins.is_empty()
        && entity_ids
            .iter()
            .any(|eid| thread.entity_pins.contains(eid))
    {
        return true;
    }
    // 3. Keyword match (OR / AND) over the body.
    if !thread.keywords.is_empty() {
        let matcher = |kw: &String| {
            let k = kw.trim().to_lowercase();
            !k.is_empty() && body_lower.contains(&k)
        };
        match thread.thread.keyword_logic {
            KeywordLogic::Or => {
                if thread.keywords.iter().any(matcher) {
                    return true;
                }
            }
            KeywordLogic::And => {
                if thread.keywords.iter().all(matcher) {
                    return true;
                }
            }
        }
    }
    // 4. Meeting-name pin — for transcripts, the body prefix carries
    //    `[Meeting: Name]`. A pin matches when the meeting name *contains* the
    //    pinned substring, so pinning "RGM CALM" covers every instance of a
    //    recurring series ("RGM CALM Discussion", "Weekly RGM CALM Sync", …).
    if !thread.meeting_pins.is_empty() {
        if let Some(meeting) = extract_meeting_name(body_lower) {
            if thread.meeting_pins.iter().any(|pin| {
                let p = pin.trim().to_lowercase();
                !p.is_empty() && meeting.contains(&p)
            }) {
                return true;
            }
        }
    }
    false
}

/// Extract the lowercased meeting name from a transcript body's `[meeting: X]`
/// prefix. `body_lower` must already be lowercased. Returns None for
/// non-transcript content.
fn extract_meeting_name(body_lower: &str) -> Option<String> {
    let start = body_lower.find("[meeting:")? + "[meeting:".len();
    let rest = &body_lower[start..];
    let end = rest.find(']')?;
    Some(rest[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::memory_store::chunks::types::{Metadata, SourceKind};
    use crate::openhuman::topic_threads::types::TopicThread;
    use chrono::TimeZone;
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> Config {
        let mut config = Config::default();
        config.workspace_dir = tmp.path().join("workspace");
        std::fs::create_dir_all(&config.workspace_dir).unwrap();
        config
    }

    fn chunk_with(source_id: &str, body: &str) -> Chunk {
        let ts = chrono::Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        Chunk {
            id: "test_chunk_id_0000000000000000".into(),
            content: body.into(),
            metadata: Metadata {
                source_kind: SourceKind::Document,
                source_id: source_id.into(),
                owner: "user".into(),
                timestamp: ts,
                time_range: (ts, ts),
                tags: vec![],
                source_ref: None,
                path_scope: None,
            },
            token_count: 10,
            seq_in_source: 0,
            created_at: ts,
            partial_message: false,
        }
    }

    fn detail(
        logic: KeywordLogic,
        keywords: &[&str],
        sources: &[&str],
        entities: &[&str],
    ) -> TopicThreadDetail {
        detail_full(logic, keywords, sources, entities, &[])
    }

    fn detail_full(
        logic: KeywordLogic,
        keywords: &[&str],
        sources: &[&str],
        entities: &[&str],
        meetings: &[&str],
    ) -> TopicThreadDetail {
        TopicThreadDetail {
            thread: TopicThread {
                id: "t1".into(),
                name: "Test".into(),
                description: String::new(),
                keyword_logic: logic,
                tree_id: "topic:tree1".into(),
                created_at_ms: 0,
            },
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
            source_pins: sources.iter().map(|s| s.to_string()).collect(),
            entity_pins: entities.iter().map(|s| s.to_string()).collect(),
            meeting_pins: meetings.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn meeting_pin_substring_matches_transcript() {
        // Pin a series substring; transcript body carries the full meeting name.
        let t = detail_full(KeywordLogic::Or, &[], &[], &[], &["RGM CALM"]);
        let transcript = chunk_with(
            "mem_src:src_x:event:abc||RGM CALM Discussion||2026-06-18",
            "[Meeting: RGM CALM Discussion] [Date: 2026-06-18] [Participants: A, B]\nhello",
        );
        let body = transcript.content.to_lowercase();
        assert!(chunk_matches_topic(&t, &transcript, &[], &body));

        // A different meeting must not match.
        let other = chunk_with(
            "mem_src:src_x:event:def||Weekly Sales Sync||2026-06-18",
            "[Meeting: Weekly Sales Sync] [Date: 2026-06-18]\nhi",
        );
        assert!(!chunk_matches_topic(
            &t,
            &other,
            &[],
            &other.content.to_lowercase()
        ));
    }

    #[test]
    fn extract_meeting_name_parses_prefix() {
        assert_eq!(
            extract_meeting_name("[meeting: rgm calm discussion] [date: x]\nbody"),
            Some("rgm calm discussion".to_string())
        );
        assert_eq!(extract_meeting_name("no meeting prefix here"), None);
    }

    #[test]
    fn keyword_or_matches_any() {
        let t = detail(
            KeywordLogic::Or,
            &["leave approval", "annual leave"],
            &[],
            &[],
        );
        let c = chunk_with("mem_src:x:1", "");
        let body = "please handle my leave approval today".to_lowercase();
        assert!(chunk_matches_topic(&t, &c, &[], &body));
    }

    #[test]
    fn keyword_or_no_match() {
        let t = detail(KeywordLogic::Or, &["leave approval"], &[], &[]);
        let c = chunk_with("mem_src:x:1", "");
        let body = "unrelated content about lunch".to_lowercase();
        assert!(!chunk_matches_topic(&t, &c, &[], &body));
    }

    #[test]
    fn keyword_and_requires_all() {
        let t = detail(KeywordLogic::And, &["leave", "approval"], &[], &[]);
        let c = chunk_with("mem_src:x:1", "");
        let both = "leave request needs approval".to_lowercase();
        let one = "leave request pending".to_lowercase();
        assert!(chunk_matches_topic(&t, &c, &[], &both));
        assert!(!chunk_matches_topic(&t, &c, &[], &one));
    }

    #[test]
    fn source_pin_matches_via_composite_prefix() {
        // Pinned base source id matches every per-item chunk under it.
        let t = detail(KeywordLogic::Or, &[], &["teams-hr"], &[]);
        let c = chunk_with("mem_src:teams-hr:msg-42", "no keywords here");
        assert!(chunk_matches_topic(&t, &c, &[], "no keywords here"));
    }

    #[test]
    fn source_pin_exact_match() {
        let t = detail(KeywordLogic::Or, &[], &["mem_src:teams-hr:msg-42"], &[]);
        let c = chunk_with("mem_src:teams-hr:msg-42", "");
        assert!(chunk_matches_topic(&t, &c, &[], ""));
    }

    #[test]
    fn source_pin_no_false_positive() {
        let t = detail(KeywordLogic::Or, &[], &["teams-hr"], &[]);
        // Different source — must not match on prefix.
        let c = chunk_with("mem_src:teams-eng:msg-1", "");
        assert!(!chunk_matches_topic(&t, &c, &[], ""));
    }

    #[test]
    fn conversation_pin_matches_teams_composite_id() {
        // A pinned single Teams conversation is stored as
        // `{source_id}:{conversation_id}`. Real chunk ids look like
        // `mem_src:{source_id}:{conversation_id}::{msg_id}`.
        let pin = "src_f80bc:19:abc@unq.gbl.spaces";
        let t = detail(KeywordLogic::Or, &[], &[pin], &[]);
        let c = chunk_with("mem_src:src_f80bc:19:abc@unq.gbl.spaces::1781093634593", "");
        assert!(chunk_matches_topic(&t, &c, &[], ""));

        // A different conversation under the same account must NOT match.
        let other = chunk_with(
            "mem_src:src_f80bc:19:different@unq.gbl.spaces::1781093634593",
            "",
        );
        assert!(!chunk_matches_topic(&t, &other, &[], ""));
    }

    #[test]
    fn parse_teams_chat_link_variants() {
        // Share link with @thread.v2, URL-encoded context suffix.
        let url = "https://teams.microsoft.com/l/chat/19:0d189700db5a4ccfaf33f88b589a9257@thread.v2/conversations?context=%7B%22contextType%22%3A%22chat%22%7D";
        assert_eq!(
            parse_teams_chat_link(url).as_deref(),
            Some("19:0d189700db5a4ccfaf33f88b589a9257@thread.v2")
        );

        // URL-encoded colon/at form.
        let enc = "https://teams.microsoft.com/l/chat/19%3Aabc123%40unq.gbl.spaces/x";
        assert_eq!(
            parse_teams_chat_link(enc).as_deref(),
            Some("19:abc123@unq.gbl.spaces")
        );

        // No conversation id → None.
        assert_eq!(parse_teams_chat_link("https://example.com/foo"), None);
    }

    #[test]
    fn entity_pin_matches_overlap() {
        let t = detail(KeywordLogic::Or, &[], &[], &["person:hr-manager"]);
        let c = chunk_with("mem_src:x:1", "");
        let entities = vec![
            "email:foo@bar.com".to_string(),
            "person:hr-manager".to_string(),
        ];
        assert!(chunk_matches_topic(&t, &c, &entities, ""));
    }

    #[test]
    fn entity_pin_no_overlap() {
        let t = detail(KeywordLogic::Or, &[], &[], &["person:hr-manager"]);
        let c = chunk_with("mem_src:x:1", "");
        let entities = vec!["person:someone-else".to_string()];
        assert!(!chunk_matches_topic(&t, &c, &entities, ""));
    }

    #[test]
    fn store_crud_round_trip() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        store::create_thread(
            &config,
            "topic-1",
            "Leave",
            "desc",
            KeywordLogic::Or,
            "topic:tree-1",
            123,
            &["leave approval".into()],
            &["teams-hr".into()],
            &["person:hr".into()],
            &["Weekly HR Sync".into()],
        )
        .unwrap();

        let got = store::get_thread(&config, "topic-1").unwrap().unwrap();
        assert_eq!(got.thread.name, "Leave");
        assert_eq!(got.thread.keyword_logic, KeywordLogic::Or);
        assert_eq!(got.keywords, vec!["leave approval".to_string()]);
        assert_eq!(got.source_pins, vec!["teams-hr".to_string()]);
        assert_eq!(got.entity_pins, vec!["person:hr".to_string()]);
        assert_eq!(got.meeting_pins, vec!["Weekly HR Sync".to_string()]);

        // Update replaces list sets and scalar fields.
        let patch = UpdateTopicPatch {
            name: Some("Renamed".into()),
            keyword_logic: Some(KeywordLogic::And),
            keywords: Some(vec!["a".into(), "b".into()]),
            ..Default::default()
        };
        store::update_thread(&config, "topic-1", &patch).unwrap();
        let got = store::get_thread(&config, "topic-1").unwrap().unwrap();
        assert_eq!(got.thread.name, "Renamed");
        assert_eq!(got.thread.keyword_logic, KeywordLogic::And);
        assert_eq!(got.keywords, vec!["a".to_string(), "b".to_string()]);
        // Untouched sets survive.
        assert_eq!(got.source_pins, vec!["teams-hr".to_string()]);

        // Delete returns the backing tree id and clears the row.
        let tree_id = store::delete_thread(&config, "topic-1").unwrap();
        assert_eq!(tree_id.as_deref(), Some("topic:tree-1"));
        assert!(store::get_thread(&config, "topic-1").unwrap().is_none());
    }
}
