//! `profile_person` mode for the `memory_tree` tool.
//!
//! Aggregates everything known about a person across all memory sources
//! (chat, email, document) into a structured profile, enriched with
//! real-time org chart data from the Microsoft Graph API.

use anyhow::Result;
use serde_json::{json, Value};

use crate::openhuman::config::Config;
use crate::openhuman::memory_sources::readers::m365::graph_get;
use crate::openhuman::memory_store::chunks::store::with_connection;
use crate::openhuman::tools::traits::{Tool, ToolResult};
pub struct ProfilePersonTool;

/// Org chart info fetched from Graph API.
#[derive(Default)]
struct OrgInfo {
    user_id: String,
    display_name: String,
    job_title: String,
    department: String,
    mail: String,
    office: String,
    city: String,
    manager_name: String,
    manager_title: String,
    manager_mail: String,
    direct_reports: Vec<(String, String, String)>, // (name, title, mail)
}

async fn fetch_org_info(token: &str, name: &str, email: &str) -> Result<OrgInfo, String> {
    // 1. Try People API first (works with People.Read, no admin consent needed)
    //    Falls back to /users filter (needs User.Read.All).
    let user = if !email.is_empty() {
        // Direct filter by email — works if User.Read.All is granted
        let filter = format!("mail eq '{email}'");
        let url = format!(
            "https://graph.microsoft.com/v1.0/users?$filter={}&\
             $select=id,displayName,jobTitle,department,mail,officeLocation,city&$top=1",
            urlencoding::encode(&filter)
        );
        let data = graph_get(token, &url)
            .await
            .map_err(|e| format!("Graph /users filter failed: {e}"))?;
        data.get("value")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .cloned()
            .ok_or_else(|| format!("No user found with email={email}"))?
    } else {
        // SAP (and many enterprises) store names as "Last, First" in displayName.
        // Strategy: try multiple filter patterns to handle both "First Last" and "Last, First" formats.
        // 1. Try last name first (most reliable for SAP: "Rabe, Robert" → startswith "Rabe")
        // 2. Try full name match
        // 3. Try first name (least reliable)
        let parts: Vec<&str> = name.split_whitespace().collect();
        let last_name = parts.last().unwrap_or(&name);
        let first_name = parts.first().unwrap_or(&name);

        // Build candidate filters in priority order:
        // 1. "Last, First" exact prefix — most reliable for SAP/enterprise directories
        //    e.g. "Vera Jia" → startswith(displayName,'Jia, Vera')
        // 2. "Last" prefix — catches "Rabe, Robert" when first name is different
        // 3. "first.last@" email guess — for "Sue Jiang" → "sue.jiang@sap.com"
        let mut filters: Vec<String> = Vec::new();
        if parts.len() >= 2 {
            // "Last, First" format (SAP standard)
            filters.push(format!(
                "startswith(displayName,'{last_name}, {first_name}')"
            ));
        }
        filters.push(format!("startswith(displayName,'{last_name}')"));
        if parts.len() == 2 {
            // email guess fallback
            filters.push(format!(
                "startswith(mail,'{}.{}@')",
                first_name.to_lowercase(),
                last_name.to_lowercase()
            ));
        }
        let mut found_user: Option<serde_json::Value> = None;
        for (i, filter) in filters.iter().enumerate() {
            let url = format!(
                "https://graph.microsoft.com/v1.0/users?$filter={}&\
                 $select=id,displayName,jobTitle,department,mail,officeLocation,city&$top=10",
                urlencoding::encode(filter)
            );
            if let Ok(data) = graph_get(token, &url).await {
                if let Some(users) = data.get("value").and_then(|v| v.as_array()) {
                    let is_email_filter = filter.starts_with("startswith(mail,");
                    let best = users.iter().find(|u| {
                        if is_email_filter {
                            // Email filter is precise enough — accept any result
                            true
                        } else {
                            // Only accept if displayName contains ALL parts of the input name
                            let dn = u
                                .get("displayName")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_lowercase();
                            parts.iter().all(|p| dn.contains(&p.to_lowercase()))
                        }
                    });
                    if let Some(u) = best {
                        log::info!(
                            "[profile_person] found user via filter={filter:?} display_name={:?}",
                            u.get("displayName").and_then(|v| v.as_str()).unwrap_or("?")
                        );
                        found_user = Some(u.clone());
                        break;
                    }
                }
            }
        }

        found_user.ok_or_else(|| {
            format!(
                "No user found matching name='{name}' in Microsoft Graph \
             (tried filters: {}). The name may not exist in your organization's directory, \
             or may be stored under a different format.",
                filters.join(", ")
            )
        })?
    };

    let user_id = user
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "User object missing 'id' field".to_string())?
        .to_string();
    let display_name = user
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let job_title = user
        .get("jobTitle")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let department = user
        .get("department")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mail = user
        .get("mail")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let office = user
        .get("officeLocation")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let city = user
        .get("city")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // 2. Get manager
    let mgr_url = format!(
        "https://graph.microsoft.com/v1.0/users/{user_id}/manager\
         ?$select=displayName,jobTitle,mail"
    );
    let (manager_name, manager_title, manager_mail) = graph_get(token, &mgr_url)
        .await
        .ok()
        .map(|m| {
            (
                m.get("displayName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                m.get("jobTitle")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                m.get("mail")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .unwrap_or_default();

    // 3. Get direct reports
    let reports_url = format!(
        "https://graph.microsoft.com/v1.0/users/{user_id}/directReports\
         ?$select=displayName,jobTitle,mail&$top=20"
    );
    let direct_reports = graph_get(token, &reports_url)
        .await
        .ok()
        .and_then(|d| d.get("value").and_then(|v| v.as_array()).cloned())
        .unwrap_or_default()
        .into_iter()
        .map(|r| {
            (
                r.get("displayName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                r.get("jobTitle")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                r.get("mail")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .collect();

    Ok(OrgInfo {
        user_id,
        display_name,
        job_title,
        department,
        mail,
        office,
        city,
        manager_name,
        manager_title,
        manager_mail,
        direct_reports,
    })
}

#[async_trait::async_trait]
impl Tool for ProfilePersonTool {
    fn name(&self) -> &str {
        "profile_person"
    }

    fn description(&self) -> &str {
        "Profile a person by aggregating everything known about them across all memory sources \
         (Teams chat, Outlook email, documents) PLUS real-time org chart from Microsoft Graph \
         (job title, manager, direct reports, department). Use name='Robert Rabe' or \
         email='rob.rabe@sap.com'. Optional: since_days (default 90), source_kinds filter."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Person's name (partial OK, e.g. 'Robert Rabe', 'Rabe')."
                },
                "email": {
                    "type": "string",
                    "description": "Person's email address for precise matching."
                },
                "since_days": {
                    "type": "integer",
                    "description": "Look back N days (default 90, 0 = all time)."
                },
                "source_kinds": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Filter to specific source kinds: chat, email, document. Default: all."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max chunks per category (default 20)."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> Result<ToolResult> {
        let config = crate::openhuman::config::rpc::load_config_with_timeout()
            .await
            .map_err(|e| anyhow::anyhow!("profile_person: load config failed: {e}"))?;

        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let email = args
            .get("email")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let since_days = args
            .get("since_days")
            .and_then(|v| v.as_u64())
            .unwrap_or(90);
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        let source_kinds: Vec<String> = args
            .get("source_kinds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        if name.is_empty() && email.is_empty() {
            return Ok(ToolResult::success(
                "profile_person: provide at least `name` or `email`".to_string(),
            ));
        }

        log::info!("[profile_person] execute called name={name:?} email={email:?}");

        // Fetch org chart from Graph API concurrently with memory query.
        // ensure_graph_token handles expiry check + auto-refresh in one place.
        let org_future = {
            let config_c = config.clone();
            let name_c = name.clone();
            let mut email_c = email.clone();

            // If email not given, look up in local contacts table for precise Graph search.
            // This handles non-standard email formats like "v.jia@sap.com" for "Vera Jia".
            if email_c.is_empty() && !name_c.is_empty() {
                if let Ok(contact) = lookup_contact_by_name(&config, &name_c) {
                    if !contact.email.is_empty() {
                        log::info!(
                            "[profile_person] found email from contacts table: {} for name={}",
                            contact.email,
                            name_c
                        );
                        email_c = contact.email;
                    }
                }
            }

            async move {
                match crate::openhuman::m365::ops::ensure_graph_token(&config_c).await {
                    Ok(token) => fetch_org_info(&token, &name_c, &email_c).await,
                    Err(e) => Err(format!("M365 token unavailable: {e}")),
                }
            }
        };

        let memory_future = {
            let config_c = config.clone();
            let name_c = name.clone();
            let email_c = email.clone();
            let sk = source_kinds.clone();
            tokio::task::spawn_blocking(move || {
                profile_person_blocking(&config_c, &name_c, &email_c, since_days, &sk, limit)
            })
        };

        let (org_result, memory_result) = tokio::join!(org_future, memory_future);
        log::info!(
            "[profile_person] org_result={}",
            match &org_result {
                Ok(o) => format!("OK display_name={}", o.display_name),
                Err(e) => format!("ERR: {e}"),
            }
        );
        let memory_text =
            memory_result.map_err(|e| anyhow::anyhow!("profile_person join error: {e}"))??;

        // Build org chart section
        let org_section = match org_result {
            Ok(org) => {
                let mut s = String::new();
                s.push_str("## Org Chart (Live from Microsoft Graph)\n\n");
                s.push_str(&format!(
                    "**{}**",
                    if org.display_name.is_empty() {
                        name.as_str()
                    } else {
                        &org.display_name
                    }
                ));
                if !org.job_title.is_empty() {
                    s.push_str(&format!(" — {}", org.job_title));
                }
                s.push('\n');
                if !org.department.is_empty() {
                    s.push_str(&format!("Department: {}\n", org.department));
                }
                if !org.mail.is_empty() {
                    s.push_str(&format!("Email: {}\n", org.mail));
                }
                if !org.office.is_empty() || !org.city.is_empty() {
                    s.push_str(&format!("Location: {} {}\n", org.city, org.office));
                }
                if !org.manager_name.is_empty() {
                    s.push_str(&format!(
                        "\n**Manager:** {} ({}) {}\n",
                        org.manager_name,
                        if org.manager_title.is_empty() {
                            "—"
                        } else {
                            &org.manager_title
                        },
                        org.manager_mail
                    ));
                }
                if !org.direct_reports.is_empty() {
                    s.push_str(&format!(
                        "\n**Direct Reports ({}):**\n",
                        org.direct_reports.len()
                    ));
                    for (rname, rtitle, rmail) in &org.direct_reports {
                        s.push_str(&format!(
                            "- {} ({}) {}\n",
                            rname,
                            if rtitle.is_empty() { "—" } else { rtitle },
                            rmail
                        ));
                    }
                }
                s.push('\n');
                s
            }
            Err(e) => {
                log::warn!("[profile_person] Graph API org chart unavailable: {e}");
                format!("## Org Chart\n\n_Not available: {e}_\n\n")
            }
        };

        Ok(ToolResult::success(format!("{org_section}{memory_text}")))
    }
}

fn profile_person_blocking(
    config: &Config,
    name: &str,
    email: &str,
    since_days: u64,
    source_kinds: &[String],
    limit: usize,
) -> Result<String> {
    with_connection(config, |conn| {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let since_ms = if since_days == 0 {
            0i64
        } else {
            now_ms - (since_days as i64) * 86_400_000
        };

        // 1. Find matching entity_ids from entity index
        let name_lower = name.to_lowercase().replace(' ', "-").replace(',', "");
        let name_lower = name_lower.trim_matches('-').to_string();
        let email_lower = email.to_lowercase();

        // Build entity_id patterns to match
        let mut entity_patterns: Vec<String> = Vec::new();
        if !name_lower.is_empty() {
            entity_patterns.push(format!("person:%{name_lower}%"));
            // Also try individual name parts
            for part in name.split_whitespace() {
                if part.len() > 2 {
                    entity_patterns.push(format!("person:%{}%", part.to_lowercase()));
                }
            }
        }
        if !email_lower.is_empty() {
            entity_patterns.push(format!("email:{email_lower}"));
            entity_patterns.push(format!(
                "person:%{}%",
                email_lower.split('@').next().unwrap_or("")
            ));
        }

        // 2. Find chunk_ids via entity index
        let mut mentioned_chunk_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for pattern in &entity_patterns {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT node_id FROM mem_tree_entity_index \
                 WHERE entity_id LIKE ?1 LIMIT 500",
            )?;
            let ids: Vec<String> = stmt
                .query_map([pattern], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            mentioned_chunk_ids.extend(ids);
        }

        // 3. Also find chunks where this person is the sender (From: prefix)
        let name_search = if name.is_empty() {
            email.to_string()
        } else {
            name.to_string()
        };

        // 4. Fetch chunks — split into "sent by" vs "mentions"
        let source_kind_filter = if source_kinds.is_empty() {
            "1=1".to_string()
        } else {
            let kinds: Vec<String> = source_kinds.iter().map(|k| format!("'{k}'")).collect();
            format!("source_kind IN ({})", kinds.join(","))
        };

        // "Sent by" — Teams/Outlook stores names as "Last, First" format.
        // Build multiple patterns to match both "First Last" and "Last, First" orderings.
        let mut from_patterns: Vec<String> = vec![format!("%From: {}%", name_search)];
        // If input is "First Last", also try "Last, First"
        let parts: Vec<&str> = name_search.split_whitespace().collect();
        if parts.len() == 2 {
            from_patterns.push(format!("%From: {}, {}%", parts[1], parts[0]));
        }
        // Also search by last name alone (for entity-based match)
        if let Some(last) = parts.last() {
            if last.len() > 2 {
                from_patterns.push(format!("%From: {},%", last));
                from_patterns.push(format!("%From: {}]%", last));
            }
        }

        // Collect all "sent by" chunks across all from_patterns
        let mut sent_chunks: Vec<(String, String, String, String, i64)> = Vec::new();
        let mut seen_sent_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for from_pattern in &from_patterns {
            let mut sent_stmt = conn.prepare(&format!(
                "SELECT id, source_kind, source_id, substr(content,1,300), timestamp_ms \
                 FROM mem_tree_chunks \
                 WHERE content LIKE ?1 \
                 AND timestamp_ms >= ?2 \
                 AND {source_kind_filter} \
                 ORDER BY timestamp_ms DESC \
                 LIMIT ?3"
            ))?;
            let rows: Vec<(String, String, String, String, i64)> = sent_stmt
                .query_map(
                    [
                        from_pattern,
                        &since_ms.to_string(),
                        &(limit * 2).to_string(),
                    ],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )?
                .filter_map(|r| r.ok())
                .collect();
            for row in rows {
                if seen_sent_ids.insert(row.0.clone()) {
                    sent_chunks.push(row);
                }
            }
        }
        sent_chunks.sort_by(|a, b| b.4.cmp(&a.4));
        sent_chunks.truncate(limit);

        // "Mentioned in" — from entity index, but not sent by
        // Further split into:
        //   - addressed_to: this person is in [To: ...] or [CC: ...] or [Participants: ...]
        //   - discussed_about: this person appears in body/entity but is NOT From/To/CC
        let sent_ids: std::collections::HashSet<String> =
            sent_chunks.iter().map(|(id, ..)| id.clone()).collect();
        let mention_ids: Vec<String> = mentioned_chunk_ids
            .iter()
            .filter(|id| !sent_ids.contains(*id))
            .cloned()
            .collect::<Vec<_>>();

        // To/CC pattern matches for "addressed to this person"
        let mut to_patterns: Vec<String> = Vec::new();
        for pat in &from_patterns {
            // Convert [From: X] → [To: X], [CC: X], [Participants: X]
            let base = pat.trim_matches('%');
            if base.starts_with("From: ") {
                let name_part = &base["From: ".len()..];
                to_patterns.push(format!("%To: {name_part}%"));
                to_patterns.push(format!("%CC: {name_part}%"));
                to_patterns.push(format!("%Participants: {name_part}%"));
            }
        }
        // Also match by email in To/CC/Participants
        if !email_lower.is_empty() {
            to_patterns.push(format!("%To: %{email_lower}%"));
            to_patterns.push(format!("%CC: %{email_lower}%"));
            to_patterns.push(format!("%Participants: %{email_lower}%"));
        }

        let mut addressed_chunks: Vec<(String, String, String, String, i64)> = Vec::new();
        let mut discussed_chunks: Vec<(String, String, String, i64)> = Vec::new();
        let mut seen_addr: std::collections::HashSet<String> = std::collections::HashSet::new();

        for chunk_id in mention_ids.iter().take(limit * 3) {
            // Check if this chunk has To/CC match for the person
            let row: Option<(String, String, String, String, i64)> = conn.query_row(
                &format!(
                    "SELECT id, source_kind, source_id, substr(content,1,300), timestamp_ms \
                     FROM mem_tree_chunks \
                     WHERE id=?1 AND timestamp_ms >= ?2 AND {source_kind_filter}"
                ),
                [chunk_id, &since_ms.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .ok();

            if let Some(row) = row {
                let content = &row.3;
                // Check if person is in To/CC/Participants
                let is_addressed = to_patterns.iter().any(|pat| {
                    let pat_lower = pat.to_lowercase();
                    let pat_trimmed = pat_lower.trim_matches('%');
                    content.to_lowercase().contains(pat_trimmed)
                });

                if is_addressed && seen_addr.insert(row.0.clone()) {
                    addressed_chunks.push(row);
                } else if !is_addressed && !seen_addr.contains(&row.0) {
                    discussed_chunks.push((row.1, row.2, row.3, row.4));
                }
            }
        }
        addressed_chunks.sort_by(|a, b| b.4.cmp(&a.4));
        addressed_chunks.truncate(limit);
        discussed_chunks.sort_by(|a, b| b.3.cmp(&a.3));
        discussed_chunks.truncate(limit);

        // 5. Format output
        let person_label = if !name.is_empty() { name } else { email };
        let period = if since_days == 0 {
            "all time".to_string()
        } else {
            format!("last {since_days} days")
        };

        let mut out = format!(
            "# Profile: {person_label}\n\
             Period: {period} | Sources: {} sent, {} addressed-to, {} discussed-about\n\n",
            sent_chunks.len(),
            addressed_chunks.len(),
            discussed_chunks.len(),
        );

        // Section 1: Messages sent BY this person (From:)
        if !sent_chunks.is_empty() {
            out.push_str(&format!(
                "## Messages sent by {person_label} ({} found)\n\n",
                sent_chunks.len()
            ));
            for (_, sk, sid, content, ts_ms) in &sent_chunks {
                let dt = chrono::DateTime::from_timestamp_millis(*ts_ms)
                    .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_default();
                let src_short = sid.split(':').last().unwrap_or(sid);
                let src_short = &src_short[..src_short.len().min(30)];
                out.push_str(&format!("**[{sk}]** {dt} (…{src_short})\n{content}\n\n"));
            }
        } else {
            out.push_str(&format!(
                "## Messages sent by {person_label}\n\nNone found in this period.\n\n"
            ));
        }

        // Section 2: Messages addressed TO this person (To:/CC:/Participants:)
        if !addressed_chunks.is_empty() {
            out.push_str(&format!(
                "## Messages addressed to {person_label} ({} found)\n\n\
                 _(emails/chats where {person_label} is in To, CC, or Participants)_\n\n",
                addressed_chunks.len()
            ));
            for (_, sk, sid, content, ts_ms) in &addressed_chunks {
                let dt = chrono::DateTime::from_timestamp_millis(*ts_ms)
                    .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_default();
                let src_short = sid.split(':').last().unwrap_or(sid);
                let src_short = &src_short[..src_short.len().min(30)];
                out.push_str(&format!("**[{sk}]** {dt} (…{src_short})\n{content}\n\n"));
            }
        }

        // Section 3: Messages discussing this person (body mentions, not From/To/CC)
        if !discussed_chunks.is_empty() {
            out.push_str(&format!(
                "## Others discussing {person_label} ({} found)\n\n\
                 _(content where {person_label} is mentioned but is not sender or recipient)_\n\n",
                discussed_chunks.len()
            ));
            for (sk, sid, content, ts_ms) in &discussed_chunks {
                let dt = chrono::DateTime::from_timestamp_millis(*ts_ms)
                    .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_default();
                let src_short = sid.split(':').last().unwrap_or(sid);
                let src_short = &src_short[..src_short.len().min(30)];
                out.push_str(&format!("**[{sk}]** {dt} (…{src_short})\n{content}\n\n"));
            }
        }

        // Source breakdown
        let mut breakdown: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (_, sk, ..) in &sent_chunks {
            *breakdown.entry(sk.clone()).or_insert(0) += 1;
        }
        for (_, sk, ..) in &addressed_chunks {
            *breakdown.entry(format!("{sk} (to)")).or_insert(0) += 1;
        }
        for (sk, ..) in &discussed_chunks {
            *breakdown.entry(format!("{sk} (discussed)")).or_insert(0) += 1;
        }
        if !breakdown.is_empty() {
            out.push_str("## Source breakdown\n");
            let mut bk: Vec<_> = breakdown.iter().collect();
            bk.sort_by(|a, b| b.1.cmp(a.1));
            for (src, count) in bk {
                out.push_str(&format!("- {src}: {count}\n"));
            }
        }

        Ok(out)
    })
    .map_err(|e| anyhow::anyhow!("profile_person DB error: {e}"))
}

// ---------------------------------------------------------------------------
// Contact directory helpers
// ---------------------------------------------------------------------------

struct ContactEntry {
    display_name: String,
    email: String,
}

/// Look up a contact by partial name from the mem_tree_contacts table.
/// Rebuilds the table first if it's empty or stale.
fn lookup_contact_by_name(config: &Config, name: &str) -> anyhow::Result<ContactEntry> {
    with_connection(config, |conn| {
        // Rebuild contacts if empty
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mem_tree_contacts", [], |r| r.get(0))
            .unwrap_or(0);
        if count == 0 {
            rebuild_contacts(conn)?;
        }

        let name_lower = name.to_lowercase();
        let parts: Vec<&str> = name_lower.split_whitespace().collect();

        // Try exact "Last, First" match first
        if parts.len() >= 2 {
            let last = parts.last().unwrap();
            let first = parts.first().unwrap();
            let pattern = format!("{last}, {first}%");
            if let Ok(row) = conn.query_row(
                "SELECT display_name, COALESCE(email,'') FROM mem_tree_contacts \
                 WHERE display_name_lower LIKE ?1 ORDER BY mention_count DESC LIMIT 1",
                [&pattern],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            ) {
                return Ok(ContactEntry {
                    display_name: row.0,
                    email: row.1,
                });
            }
        }

        // Try any part of the name
        let pattern = format!("%{name_lower}%");
        if let Ok(row) = conn.query_row(
            "SELECT display_name, COALESCE(email,'') FROM mem_tree_contacts \
             WHERE display_name_lower LIKE ?1 ORDER BY mention_count DESC LIMIT 1",
            [&pattern],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ) {
            return Ok(ContactEntry {
                display_name: row.0,
                email: row.1,
            });
        }

        Err(anyhow::anyhow!("no contact found for name={name}"))
    })
    .map_err(|e| anyhow::anyhow!("lookup_contact: {e}"))
}

/// Rebuild mem_tree_contacts from chunk content.
/// Extracts [From: Display Name] and [email:addr] patterns from chat/email chunks.
fn rebuild_contacts(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    use std::collections::HashMap;

    log::info!("[profile_person] rebuilding mem_tree_contacts...");

    // Extract [From: Name] patterns from chunk content
    let mut contacts: HashMap<String, (String, i64, i64)> = HashMap::new(); // display_name → (email, count, last_seen_ms)

    let mut stmt = conn.prepare(
        "SELECT content, source_kind, timestamp_ms FROM mem_tree_chunks \
         WHERE source_kind IN ('chat','email') AND content LIKE '%[From: %'",
    )?;

    let rows: Vec<(String, String, i64)> = stmt
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2).unwrap_or(0)))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (content, _source_kind, ts_ms) in &rows {
        // Extract [From: XYZ] where XYZ is not an email address
        let mut pos = 0;
        while let Some(start) = content[pos..].find("[From: ") {
            let abs_start = pos + start + 7; // after "[From: "
            if let Some(end) = content[abs_start..].find(']') {
                let candidate = content[abs_start..abs_start + end].trim();
                if !candidate.contains('@') && candidate.len() > 3 && candidate.len() < 60 {
                    let entry =
                        contacts
                            .entry(candidate.to_string())
                            .or_insert(("".to_string(), 0, 0));
                    entry.1 += 1;
                    if *ts_ms > entry.2 {
                        entry.2 = *ts_ms;
                    }
                }
                pos = abs_start + end + 1;
            } else {
                break;
            }
        }
    }

    // Upsert into mem_tree_contacts
    let tx = conn.unchecked_transaction()?;
    for (display_name, (email, count, last_seen_ms)) in &contacts {
        tx.execute(
            "INSERT INTO mem_tree_contacts (display_name, display_name_lower, email, source_kind, mention_count, last_seen_ms) \
             VALUES (?1, ?2, ?3, 'mixed', ?4, ?5) \
             ON CONFLICT(display_name, source_kind) DO UPDATE SET \
               mention_count = mention_count + ?4, \
               last_seen_ms = MAX(last_seen_ms, ?5)",
            rusqlite::params![
                display_name,
                display_name.to_lowercase(),
                if email.is_empty() { None } else { Some(email) },
                count,
                last_seen_ms,
            ],
        )?;
    }
    tx.commit()?;

    log::info!(
        "[profile_person] rebuilt mem_tree_contacts: {} entries",
        contacts.len()
    );
    Ok(())
}
