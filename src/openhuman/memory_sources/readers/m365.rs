//! Memory source readers for Microsoft 365 data — Outlook mail, calendar, and Teams messages.
//!
//! All three readers call the Microsoft Graph API directly using the cached
//! graph access token from the m365 token file written by the bundled m365-cli.

use async_trait::async_trait;
use chrono::Utc;

use crate::openhuman::config::Config;
use crate::openhuman::memory_sources::types::{
    ContentType, SourceContent, SourceItem, SourceKind,
};

use super::{MemorySourceEntry, SourceReader};

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";
const DEFAULT_SYNC_DAYS: u32 = 30;
const DEFAULT_MAX_ITEMS: u32 = 50;

// ---------------------------------------------------------------------------
// Token helper
// ---------------------------------------------------------------------------

fn read_graph_token(config: &Config) -> Result<String, String> {
    let path = config.workspace_dir.join("m365").join("tokens.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read m365 token file: {e}"))?;
    let tokens: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("cannot parse m365 token file: {e}"))?;
    let entry = tokens
        .get("graph")
        .ok_or("graph token not found — please connect Outlook in SAP Systems")?;
    let exp = entry
        .get("expiresOn")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if exp > 0 && exp < Utc::now().timestamp() {
        return Err(
            "graph token expired — please click Refresh in SAP Systems to renew it".into(),
        );
    }
    entry
        .get("token")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "graph token value is missing".into())
}

// ---------------------------------------------------------------------------
// Graph API helper
// ---------------------------------------------------------------------------

async fn graph_get(token: &str, url: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("build http client: {e}"))?;
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| format!("graph request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("graph API returned {status}: {body}"));
    }
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("parse graph response: {e}"))
}

fn since_date(days: u32) -> String {
    let dt = Utc::now() - chrono::Duration::days(days as i64);
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// ---------------------------------------------------------------------------
// OutlookMailReader
// ---------------------------------------------------------------------------

pub struct OutlookMailReader;

#[async_trait]
impl SourceReader for OutlookMailReader {
    fn kind(&self) -> SourceKind {
        SourceKind::OutlookMail
    }

    async fn list_items(
        &self,
        source: &MemorySourceEntry,
        config: &Config,
    ) -> Result<Vec<SourceItem>, String> {
        let token = read_graph_token(config)?;
        let days = source.m365_sync_days.unwrap_or(DEFAULT_SYNC_DAYS);
        let top = source.m365_max_items.unwrap_or(DEFAULT_MAX_ITEMS);
        let since = since_date(days);
        let url = format!(
            "{GRAPH_BASE}/me/messages?$top={top}&$filter=receivedDateTime ge {since}\
             &$select=id,subject,receivedDateTime,from&$orderby=receivedDateTime desc"
        );
        let data = graph_get(&token, &url).await?;
        let items = data
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or("unexpected graph response shape")?
            .iter()
            .map(|m| {
                let id = m["id"].as_str().unwrap_or("").to_string();
                let subject = m["subject"].as_str().unwrap_or("(no subject)").to_string();
                let received = m["receivedDateTime"].as_str().unwrap_or("").to_string();
                let updated_at_ms = chrono::DateTime::parse_from_rfc3339(&received)
                    .ok()
                    .map(|dt| dt.timestamp_millis());
                SourceItem {
                    id,
                    title: subject,
                    updated_at_ms,
                }
            })
            .collect();
        Ok(items)
    }

    async fn read_item(
        &self,
        _source: &MemorySourceEntry,
        item_id: &str,
        config: &Config,
    ) -> Result<SourceContent, String> {
        let token = read_graph_token(config)?;
        let url = format!(
            "{GRAPH_BASE}/me/messages/{item_id}\
             ?$select=id,subject,body,from,toRecipients,receivedDateTime"
        );
        let msg = graph_get(&token, &url).await?;

        let subject = msg["subject"].as_str().unwrap_or("(no subject)").to_string();
        let from = msg["from"]["emailAddress"]["address"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let received = msg["receivedDateTime"].as_str().unwrap_or("").to_string();
        let body_content = msg["body"]["content"].as_str().unwrap_or("").to_string();
        let content_type_str = msg["body"]["contentType"].as_str().unwrap_or("text");
        let content_type = if content_type_str == "html" {
            ContentType::Html
        } else {
            ContentType::Plaintext
        };

        Ok(SourceContent {
            id: item_id.to_string(),
            title: subject,
            body: body_content,
            content_type,
            metadata: serde_json::json!({
                "from": from,
                "receivedDateTime": received,
                "source": "outlook_mail",
            }),
        })
    }
}

// ---------------------------------------------------------------------------
// OutlookCalendarReader
// ---------------------------------------------------------------------------

pub struct OutlookCalendarReader;

#[async_trait]
impl SourceReader for OutlookCalendarReader {
    fn kind(&self) -> SourceKind {
        SourceKind::OutlookCalendar
    }

    async fn list_items(
        &self,
        source: &MemorySourceEntry,
        config: &Config,
    ) -> Result<Vec<SourceItem>, String> {
        let token = read_graph_token(config)?;
        let days = source.m365_sync_days.unwrap_or(DEFAULT_SYNC_DAYS);
        let top = source.m365_max_items.unwrap_or(DEFAULT_MAX_ITEMS);
        let since = since_date(days);
        let url = format!(
            "{GRAPH_BASE}/me/events?$top={top}\
             &$filter=start/dateTime ge '{since}'\
             &$select=id,subject,start,end&$orderby=start/dateTime desc"
        );
        let data = graph_get(&token, &url).await?;
        let items = data
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or("unexpected graph response shape")?
            .iter()
            .map(|e| {
                let id = e["id"].as_str().unwrap_or("").to_string();
                let subject = e["subject"].as_str().unwrap_or("(no title)").to_string();
                let start = e["start"]["dateTime"].as_str().unwrap_or("").to_string();
                let updated_at_ms = chrono::DateTime::parse_from_rfc3339(&start)
                    .ok()
                    .map(|dt| dt.timestamp_millis());
                SourceItem {
                    id,
                    title: subject,
                    updated_at_ms,
                }
            })
            .collect();
        Ok(items)
    }

    async fn read_item(
        &self,
        _source: &MemorySourceEntry,
        item_id: &str,
        config: &Config,
    ) -> Result<SourceContent, String> {
        let token = read_graph_token(config)?;
        let url = format!(
            "{GRAPH_BASE}/me/events/{item_id}\
             ?$select=id,subject,start,end,body,attendees,organizer"
        );
        let event = graph_get(&token, &url).await?;

        let subject = event["subject"].as_str().unwrap_or("(no title)").to_string();
        let start = event["start"]["dateTime"].as_str().unwrap_or("").to_string();
        let end = event["end"]["dateTime"].as_str().unwrap_or("").to_string();
        let organizer = event["organizer"]["emailAddress"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let attendees: Vec<String> = event["attendees"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|a| a["emailAddress"]["name"].as_str().map(str::to_string))
            .collect();
        let body_content = event["body"]["content"].as_str().unwrap_or("").to_string();

        let body = format!(
            "## Event: {subject}\n\n\
             **Start**: {start}\n\
             **End**: {end}\n\
             **Organizer**: {organizer}\n\
             **Attendees**: {}\n\n\
             ---\n\n{body_content}",
            attendees.join(", ")
        );

        Ok(SourceContent {
            id: item_id.to_string(),
            title: subject,
            body,
            content_type: ContentType::Markdown,
            metadata: serde_json::json!({
                "start": start,
                "end": end,
                "organizer": organizer,
                "source": "outlook_calendar",
            }),
        })
    }
}

// ---------------------------------------------------------------------------
// TeamsMessagesReader
// ---------------------------------------------------------------------------

pub struct TeamsMessagesReader;

#[async_trait]
impl SourceReader for TeamsMessagesReader {
    fn kind(&self) -> SourceKind {
        SourceKind::TeamsMessages
    }

    async fn list_items(
        &self,
        source: &MemorySourceEntry,
        config: &Config,
    ) -> Result<Vec<SourceItem>, String> {
        let token = read_graph_token(config)?;
        let top_chats = 20usize;
        let messages_per_chat = source.m365_max_items.unwrap_or(DEFAULT_MAX_ITEMS) / 4;
        let messages_per_chat = messages_per_chat.max(5);

        // List recent chats
        let chats_url =
            format!("{GRAPH_BASE}/me/chats?$top={top_chats}&$select=id,topic,chatType");
        let chats_data = graph_get(&token, &chats_url).await?;
        let chats = chats_data
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or("unexpected chats response")?;

        let mut items = Vec::new();
        for chat in chats.iter().take(top_chats) {
            let chat_id = chat["id"].as_str().unwrap_or("").to_string();
            if chat_id.is_empty() {
                continue;
            }
            let topic = chat["topic"].as_str().unwrap_or("Chat").to_string();

            // Get recent messages for this chat
            let msgs_url = format!(
                "{GRAPH_BASE}/me/chats/{chat_id}/messages\
                 ?$top={messages_per_chat}&$select=id,body,from,createdDateTime"
            );
            match graph_get(&token, &msgs_url).await {
                Ok(msgs_data) => {
                    if let Some(msgs) = msgs_data.get("value").and_then(|v| v.as_array()) {
                        for msg in msgs {
                            let msg_id = msg["id"].as_str().unwrap_or("").to_string();
                            if msg_id.is_empty() {
                                continue;
                            }
                            let created =
                                msg["createdDateTime"].as_str().unwrap_or("").to_string();
                            let updated_at_ms =
                                chrono::DateTime::parse_from_rfc3339(&created)
                                    .ok()
                                    .map(|dt| dt.timestamp_millis());
                            let sender = msg["from"]["user"]["displayName"]
                                .as_str()
                                .unwrap_or("Unknown")
                                .to_string();
                            items.push(SourceItem {
                                id: format!("{chat_id}::{msg_id}"),
                                title: format!("[{topic}] {sender}"),
                                updated_at_ms,
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        chat_id = %chat_id,
                        error = %e,
                        "[memory_sources:teams] skipping chat messages"
                    );
                }
            }
        }

        Ok(items)
    }

    async fn read_item(
        &self,
        _source: &MemorySourceEntry,
        item_id: &str,
        config: &Config,
    ) -> Result<SourceContent, String> {
        // item_id format: "<chat_id>::<message_id>"
        let parts: Vec<&str> = item_id.splitn(2, "::").collect();
        if parts.len() != 2 {
            return Err(format!("invalid Teams message id: {item_id}"));
        }
        let (chat_id, msg_id) = (parts[0], parts[1]);

        let token = read_graph_token(config)?;
        let url = format!(
            "{GRAPH_BASE}/me/chats/{chat_id}/messages/{msg_id}\
             ?$select=id,body,from,createdDateTime,subject"
        );
        let msg = graph_get(&token, &url).await?;

        let sender = msg["from"]["user"]["displayName"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string();
        let created = msg["createdDateTime"].as_str().unwrap_or("").to_string();
        let body_content = msg["body"]["content"].as_str().unwrap_or("").to_string();
        let content_type_str = msg["body"]["contentType"].as_str().unwrap_or("text");
        let content_type = if content_type_str == "html" {
            ContentType::Html
        } else {
            ContentType::Plaintext
        };
        let title = format!("Teams message from {sender} at {created}");

        Ok(SourceContent {
            id: item_id.to_string(),
            title,
            body: body_content,
            content_type,
            metadata: serde_json::json!({
                "sender": sender,
                "createdDateTime": created,
                "chatId": chat_id,
                "source": "teams_messages",
            }),
        })
    }
}
