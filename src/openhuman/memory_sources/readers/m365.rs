//! Memory source readers for Microsoft 365 data — Outlook mail, calendar, and Teams messages.
//!
//! All three readers call the Microsoft Graph API directly using the cached
//! graph access token from the m365 token file written by the bundled m365-cli.

use async_trait::async_trait;
use chrono::Utc;

use crate::openhuman::config::Config;
use crate::openhuman::memory_sources::types::{ContentType, SourceContent, SourceItem, SourceKind};

use super::{MemorySourceEntry, SourceReader};

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";
const DEFAULT_SYNC_DAYS: u32 = 30;
const DEFAULT_MAX_ITEMS: u32 = 50;

// ---------------------------------------------------------------------------
// Token helper
// ---------------------------------------------------------------------------

pub fn read_graph_token_public(config: &Config) -> Result<String, String> {
    read_token_by_key(config, "graph")
}

fn read_graph_token(config: &Config) -> Result<String, String> {
    read_token_by_key(config, "graph")
}

/// Read the graph_chat token (Outlook Web appid 9199bf20, includes Chat.Read scope).
/// This token is refreshed by auth login/refresh in SAP Systems.
fn read_chat_graph_token(config: &Config) -> Result<String, String> {
    read_token_by_key(config, "graph_chat").map_err(|e| {
        format!("{e}. Please click Refresh in SAP Systems to renew the Teams chat token.")
    })
}

fn read_token_by_key(config: &Config, key: &str) -> Result<String, String> {
    let path = config.workspace_dir.join("m365").join("tokens.json");
    let raw =
        std::fs::read_to_string(&path).map_err(|e| format!("cannot read m365 token file: {e}"))?;
    let tokens: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("cannot parse m365 token file: {e}"))?;
    let entry = tokens
        .get(key)
        .ok_or_else(|| format!("{key} token not found — please connect Outlook in SAP Systems"))?;
    let exp = entry.get("expiresOn").and_then(|v| v.as_i64()).unwrap_or(0);
    // Use a 30-second safety margin (matches m365_token_usable_for) so the
    // scheduler check and this reader agree on token validity.
    if exp > 0 && exp < Utc::now().timestamp() + 30 {
        return Err(format!(
            "{key} token expired — please click Refresh in SAP Systems to renew it"
        ));
    }
    entry
        .get("token")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{key} token value is missing"))
}

// ---------------------------------------------------------------------------
// Graph API helper
// ---------------------------------------------------------------------------

pub(crate) async fn graph_get(token: &str, url: &str) -> Result<serde_json::Value, String> {
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
             ?$select=id,subject,body,from,toRecipients,ccRecipients,receivedDateTime"
        );
        let msg = graph_get(&token, &url).await?;

        let subject = msg["subject"]
            .as_str()
            .unwrap_or("(no subject)")
            .to_string();

        // Helper: extract name+email pair from an address node
        let format_participant = |addr: &serde_json::Value| -> serde_json::Value {
            let email = addr["emailAddress"]["address"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let name = addr["emailAddress"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string();
            serde_json::json!({"name": name, "email": email})
        };

        // Build participants: From + To + CC unified list, deduplicated by email
        let mut seen_emails = std::collections::HashSet::new();
        let mut participants: Vec<serde_json::Value> = Vec::new();

        // From
        let from_p = format_participant(&msg["from"]);
        if let Some(e) = from_p["email"].as_str().filter(|e| !e.is_empty()) {
            if seen_emails.insert(e.to_lowercase()) {
                participants.push(from_p);
            }
        }
        // To
        for addr in msg["toRecipients"].as_array().unwrap_or(&vec![]) {
            let p = format_participant(addr);
            if let Some(e) = p["email"].as_str().filter(|e| !e.is_empty()) {
                if seen_emails.insert(e.to_lowercase()) {
                    participants.push(p);
                }
            }
        }
        // CC
        for addr in msg["ccRecipients"].as_array().unwrap_or(&vec![]) {
            let p = format_participant(addr);
            if let Some(e) = p["email"].as_str().filter(|e| !e.is_empty()) {
                if seen_emails.insert(e.to_lowercase()) {
                    participants.push(p);
                }
            }
        }

        // Also build the content-prefix lists as before
        let format_addr = |addr: &serde_json::Value| -> String {
            let email = addr["emailAddress"]["address"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let name = addr["emailAddress"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string();
            if name.is_empty() || name == email {
                email
            } else {
                format!("{name} <{email}>")
            }
        };

        let from = format_addr(&msg["from"]);
        let from_email = msg["from"]["emailAddress"]["address"]
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

        // Build To: list (all recipients)
        let to_list: Vec<String> = msg["toRecipients"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(format_addr)
            .collect();
        let to_str = if to_list.is_empty() {
            String::new()
        } else {
            format!(" [To: {}]", to_list.join(", "))
        };

        // Build CC: list (all recipients)
        let cc_list: Vec<String> = msg["ccRecipients"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(format_addr)
            .collect();
        let cc_str = if cc_list.is_empty() {
            String::new()
        } else {
            format!(" [CC: {}]", cc_list.join(", "))
        };

        // Build [Participants: ...] prefix for entity extraction
        // Format: "Name <email>" for each unique participant
        let participants_str = if participants.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = participants
                .iter()
                .map(|p| {
                    let name = p["name"].as_str().unwrap_or("");
                    let email = p["email"].as_str().unwrap_or("");
                    if name.is_empty() || name == email {
                        email.to_string()
                    } else {
                        format!("{name} <{email}>")
                    }
                })
                .collect();
            format!(" [Participants: {}]", parts.join(", "))
        };

        Ok(SourceContent {
            id: item_id.to_string(),
            title: subject.clone(),
            body: format!(
                "[Subject: {subject}] [From: {from}] [Date: {received}]{to_str}{cc_str}{participants_str}\n{body_content}"
            ),
            content_type,
            metadata: serde_json::json!({
                "from": from_email,
                "from_display": from,
                "to": to_list,
                "cc": cc_list,
                "participants": participants,
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

        let subject = event["subject"]
            .as_str()
            .unwrap_or("(no title)")
            .to_string();
        let start = event["start"]["dateTime"]
            .as_str()
            .unwrap_or("")
            .to_string();
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
// NOTE: The regular graph token (obtained via Teams refresh token exchange,
// appid 5e3ce6c0) does NOT include Chat.Read scope. We use graph_chat token
// (from the Outlook tab's MSAL cache, appid 9199bf20) which includes Chat.Read.
// This token is populated by ensure_chat_graph_token() in tokens.py and cached
// under the 'graph_chat' key in tokens.json.

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
        let token = read_chat_graph_token(config).map_err(|e| {
            format!("{e}. Hint: sync Outlook Mail first to populate the chat graph token.")
        })?;
        let days = source.m365_sync_days.unwrap_or(DEFAULT_SYNC_DAYS);
        let messages_per_chat = (source.m365_max_items.unwrap_or(DEFAULT_MAX_ITEMS) / 4).max(5);

        // Fetch all chats with pagination (Vera's 1:1 may be beyond page 1)
        // $expand=members without $select — conversationMember does not support
        // $select sub-filtering (causes 400 Bad Request).
        let mut all_chats: Vec<serde_json::Value> = Vec::new();
        let mut chats_url = Some(format!(
            "{GRAPH_BASE}/me/chats?$top=50&$select=id,topic,chatType\
             &$expand=members"
        ));
        let mut pages = 0usize;
        while let Some(url) = chats_url {
            if pages >= 10 {
                break;
            } // safety cap at 500 chats
            let chats_data = graph_get(&token, &url).await?;
            if let Some(arr) = chats_data.get("value").and_then(|v| v.as_array()) {
                all_chats.extend(arr.iter().cloned());
            }
            chats_url = chats_data
                .get("@odata.nextLink")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            pages += 1;
        }
        let chats = all_chats;

        let since_ms =
            (chrono::Utc::now() - chrono::Duration::days(days as i64)).timestamp_millis();
        let mut items = Vec::new();

        for chat in chats.iter() {
            let chat_id = chat["id"].as_str().unwrap_or("").to_string();
            if chat_id.is_empty() {
                continue;
            }
            let chat_type = chat["chatType"].as_str().unwrap_or("unknown");
            // Build a human-readable label for the chat based on type and members.
            let chat_label = if chat_type == "oneOnOne" {
                // Pick the member who is not the current user (any non-empty name)
                let members = chat
                    .get("members")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let other = members
                    .iter()
                    .filter_map(|m| m.get("displayName").and_then(|v| v.as_str()))
                    .find(|name| !name.is_empty())
                    .unwrap_or("Unknown");
                format!("1:1 with {other}")
            } else {
                let topic = chat["topic"].as_str().unwrap_or("").to_string();
                if topic.is_empty() {
                    let members = chat
                        .get("members")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let names: Vec<&str> = members
                        .iter()
                        .filter_map(|m| m.get("displayName").and_then(|v| v.as_str()))
                        .filter(|n| !n.is_empty())
                        .take(3)
                        .collect();
                    if names.is_empty() {
                        "Group Chat".to_string()
                    } else {
                        names.join(", ")
                    }
                } else {
                    topic
                }
            };
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
                            let created = msg["createdDateTime"].as_str().unwrap_or("").to_string();
                            let ts = chrono::DateTime::parse_from_rfc3339(&created)
                                .ok()
                                .map(|dt| dt.timestamp_millis());
                            // Skip messages older than sync window
                            if let Some(t) = ts {
                                if t < since_ms {
                                    continue;
                                }
                            }
                            let sender = msg["from"]["user"]["displayName"]
                                .as_str()
                                .unwrap_or("Unknown")
                                .to_string();
                            items.push(SourceItem {
                                id: format!("{chat_id}::{msg_id}"),
                                title: format!("[{chat_label}] {sender}"),
                                updated_at_ms: ts,
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
        let parts: Vec<&str> = item_id.splitn(2, "::").collect();
        if parts.len() != 2 {
            return Err(format!("invalid Teams message id: {item_id}"));
        }
        let (chat_id, msg_id) = (parts[0], parts[1]);
        let token = read_chat_graph_token(config)?;

        // For 1:1 chats, fetch the other participant's name to include in body.
        // This allows Claude to find messages by the other person's name.
        let chat_info = {
            let members_url = format!(
                "{GRAPH_BASE}/me/chats/{chat_id}?$select=id,topic,chatType&$expand=members"
            );
            graph_get(&token, &members_url).await.ok()
        };
        let chat_type = chat_info
            .as_ref()
            .and_then(|c| c.get("chatType"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let chat_context = if chat_type == "oneOnOne" {
            // Find the other participant (not the current user)
            let members = chat_info
                .as_ref()
                .and_then(|c| c.get("members"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            // Pick first member whose name differs (the "other" person)
            let other = members
                .iter()
                .filter_map(|m| m.get("displayName").and_then(|v| v.as_str()))
                .find(|name| !name.is_empty())
                .unwrap_or("1:1 chat")
                .to_string();
            format!("[1:1 Chat with {other}]")
        } else {
            let topic = chat_info
                .as_ref()
                .and_then(|c| c.get("topic"))
                .and_then(|v| v.as_str())
                .unwrap_or("Group Chat")
                .to_string();
            format!("[Group Chat: {topic}]")
        };
        let url = format!(
            "{GRAPH_BASE}/me/chats/{chat_id}/messages/{msg_id}\
             ?$select=id,body,from,createdDateTime"
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

        Ok(SourceContent {
            id: item_id.to_string(),
            title: format!("Teams message from {sender} at {created}"),
            // Prefix body with chat context, sender and human-readable date so Claude can
            // identify the message time without converting raw timestamps.
            body: format!("{chat_context} [From: {sender}] [{created}]\n{body_content}"),
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
