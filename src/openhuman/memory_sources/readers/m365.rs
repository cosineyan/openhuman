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

pub(crate) async fn graph_post(token: &str, url: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("build http client: {e}"))?;
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("graph post failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!("graph API POST returned {status}: {body_text}"));
    }
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("parse graph post response: {e}"))
}

/// Extract a Teams meeting join URL from a calendar event body (HTML or plain text).
/// Teams meeting URLs look like: https://teams.microsoft.com/l/meetup-join/...
fn extract_teams_join_url_from_body(body: &str) -> Option<String> {
    // Look for href containing teams.microsoft.com/l/meetup-join
    if let Some(start) = body.find("https://teams.microsoft.com/l/meetup-join/") {
        // Find the end of the URL (terminated by ", ', <, space, or newline)
        let rest = &body[start..];
        let end = rest
            .find(|c: char| matches!(c, '"' | '\'' | '<' | ' ' | '\n' | '\r' | '&'))
            .unwrap_or(rest.len());
        let raw = &rest[..end];
        // URL-decode the extracted URL (HTML entities like &amp; may be present)
        let decoded = raw.replace("&amp;", "&");
        if !decoded.is_empty() {
            return Some(decoded);
        }
    }
    None
}

/// Extract the Teams thread_id from a meetup-join URL.
/// URL format: https://teams.microsoft.com/l/meetup-join/{encoded_thread_id}/0?...
/// where encoded_thread_id is URL-encoded "19:meeting_XXX@thread.v2"
fn extract_thread_id_from_join_url(join_url: &str) -> Option<String> {
    // Find the path segment after /meetup-join/
    let marker = "/l/meetup-join/";
    let start = join_url.find(marker)? + marker.len();
    let rest = &join_url[start..];
    // Thread id is the next path segment (up to the next /)
    let end = rest.find('/').unwrap_or(rest.len());
    let encoded = &rest[..end];
    if encoded.is_empty() {
        return None;
    }
    // URL-decode: "19%3Ameeting_XXX%40thread.v2" → "19:meeting_XXX@thread.v2"
    urlencoding::decode(encoded).ok().map(|s| s.into_owned())
}

fn since_date(days: u32) -> String {
    let dt = Utc::now() - chrono::Duration::days(days as i64);
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Parse a Graph API datetime string to milliseconds since epoch.
///
/// Graph API returns `start.dateTime` as `"2026-06-29T13:00:00.0000000"` — 7
/// fractional-second digits, which is not valid RFC 3339 (chrono only accepts
/// 0, 3, 6, or 9 digits). We truncate to millisecond precision (3 digits)
/// before parsing to handle this format robustly.
fn parse_graph_datetime_ms(s: &str) -> Option<i64> {
    // Normalise fractional seconds to exactly 3 digits, then append 'Z' if missing
    let normalised = if let Some(dot_pos) = s.find('.') {
        let frac = &s[dot_pos + 1..];
        let frac_digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
        let frac3 = if frac_digits.len() >= 3 {
            frac_digits[..3].to_string()
        } else {
            format!("{:0<3}", frac_digits)
        };
        let rest_after_frac = &frac[frac_digits.len()..]; // e.g. "Z" or ""
        let suffix = if rest_after_frac.is_empty() || rest_after_frac == "0000000" {
            "Z"
        } else {
            rest_after_frac
        };
        format!("{}.{}{}", &s[..dot_pos], frac3, suffix)
    } else if s.ends_with('Z') || s.contains('+') || s.contains('-') {
        s.to_string()
    } else {
        format!("{}Z", s)
    };
    chrono::DateTime::parse_from_rfc3339(&normalised)
        .ok()
        .map(|dt| dt.timestamp_millis())
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
                let updated_at_ms = parse_graph_datetime_ms(&received);
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
                let updated_at_ms = parse_graph_datetime_ms(&start);
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
        let _messages_per_chat = (source.m365_max_items.unwrap_or(DEFAULT_MAX_ITEMS) / 4).max(5);
        let since_ms =
            (chrono::Utc::now() - chrono::Duration::days(days as i64)).timestamp_millis();

        // Fetch chats that have recent activity within the sync window.
        // Sort by lastMessageDateTime desc so we can stop once we reach chats
        // older than the window — avoids fetching all 500+ chats every sync.
        let since_iso = (chrono::Utc::now() - chrono::Duration::days(days as i64))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        let mut all_chats: Vec<serde_json::Value> = Vec::new();
        let mut chats_url = Some(format!(
            "{GRAPH_BASE}/me/chats?$top=50&$select=id,topic,chatType\
             &$expand=members"
        ));
        let mut pages = 0usize;
        while let Some(url) = chats_url {
            if pages >= 10 {
                break; // safety cap at 500 chats
            }
            let chats_data = graph_get(&token, &url).await?;
            let arr = chats_data
                .get("value")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            all_chats.extend(arr.iter().cloned());

            chats_url = if arr.len() < 50 {
                None
            } else {
                chats_data
                    .get("@odata.nextLink")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            };
            pages += 1;
        }
        let chats = all_chats;
        log::info!(
            "[teams_messages] found {} active chats in last {days} days",
            chats.len()
        );

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
            // Fetch all messages within the sync window using pagination.
            // We fetch newest-first and stop as soon as we hit a message older
            // than the window — avoids pulling the full history of old chats.
            // already_ingested dedup in sync.rs prevents double-storing on re-sync.
            let mut msgs_url: Option<String> = Some(format!(
                "{GRAPH_BASE}/me/chats/{chat_id}/messages\
                 ?$top=50&$select=id,body,from,createdDateTime\
                 &$orderby=createdDateTime desc"
            ));
            let mut page = 0usize;
            let mut hit_window_end = false;
            while let Some(url) = msgs_url {
                if page >= 20 || hit_window_end {
                    break; // safety cap: max 1000 messages per chat
                }
                match graph_get(&token, &url).await {
                    Ok(msgs_data) => {
                        let msgs = msgs_data
                            .get("value")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        for msg in &msgs {
                            let msg_id = msg["id"].as_str().unwrap_or("").to_string();
                            if msg_id.is_empty() {
                                continue;
                            }
                            let created = msg["createdDateTime"].as_str().unwrap_or("").to_string();
                            let ts = parse_graph_datetime_ms(&created);
                            if let Some(t) = ts {
                                if t < since_ms {
                                    // This message (and all following, since desc order)
                                    // are outside the window — stop pagination.
                                    hit_window_end = true;
                                    break;
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
                        // Only continue to next page if there are more and we haven't
                        // hit the window end
                        msgs_url = if hit_window_end || msgs.len() < 50 {
                            None
                        } else {
                            msgs_data
                                .get("@odata.nextLink")
                                .and_then(|v| v.as_str())
                                .map(str::to_string)
                        };
                        page += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            chat_id = %chat_id,
                            error = %e,
                            "[memory_sources:teams] skipping chat messages"
                        );
                        break;
                    }
                }
            } // end while msgs_url
        } // end for chat

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

// ---------------------------------------------------------------------------
// TeamsTranscriptReader
// ---------------------------------------------------------------------------
//
// Pipeline:
//   list_items → Calendar events (online meetings, last N days)
//                Item id = "{thread_id}||{meeting_subject}||{start_iso}"
//   read_item  → m365-cli `meetings recap --summary` + `meetings recap`
//                Produces one chunk per meeting with:
//                  - Full transcript text (speaker: line format)
//                  - AI summary topics & action items
//                  - Metadata: participants, topics, recording URL
// ---------------------------------------------------------------------------

pub struct TeamsTranscriptReader;

#[async_trait]
impl SourceReader for TeamsTranscriptReader {
    fn kind(&self) -> SourceKind {
        SourceKind::TeamsTranscript
    }

    async fn list_items(
        &self,
        source: &MemorySourceEntry,
        config: &Config,
    ) -> Result<Vec<SourceItem>, String> {
        let token = read_graph_token(config)?;
        let days = source.m365_sync_days.unwrap_or(30);
        let max_items = source.m365_max_items.unwrap_or(500) as usize;

        let start = (Utc::now() - chrono::Duration::days(days as i64))
            .format("%Y-%m-%dT00:00:00Z")
            .to_string();
        let end = (Utc::now() + chrono::Duration::days(1))
            .format("%Y-%m-%dT00:00:00Z")
            .to_string();

        // Fetch all pages (Graph API max $top=100 per page for calendarView)
        let page_size = 100usize;
        let params = format!(
            "startDateTime={start}&endDateTime={end}\
             &$select=id,subject,start,end,onlineMeeting,onlineMeetingUrl,isOnlineMeeting,attendees\
             &$top={page_size}"
        );
        let mut next_url = Some(format!("{GRAPH_BASE}/me/calendarView?{params}"));
        let mut all_events: Vec<serde_json::Value> = Vec::new();

        while let Some(url) = next_url {
            if all_events.len() >= max_items {
                break;
            }
            let data = graph_get(&token, &url).await?;
            if let Some(arr) = data.get("value").and_then(|v| v.as_array()) {
                all_events.extend(arr.iter().cloned());
            }
            // Follow @odata.nextLink for pagination
            next_url = data
                .get("@odata.nextLink")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }

        let items = all_events
            .iter()
            .filter_map(|ev| {
                // Only include online meetings
                let is_online = ev["isOnlineMeeting"].as_bool().unwrap_or(false)
                    || ev["onlineMeetingUrl"].as_str().is_some();
                if !is_online {
                    return None;
                }

                let subject = ev["subject"].as_str().unwrap_or("(no subject)").to_string();
                let start_dt = ev["start"]["dateTime"].as_str().unwrap_or("").to_string();

                // Extract join URL for thread_id lookup
                let join_url = ev["onlineMeetingUrl"]
                    .as_str()
                    .or_else(|| ev["onlineMeeting"]["joinUrl"].as_str())
                    .unwrap_or("")
                    .to_string();

                // Also get event id as fallback for meetings without join URL
                let event_id = ev["id"].as_str().unwrap_or("").to_string();

                // Skip if no way to look up the meeting
                if join_url.is_empty() && event_id.is_empty() {
                    return None;
                }

                let updated_at_ms = parse_graph_datetime_ms(&start_dt);

                // Item id: prefer join URL, fall back to event ID
                let lookup_key = if !join_url.is_empty() {
                    format!("join:{}", urlencoding::encode(&join_url))
                } else {
                    format!("event:{}", urlencoding::encode(&event_id))
                };

                let item_id = format!(
                    "{}||{}||{}",
                    lookup_key,
                    urlencoding::encode(&subject),
                    urlencoding::encode(&start_dt)
                );

                Some(SourceItem {
                    id: item_id,
                    title: subject,
                    updated_at_ms,
                })
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
        // Parse the encoded item_id
        let parts: Vec<&str> = item_id.splitn(3, "||").collect();
        if parts.len() < 2 {
            return Err(format!("invalid TeamsTranscript item_id: {item_id}"));
        }
        let lookup_key = urlencoding::decode(parts[0])
            .map_err(|e| format!("decode lookup_key: {e}"))?
            .into_owned();
        let subject = urlencoding::decode(parts[1])
            .map_err(|e| format!("decode subject: {e}"))?
            .into_owned();
        let start_dt = if parts.len() > 2 {
            urlencoding::decode(parts[2])
                .map_err(|e| format!("decode start_dt: {e}"))?
                .into_owned()
        } else {
            String::new()
        };

        // Step 1: extract thread_id from the join URL (no Graph API call needed —
        // the thread_id is embedded in the meetup-join path segment of the URL itself,
        // and the /me/onlineMeetings filter API only works for meetings YOU organized).
        let thread_id = if let Some(join_url) = lookup_key.strip_prefix("join:") {
            extract_thread_id_from_join_url(join_url).ok_or_else(|| {
                format!("cannot extract thread_id from join URL for: {subject}")
            })?
        } else if let Some(event_id) = lookup_key.strip_prefix("event:") {
            // For meetings without join URL in Calendar, fetch the event body and
            // extract the Teams join URL embedded in the HTML.
            let graph_tok = read_graph_token(config)?;
            let event_url = format!(
                "{GRAPH_BASE}/me/events/{event_id}?$select=body,onlineMeetingUrl,subject"
            );
            let event_data = graph_get(&graph_tok, &event_url)
                .await
                .map_err(|e| format!("calendar event lookup for {subject}: {e}"))?;

            let body_content = event_data
                .get("body")
                .and_then(|b| b.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let join_url_candidate =
                extract_teams_join_url_from_body(body_content).or_else(|| {
                    event_data
                        .get("onlineMeetingUrl")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                });

            match join_url_candidate.as_deref().and_then(extract_thread_id_from_join_url) {
                Some(tid) => tid,
                None => {
                    return Err(format!(
                        "cannot find Teams thread_id for event {event_id} ({subject})"
                    ))
                }
            }
        } else {
            return Err(format!("invalid lookup_key format: {lookup_key}"));
        };

        // Step 2: run m365-cli meetings recap to get transcript + summary
        let token_file = config.workspace_dir.join("m365").join("tokens.json");
        let script = crate::openhuman::m365::ops::resolve_m365_cli_script()
            .ok_or("m365_cli.py not found")?;

        // Get AI summary (best-effort — substrate token may be expired)
        let summary_json = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            tokio::process::Command::new("python3")
                .arg(&script)
                .args(["meetings", "recap", &thread_id, "--summary", "--json"])
                .env("M365_TOKEN_FILE", token_file.to_string_lossy().as_ref())
                .output(),
        )
        .await
        {
            Ok(Ok(out)) => serde_json::from_slice::<serde_json::Value>(&out.stdout)
                .unwrap_or(serde_json::Value::Null),
            Ok(Err(e)) => {
                log::warn!("[teams_transcript] meetings recap --summary failed: {e}");
                serde_json::Value::Null
            }
            Err(_) => {
                log::warn!("[teams_transcript] meetings recap --summary timed out");
                serde_json::Value::Null
            }
        };

        // Get transcript (required — skip if unavailable)
        let transcript_json = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            tokio::process::Command::new("python3")
                .arg(&script)
                .args(["meetings", "recap", &thread_id, "--json"])
                .env("M365_TOKEN_FILE", token_file.to_string_lossy().as_ref())
                .output(),
        )
        .await
        {
            Ok(Ok(out)) => serde_json::from_slice::<serde_json::Value>(&out.stdout)
                .unwrap_or(serde_json::Value::Null),
            Ok(Err(e)) => {
                return Err(format!("meetings recap transcript failed: {e}"));
            }
            Err(_) => {
                return Err("meetings recap transcript timed out after 120s".to_string());
            }
        };

        // Step 3: build content body
        let mut body = format!("[Meeting: {subject}] [Date: {start_dt}]\n\n");

        // Participants are embedded in the transcript entries (displayName per speaker)
        let mut participants: Vec<String> = Vec::new();

        // AI Summary section
        let mut topics: Vec<String> = Vec::new();
        let mut action_items: Vec<String> = Vec::new();

        if let Some(data) = summary_json.get("data") {
            body.push_str("## AI Meeting Summary\n\n");

            if let Some(topic_list) = data.get("topics").and_then(|t| t.as_array()) {
                for t in topic_list {
                    let headline = t.get("headline").and_then(|v| v.as_str()).unwrap_or("");
                    let summary = t.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                    if !headline.is_empty() {
                        body.push_str(&format!("### {headline}\n{summary}\n\n"));
                        topics.push(headline.to_string());
                    }
                }
            }

            if let Some(actions) = data.get("actionItems").and_then(|a| a.as_array()) {
                if !actions.is_empty() {
                    body.push_str("## Action Items\n\n");
                    for a in actions {
                        let text = a.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        let owner = a.get("owner").and_then(|v| v.as_str()).unwrap_or("");
                        let item = if owner.is_empty() {
                            format!("- {text}")
                        } else {
                            format!("- {text} (@{owner})")
                        };
                        body.push_str(&item);
                        body.push('\n');
                        action_items.push(text.to_string());
                    }
                    body.push('\n');
                }
            }
        }

        // Transcript section — also compute speaker statistics
        let mut transcript_entries: Vec<String> = Vec::new();
        // speaker_id -> (total_secs, turns, words)
        let mut speaker_stats: std::collections::HashMap<String, (f64, u32, u32)> =
            std::collections::HashMap::new();

        if let Some(data) = transcript_json.get("data") {
            if let Some(entries) = data.get("entries").and_then(|e| e.as_array()) {
                if !entries.is_empty() {
                    body.push_str("## Transcript\n\n");

                    // Helper: parse "HH:MM:SS.fff" offset to seconds
                    fn parse_offset_secs(s: &str) -> f64 {
                        let dot = s.find('.').unwrap_or(s.len());
                        let frac: f64 = if dot < s.len() {
                            format!("0{}", &s[dot..]).parse().unwrap_or(0.0)
                        } else {
                            0.0
                        };
                        let parts: Vec<u64> =
                            s[..dot].split(':').filter_map(|p| p.parse().ok()).collect();
                        let base = match parts.len() {
                            3 => parts[0] * 3600 + parts[1] * 60 + parts[2],
                            2 => parts[0] * 60 + parts[1],
                            1 => parts[0],
                            _ => 0,
                        };
                        base as f64 + frac
                    }

                    for entry in entries {
                        let speaker = entry.get("speaker").and_then(|v| v.as_str()).unwrap_or("?");
                        let text = entry.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        let start_off = entry
                            .get("startOffset")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0");
                        let end_off = entry
                            .get("endOffset")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0");

                        let duration =
                            (parse_offset_secs(end_off) - parse_offset_secs(start_off)).max(0.0);
                        let word_count = text.split_whitespace().count() as u32;

                        let stat = speaker_stats
                            .entry(speaker.to_string())
                            .or_insert((0.0, 0, 0));
                        stat.0 += duration;
                        stat.1 += 1;
                        stat.2 += word_count;

                        let line = format!("[{start_off}] {speaker}: {text}");
                        body.push_str(&line);
                        body.push('\n');
                        if !participants.contains(&speaker.to_string()) {
                            participants.push(speaker.to_string());
                        }
                        transcript_entries.push(line);
                    }
                }
            }
        }

        // Build speaker breakdown sorted by speaking time
        let total_secs: f64 = speaker_stats.values().map(|(s, _, _)| s).sum();
        let mut speaker_breakdown: Vec<serde_json::Value> = speaker_stats
            .iter()
            .map(|(name, (secs, turns, words))| {
                let pct = if total_secs > 0.0 {
                    (secs / total_secs * 100.0 * 10.0).round() / 10.0
                } else {
                    0.0
                };
                serde_json::json!({
                    "name": name,
                    "speaking_seconds": (*secs as u32),
                    "speaking_pct": pct,
                    "turns": turns,
                    "words": words,
                })
            })
            .collect();
        speaker_breakdown.sort_by(|a, b| {
            let sa = a["speaking_seconds"].as_u64().unwrap_or(0);
            let sb = b["speaking_seconds"].as_u64().unwrap_or(0);
            sb.cmp(&sa)
        });

        if body.len() <= format!("[Meeting: {subject}] [Date: {start_dt}]\n\n").len() {
            return Err(format!(
                "no transcript or summary available for meeting: {subject}"
            ));
        }

        // Add participants prefix for entity extraction
        let participants_str = if participants.is_empty() {
            String::new()
        } else {
            format!(" [Participants: {}]", participants.join(", "))
        };

        let topics_str = if topics.is_empty() {
            String::new()
        } else {
            format!(" [Topics: {}]", topics.join("; "))
        };

        let full_body = format!(
            "[Meeting: {subject}] [Date: {start_dt}]{participants_str}{topics_str}\n\n{body}"
        );

        Ok(SourceContent {
            id: item_id.to_string(),
            title: subject.clone(),
            body: full_body,
            content_type: ContentType::Plaintext,
            metadata: serde_json::json!({
                "meeting_subject": subject,
                "start_datetime": start_dt,
                "thread_id": thread_id,
                "participants": participants,
                "topics": topics,
                "action_items": action_items,
                "transcript_length": transcript_entries.len(),
                "speaker_breakdown": speaker_breakdown,
                "source": "teams_transcript",
            }),
        })
    }
}
