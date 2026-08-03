use std::sync::Arc;

use crate::openhuman::config::Config;
use crate::openhuman::memory_store::chunks::store::{list_chunks, ListChunksQuery};
use crate::openhuman::memory_store::chunks::types::SourceKind;
use crate::openhuman::projects::{
    create_task as projects_create_task, update_task as projects_update_task, CreateTaskInput,
    Task, TaskPatch,
};
use crate::rpc::RpcOutcome;

use super::store;
use super::types::{
    CreateRuleInput, EmailAutomationRule, EmailChunkSummary, EmailContext, RuleHit, RulePatch,
    RunNowResult,
};

// ---------------------------------------------------------------------------
// Email context extraction
// ---------------------------------------------------------------------------

/// Extract subject and sender from the m365 body_preview prefix.
/// The OutlookMailReader always prefixes email bodies with:
/// `[Subject: {subject}] [From: {name} <{email}>] [Date: {date}]\n{body}`
pub fn extract_email_context(body_preview: &str) -> EmailContext {
    let subject = extract_bracketed(body_preview, "Subject: ").unwrap_or_default();
    let sender = extract_bracketed(body_preview, "From: ").unwrap_or_default();
    EmailContext {
        subject,
        sender,
        body_preview: body_preview.to_string(),
        full_body: body_preview.to_string(),
        chunk_id: String::new(),
        source_id: String::new(),
    }
}

fn extract_bracketed(text: &str, prefix: &str) -> Option<String> {
    let needle = format!("[{prefix}");
    let start = text.find(&needle)?;
    let inner_start = start + needle.len();
    let inner = &text[inner_start..];
    let end = inner.find(']')?;
    Some(inner[..end].trim().to_string())
}

// ---------------------------------------------------------------------------
// Rule evaluation
// ---------------------------------------------------------------------------

pub fn evaluate_rule(rule: &EmailAutomationRule, ctx: &EmailContext) -> bool {
    if let Some(sender_pat) = &rule.sender_contains {
        if !ctx
            .sender
            .to_lowercase()
            .contains(&sender_pat.to_lowercase())
        {
            return false;
        }
    }
    if let Some(subject_pat) = &rule.subject_contains {
        if !ctx
            .subject
            .to_lowercase()
            .contains(&subject_pat.to_lowercase())
        {
            return false;
        }
    }
    if let Some(body_pat) = &rule.body_contains {
        if !ctx
            .body_preview
            .to_lowercase()
            .contains(&body_pat.to_lowercase())
        {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Template rendering
// ---------------------------------------------------------------------------

fn render_template(template: &str, ctx: &EmailContext) -> String {
    template
        .replace("{{subject}}", &ctx.subject)
        .replace("{{sender}}", &ctx.sender)
        .replace(
            "{{body_preview}}",
            &ctx.body_preview.chars().take(200).collect::<String>(),
        )
}

/// Apply variables from parse_script output to a template.
/// Variables are substituted as {{key}} in the template.
fn render_template_with_vars(
    template: &str,
    ctx: &EmailContext,
    vars: &serde_json::Value,
) -> String {
    let mut result = render_template(template, ctx);
    // Add chunk_id as a built-in variable for linking back to the original email
    if !ctx.chunk_id.is_empty() {
        result = result.replace("{{chunk_id}}", &ctx.chunk_id);
    }
    if let Some(obj) = vars.as_object() {
        for (key, val) in obj {
            let placeholder = format!("{{{{{}}}}}", key);
            let value = match val {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(arr) => arr
                    .iter()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &value);
        }
    }
    result
}

/// Run the rule's parse_script against the email body using python3 subprocess.
/// Returns a JSON Value with extracted variables, or Null on failure.
fn run_parse_script(script: &str, email_body: &str) -> serde_json::Value {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Write script to a temp file
    let mut tmp = match tempfile::NamedTempFile::new() {
        Ok(f) => f,
        Err(e) => {
            log::warn!("[email_automation] parse_script: failed to create tempfile: {e}");
            return serde_json::Value::Null;
        }
    };
    if let Err(e) = tmp.write_all(script.as_bytes()) {
        log::warn!("[email_automation] parse_script: failed to write script: {e}");
        return serde_json::Value::Null;
    }
    let tmp_path = tmp.path().to_path_buf();
    let _ = tmp_path; // kept for lifetime (tempfile must stay alive)
    let _ = tmp.flush();

    // Write email_body to a separate temp file and pass its path as sys.argv[1].
    // This avoids shell argument length limits and quoting issues with HTML bodies.
    let mut body_tmp = match tempfile::NamedTempFile::new() {
        Ok(f) => f,
        Err(e) => {
            log::warn!("[email_automation] parse_script: failed to create body tempfile: {e}");
            return serde_json::Value::Null;
        }
    };
    if let Err(e) = body_tmp.write_all(email_body.as_bytes()) {
        log::warn!("[email_automation] parse_script: failed to write body tempfile: {e}");
        return serde_json::Value::Null;
    }
    let body_tmp_path = body_tmp.path().to_path_buf();
    let _ = body_tmp.flush();

    // Bootstrap wrapper: reads body from file into sys.argv[1], then exec()s the real script.
    // Using exec() keeps the user script in its own scope so there are no name conflicts.
    let script_path_str = tmp_path.to_string_lossy();
    let body_path_str = body_tmp_path.to_string_lossy();
    let wrapper = format!(
        r#"import sys as _sys, runpy as _rp
_sys.argv = ['{script_path}', open('{body_path}', encoding='utf-8').read()]
_rp.run_path('{script_path}', run_name='__main__')
"#,
        script_path = script_path_str.replace('\'', "\\'"),
        body_path = body_path_str.replace('\'', "\\'"),
    );

    let mut wrapper_tmp = match tempfile::NamedTempFile::new() {
        Ok(f) => f,
        Err(e) => {
            log::warn!("[email_automation] parse_script: failed to create wrapper tempfile: {e}");
            return serde_json::Value::Null;
        }
    };
    if let Err(e) = wrapper_tmp.write_all(wrapper.as_bytes()) {
        log::warn!("[email_automation] parse_script: failed to write wrapper: {e}");
        return serde_json::Value::Null;
    }
    let wrapper_path = wrapper_tmp.path().to_path_buf();
    let _ = wrapper_tmp.flush();

    let output = match Command::new("python3")
        .arg(&wrapper_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            log::warn!("[email_automation] parse_script: python3 exec failed: {e}");
            return serde_json::Value::Null;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("[email_automation] parse_script stderr: {stderr}");
        return serde_json::Value::Null;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str(stdout.trim()) {
        Ok(v) => {
            log::info!(
                "[email_automation] parse_script returned: {}",
                &stdout.trim()[..stdout.trim().len().min(500)]
            );
            v
        }
        Err(e) => {
            log::warn!("[email_automation] parse_script: JSON parse failed: {e}\nOutput: {stdout}");
            serde_json::Value::Null
        }
    }
}

// ---------------------------------------------------------------------------
// Task creation
// ---------------------------------------------------------------------------

/// Create a task for a matched rule.
/// Two-step: create (no assignee) → update to set assignee, which fires
/// DomainEvent::ProjectTaskAssignedToAi when assignee = "ai".
pub fn create_task_from_rule(
    config: &Config,
    title: &str,
    description: Option<&str>,
    assignee: &str,
    bucket_id: Option<&str>,
    settings_profile: Option<&str>,
    model: Option<&str>,
    fallback_direction: Option<&str>,
    fallback_end: Option<&str>,
) -> Result<Task, String> {
    let input = CreateTaskInput {
        title: title.to_string(),
        description: description.map(str::to_string),
        bucket_id: bucket_id.map(str::to_string),
        priority: None,
        due_date: None,
        parent_task_id: None,
        settings_profile: settings_profile.map(str::to_string),
        model: model.map(str::to_string),
        fallback_direction: fallback_direction.map(str::to_string),
        fallback_end: fallback_end.map(str::to_string),
    };
    let outcome = projects_create_task(config, input, "email_automation")?;
    let task = outcome.value;

    // Set assignee — fires DomainEvent::ProjectTaskAssignedToAi when "ai"
    if !assignee.is_empty() {
        let patch = TaskPatch {
            assignee: Some(Some(assignee.to_string())),
            ..TaskPatch::default()
        };
        let _ = projects_update_task(config, &task.id, patch, "email_automation")?;
    }

    Ok(task)
}

// ---------------------------------------------------------------------------
// Process a single email (called from bus and run_now)
// ---------------------------------------------------------------------------

pub fn process_email(config: &Config, ctx: &EmailContext) -> Option<RuleHit> {
    let rules = match store::list_enabled_rules(config) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[email_automation] load rules failed: {e}");
            return None;
        }
    };

    for rule in &rules {
        if evaluate_rule(rule, ctx) {
            // Check if this email has already been processed by this rule
            if !ctx.source_id.is_empty()
                && store::is_email_processed(config, &ctx.source_id, &rule.id)
            {
                log::debug!(
                    "[email_automation] skipping duplicate: source_id='{}' rule='{}'",
                    &ctx.source_id[..ctx.source_id.len().min(30)],
                    rule.name
                );
                return None;
            }

            // Batch mode: enqueue and defer task creation
            if rule.batch_mode {
                let body = if !ctx.full_body.is_empty() {
                    &ctx.full_body
                } else {
                    &ctx.body_preview
                };
                match store::enqueue_batch_email(config, &rule.id, &ctx.source_id, body) {
                    Ok(_) => log::info!(
                        "[email_automation] rule '{}' batch-queued source_id='{}'",
                        rule.name,
                        &ctx.source_id[..ctx.source_id.len().min(30)]
                    ),
                    Err(e) => log::warn!("[email_automation] enqueue_batch_email failed: {e}"),
                }
                return Some(RuleHit {
                    rule_id: rule.id.clone(),
                    rule_name: rule.name.clone(),
                    task_title: format!("[batch queued] {}", rule.name),
                });
            }

            // Run parse_script if present to extract email-specific variables
            // Use full_body if available (has complete email content), fall back to body_preview
            let vars = if let Some(script) = &rule.parse_script {
                let body = if !ctx.full_body.is_empty() {
                    &ctx.full_body
                } else {
                    &ctx.body_preview
                };
                let v = run_parse_script(script, body);
                log::debug!("[email_automation] parse_script vars: {:?}", v);
                v
            } else {
                serde_json::Value::Null
            };

            let title = render_template_with_vars(&rule.task_title_template, ctx, &vars);
            let description = rule
                .task_description_template
                .as_deref()
                .map(|t| render_template_with_vars(t, ctx, &vars));

            log::info!(
                "[email_automation] rule '{}' matched subject='{}' → creating task '{}'",
                rule.name,
                ctx.subject,
                title
            );

            match create_task_from_rule(
                config,
                &title,
                description.as_deref(),
                &rule.assignee,
                rule.bucket_id.as_deref(),
                rule.settings_profile.as_deref(),
                rule.model.as_deref(),
                rule.fallback_direction.as_deref(),
                rule.fallback_end.as_deref(),
            ) {
                Ok(task) => {
                    if !ctx.source_id.is_empty() {
                        if let Err(e) =
                            store::mark_email_processed(config, &ctx.source_id, &rule.id, &task.id)
                        {
                            log::warn!("[email_automation] mark_email_processed failed: {e}");
                        }
                        // Move email to ai-processed folder (best-effort, async)
                        if rule.assignee == "ai" {
                            let config_mv = config.clone();
                            let source_id_mv = ctx.source_id.clone();
                            tokio::spawn(async move {
                                move_email_to_ai_processed(&config_mv, &source_id_mv).await;
                            });
                        }
                    }
                    return Some(RuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        task_title: title,
                    });
                }
                Err(e) => {
                    log::warn!("[email_automation] create_task failed: {e}");
                }
            }
        }
    }

    // LLM fallback — only when no rule matched and at least one rule has it enabled
    if rules.iter().any(|r| r.llm_fallback_enabled) {
        let config_clone = config.clone();
        let ctx_clone = ctx.clone();
        tokio::spawn(async move {
            if let Some((title, desc)) = llm_classify_email(&config_clone, &ctx_clone).await {
                log::info!(
                    "[email_automation] LLM fallback task '{}' for subject='{}'",
                    title,
                    ctx_clone.subject
                );
                let _ = create_task_from_rule(
                    &config_clone,
                    &title,
                    desc.as_deref(),
                    "ai",
                    None,
                    None,
                    None,
                    None,
                    None,
                );
            }
        });
    }

    None
}

// ---------------------------------------------------------------------------
// LLM fallback
// ---------------------------------------------------------------------------

async fn llm_classify_email(
    config: &Config,
    ctx: &EmailContext,
) -> Option<(String, Option<String>)> {
    use crate::openhuman::inference::provider::factory::create_chat_provider;
    use crate::openhuman::inference::provider::traits::{ChatMessage, ChatRequest};

    let (provider, model) = create_chat_provider("chat", config).ok()?;

    let prompt = format!(
        "You are an email assistant. Given the following email, decide if the recipient needs to take an action that warrants creating a task.\n\
         Subject: {}\n\
         Sender: {}\n\
         Preview: {}\n\n\
         If a task should be created, respond with JSON: {{\"create\": true, \"title\": \"<short task title>\", \"description\": \"<optional brief description>\"}}\n\
         If no task is needed, respond with JSON: {{\"create\": false}}\n\
         Respond with JSON only, no other text.",
        ctx.subject,
        ctx.sender,
        ctx.body_preview.chars().take(500).collect::<String>()
    );

    let messages = vec![ChatMessage::user(&prompt)];
    let request = ChatRequest {
        messages: &messages,
        tools: None,
        max_tokens: Some(256),
        stream: None,
        hint_thread_id: None,
    };

    let response = provider.chat(request, &model, 0.0).await.ok()?;
    let text = response.text?;

    let json: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    if json.get("create").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let title = json.get("title")?.as_str()?.to_string();
    let description = json
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some((title, description))
}

// ---------------------------------------------------------------------------
// Manual scan (run_now)
// ---------------------------------------------------------------------------

/// Fetch the full email body for a given source_id.
fn regex_replace_all_simple_html(html: &str) -> String {
    // Remove HTML tags, decode common entities
    let no_tags: String = {
        let mut result = String::with_capacity(html.len());
        let mut in_tag = false;
        for ch in html.chars() {
            match ch {
                '<' => {
                    in_tag = true;
                }
                '>' => {
                    in_tag = false;
                    result.push(' ');
                }
                _ if !in_tag => {
                    result.push(ch);
                }
                _ => {}
            }
        }
        result
    };
    no_tags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
/// Tries Graph API first (best quality), then chunk concatenation, then content fallback.
pub async fn fetch_full_email_body_pub(
    config: &Config,
    source_id: &str,
    content_fallback: &str,
) -> String {
    fetch_full_email_body(config, source_id, content_fallback).await
}

/// Move an email to the "ai-processed" folder in Outlook via Graph API.
/// Creates the folder if it doesn't exist. Best-effort: errors are logged but not propagated.
pub async fn move_email_to_ai_processed(config: &Config, source_id: &str) {
    use crate::openhuman::memory_sources::readers::m365::{
        graph_get, graph_post, read_graph_token_public,
    };

    // source_id format: mem_src:{src_id}:{message_id}
    let msg_id = {
        let parts: Vec<&str> = source_id.splitn(3, ':').collect();
        if parts.len() == 3 {
            let raw = parts[2];
            let decoded = urlencoding::decode(raw)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| raw.to_string());
            if !decoded.is_empty() {
                Some(decoded)
            } else {
                None
            }
        } else {
            None
        }
    };
    let Some(msg_id) = msg_id else {
        log::debug!(
            "[email_automation] move_email: cannot parse message_id from source_id={}",
            &source_id[..source_id.len().min(30)]
        );
        return;
    };

    let token = match read_graph_token_public(config) {
        Ok(t) if !t.is_empty() => t,
        _ => {
            log::debug!("[email_automation] move_email: no graph token");
            return;
        }
    };

    // Find or create "ai-processed" folder
    let folders_url = "https://graph.microsoft.com/v1.0/me/mailFolders?$top=50";
    let folder_id = match graph_get(&token, folders_url).await {
        Ok(data) => {
            let existing = data
                .get("value")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter().find(|f| {
                        f.get("displayName").and_then(|n| n.as_str()) == Some("ai-processed")
                    })
                })
                .and_then(|f| f.get("id").and_then(|v| v.as_str()).map(str::to_string));
            if let Some(id) = existing {
                id
            } else {
                // Create the folder
                let create_url = "https://graph.microsoft.com/v1.0/me/mailFolders";
                match graph_post(
                    &token,
                    create_url,
                    serde_json::json!({ "displayName": "ai-processed" }),
                )
                .await
                {
                    Ok(f) => match f.get("id").and_then(|v| v.as_str()).map(str::to_string) {
                        Some(id) => {
                            log::info!("[email_automation] created ai-processed folder");
                            id
                        }
                        None => {
                            log::warn!("[email_automation] move_email: created folder but no id in response");
                            return;
                        }
                    },
                    Err(e) => {
                        log::warn!("[email_automation] move_email: create folder failed: {e}");
                        return;
                    }
                }
            }
        }
        Err(e) => {
            log::warn!("[email_automation] move_email: list folders failed: {e}");
            return;
        }
    };

    // Move the message
    let move_url = format!(
        "https://graph.microsoft.com/v1.0/me/messages/{}/move",
        urlencoding::encode(&msg_id)
    );
    match graph_post(
        &token,
        &move_url,
        serde_json::json!({ "destinationId": folder_id }),
    )
    .await
    {
        Ok(_) => log::info!(
            "[email_automation] moved message {} to ai-processed",
            &msg_id[..msg_id.len().min(20)]
        ),
        Err(e) => log::warn!("[email_automation] move_email: move failed: {e}"),
    }
}

/// Format a Graph `message` JSON (with subject/body/from/receivedDateTime)
/// into the prefixed body string parse_script expects. Returns None when the
/// message has no body content.
fn format_graph_message(data: &serde_json::Value) -> Option<String> {
    let body_content = data
        .get("body")
        .and_then(|b| b.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if body_content.is_empty() {
        return None;
    }
    let content_type = data
        .get("body")
        .and_then(|b| b.get("contentType"))
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    log::info!(
        "[email_automation] fetch_full_email_body: content_type={} body_len={} contains_href_launchpad={}",
        content_type,
        body_content.len(),
        body_content.contains("launchpad")
    );
    let subject = data.get("subject").and_then(|v| v.as_str()).unwrap_or("");
    let from = data
        .get("from")
        .and_then(|f| f.get("emailAddress"))
        .map(|e| {
            let name = e.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let addr = e.get("address").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                addr.to_string()
            } else {
                format!("{name} <{addr}>")
            }
        })
        .unwrap_or_default();
    let date = data
        .get("receivedDateTime")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let prefix = format!("[Subject: {subject}] [From: {from}] [Date: {date}]\n");
    Some(format!("{prefix}{body_content}"))
}

/// Extract the `[Subject: …]` value from the ingested chunk fallback content,
/// used to re-resolve a stale message-id by searching Outlook.
fn extract_subject_from_content(content: &str) -> Option<String> {
    let start = content.find("[Subject:")? + "[Subject:".len();
    let rest = &content[start..];
    // Subject runs to the first "] [" delimiter (before [From:/[Date:) or "]".
    let end = rest.find("] [").or_else(|| rest.find(']'))?;
    let subject = rest[..end].trim();
    if subject.is_empty() {
        None
    } else {
        Some(subject.to_string())
    }
}

async fn fetch_full_email_body(config: &Config, source_id: &str, content_fallback: &str) -> String {
    use crate::openhuman::memory_sources::readers::m365::graph_get;

    let token = crate::openhuman::memory_sources::readers::m365::read_graph_token_public(config)
        .unwrap_or_default();

    // source_id format: mem_src:{src_id}:{message_id}
    // message_id can contain ':' so we split on the first two ':' only
    let msg_id = {
        let parts: Vec<&str> = source_id.splitn(3, ':').collect();
        if parts.len() == 3 {
            let raw = parts[2];
            let decoded = urlencoding::decode(raw)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| raw.to_string());
            if !decoded.is_empty() {
                Some(decoded)
            } else {
                None
            }
        } else {
            None
        }
    };

    if !token.is_empty() {
        // 1) Try the stored message-id directly.
        if let Some(msg_id) = msg_id {
            let url = format!(
                "https://graph.microsoft.com/v1.0/me/messages/{}?$select=subject,body,from,receivedDateTime,toRecipients",
                urlencoding::encode(&msg_id)
            );
            match graph_get(&token, &url).await {
                Ok(data) => {
                    if let Some(body) = format_graph_message(&data) {
                        return body;
                    }
                }
                Err(e) => {
                    // Stale id (moved/re-synced email → 404) is common; fall
                    // through to a subject search rather than the URL-less chunk.
                    log::info!(
                        "[email_automation] fetch_full_email_body: id fetch failed ({e}); trying subject re-resolve"
                    );
                }
            }
        }

        // 2) Re-resolve by subject search (handles stale/rotated message ids).
        //    Graph rejects `$search` combined with `$select` on messages (400),
        //    and returns the full message resource (incl. `body`) by default —
        //    so we omit `$select` here.
        if let Some(subject) = extract_subject_from_content(content_fallback) {
            // Graph $search requires the value quoted; strip embedded quotes.
            let q = subject.replace('"', " ");
            let url = format!(
                "https://graph.microsoft.com/v1.0/me/messages?$search=%22{}%22&$top=1",
                urlencoding::encode(&q)
            );
            match graph_get(&token, &url).await {
                Ok(data) => {
                    if let Some(first) = data
                        .get("value")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                    {
                        if let Some(body) = format_graph_message(first) {
                            log::info!(
                                "[email_automation] fetch_full_email_body: recovered via subject search ({} chars body)",
                                body.len()
                            );
                            return body;
                        }
                    }
                    log::info!(
                        "[email_automation] fetch_full_email_body: subject search returned no usable message"
                    );
                }
                Err(e) => {
                    log::warn!(
                        "[email_automation] fetch_full_email_body: subject search failed: {e}"
                    );
                }
            }
        }
    }

    // Fallback: concatenate all chunks (stripped/summarized — may lack URLs).
    use crate::openhuman::memory_store::chunks::store::with_connection;
    use anyhow::Context as _;
    with_connection(config, |conn| {
        let mut stmt = conn
            .prepare(
                "SELECT content FROM mem_tree_chunks \
             WHERE source_id=?1 AND source_kind='email' \
             ORDER BY seq_in_source ASC",
            )
            .context("prepare full body")?;
        let parts = stmt
            .query_map(rusqlite::params![source_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("query full body")?;
        Ok(parts.join(""))
    })
    .unwrap_or_else(|_| content_fallback.to_string())
}

pub async fn run_now(
    config: Arc<Config>,
    last_n: usize,
    hours: Option<u64>,
) -> Result<RpcOutcome<RunNowResult>, String> {
    use crate::openhuman::memory_store::chunks::store::with_connection;
    use anyhow::Context as _;

    let config_ref = &*config;

    let since_ms =
        hours.map(|h| (chrono::Utc::now() - chrono::Duration::hours(h as i64)).timestamp_millis());

    // Use direct SQL to get seq_in_source=0 chunks (one per email, with [Subject:][From:] prefix)
    let rows: Vec<(String, String, String, String)> = with_connection(config_ref, |conn| {
        let mut sql =
            "SELECT id, source_id, coalesce(content_path,''), content FROM mem_tree_chunks \
            WHERE source_kind='email' AND seq_in_source=0"
                .to_string();
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ms) = since_ms {
            sql.push_str(" AND timestamp_ms >= ?");
            bound.push(Box::new(ms));
        }
        sql.push_str(" ORDER BY timestamp_ms DESC LIMIT ?");
        let lim = if since_ms.is_some() {
            10_000i64
        } else {
            (last_n * 5) as i64
        };
        bound.push(Box::new(lim));

        let mut stmt = conn.prepare(&sql).context("prepare run_now")?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(bound.iter().map(|b| b.as_ref())),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("query run_now")?;
        Ok(rows)
    })
    .map_err(|e| e.to_string())?;

    let mut seen_sources: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut emails_scanned = 0usize;
    let mut hits: Vec<RuleHit> = Vec::new();

    for (chunk_id, source_id, content_path, content) in rows {
        if !seen_sources.insert(source_id.clone()) {
            continue;
        }
        emails_scanned += 1;

        // Phase 1: quick rule matching using content preview (no API call)
        let preview_ctx = {
            let mut c = extract_email_context(&content);
            c.source_id = source_id.clone();
            c.chunk_id = chunk_id.clone();
            c
        };

        // Check dedup first with preview ctx
        let rules = match store::list_enabled_rules(config_ref) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let matching_rule = rules.iter().find(|r| evaluate_rule(r, &preview_ctx));
        if matching_rule.is_none() {
            continue; // no rule matches — skip expensive body fetch
        }
        let matching_rule = matching_rule.unwrap();

        // Check dedup
        if store::is_email_processed(config_ref, &source_id, &matching_rule.id) {
            log::debug!(
                "[email_automation] run_now: skipping duplicate source_id={}",
                &source_id[..source_id.len().min(30)]
            );
            continue;
        }

        // Phase 2: fetch full body only for matching emails
        let full_body = if !content_path.is_empty() {
            match crate::openhuman::memory_store::content::read::read_chunk_body(
                config_ref, &chunk_id,
            ) {
                Ok(b) if b.len() > 500 => b,
                // read_chunk_body returned truncated/empty content — fall through to Graph API
                Ok(_) => fetch_full_email_body(config_ref, &source_id, &content).await,
                Err(_) => fetch_full_email_body(config_ref, &source_id, &content).await,
            }
        } else {
            fetch_full_email_body(config_ref, &source_id, &content).await
        };

        let mut ctx = extract_email_context(&full_body);
        ctx.full_body = full_body.clone();
        ctx.chunk_id = chunk_id.clone();
        ctx.source_id = source_id.clone();
        log::debug!(
            "[email_automation] run_now chunk={} sender='{}' subject='{}'",
            &chunk_id[..8.min(chunk_id.len())],
            ctx.sender,
            ctx.subject
        );
        if let Some(hit) = process_email(config_ref, &ctx) {
            hits.push(hit);
        }
    }

    let tasks_created = hits.len();
    log::info!(
        "[email_automation] run_now scanned={emails_scanned} created={tasks_created} hours={hours:?}"
    );

    Ok(RpcOutcome::single_log(
        RunNowResult {
            emails_scanned,
            tasks_created,
            hits,
        },
        format!("email_automation run_now: scanned={emails_scanned} created={tasks_created}"),
    ))
}

// ---------------------------------------------------------------------------
// RpcOutcome wrappers
// ---------------------------------------------------------------------------

pub fn list_processed_emails_rpc(
    config: &Config,
    limit: usize,
) -> Result<RpcOutcome<Vec<store::ProcessedEmailEntry>>, String> {
    let entries = store::list_processed_emails(config, limit).map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        entries,
        "email_automation: list_processed_emails",
    ))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct EmailContentResult {
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: String,
    pub body: String,
}

pub fn get_email_content_rpc(
    config: &Config,
    source_id: &str,
) -> Result<RpcOutcome<Option<EmailContentResult>>, String> {
    match store::get_email_for_display(config, source_id).map_err(|e| e.to_string())? {
        Some((subject, from, to, date, body)) => Ok(RpcOutcome::single_log(
            Some(EmailContentResult {
                subject,
                from,
                to,
                date,
                body,
            }),
            "email_automation: get_email_content",
        )),
        None => Ok(RpcOutcome::single_log(
            None,
            "email_automation: get_email_content not found",
        )),
    }
}

pub fn list_rules_rpc(config: &Config) -> Result<RpcOutcome<Vec<EmailAutomationRule>>, String> {
    let rules = store::list_rules(config).map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        rules,
        "email_automation: list_rules",
    ))
}

pub fn create_rule_rpc(
    config: &Config,
    input: CreateRuleInput,
) -> Result<RpcOutcome<EmailAutomationRule>, String> {
    let rule = store::create_rule(config, input).map_err(|e| e.to_string())?;
    log::info!("[email_automation] rule created id={}", rule.id);
    Ok(RpcOutcome::single_log(
        rule,
        "email_automation: create_rule",
    ))
}

pub fn update_rule_rpc(
    config: &Config,
    id: &str,
    patch: RulePatch,
) -> Result<RpcOutcome<EmailAutomationRule>, String> {
    let rule = store::update_rule(config, id, patch).map_err(|e| e.to_string())?;
    log::info!("[email_automation] rule updated id={id}");
    Ok(RpcOutcome::single_log(
        rule,
        "email_automation: update_rule",
    ))
}

pub fn delete_rule_rpc(config: &Config, id: &str) -> Result<RpcOutcome<serde_json::Value>, String> {
    store::delete_rule(config, id).map_err(|e| e.to_string())?;
    log::info!("[email_automation] rule deleted id={id}");
    Ok(RpcOutcome::single_log(
        serde_json::json!({ "deleted": id }),
        "email_automation: delete_rule",
    ))
}

// ---------------------------------------------------------------------------
// Email chunk search (for the "generate from email" picker)
// ---------------------------------------------------------------------------

/// List recent email chunks, optionally filtered by sender or subject keyword.
/// Uses direct SQL LIKE query on the content field for full coverage.
pub fn search_email_chunks_rpc(
    config: &Config,
    sender_filter: Option<&str>,
    subject_filter: Option<&str>,
    limit: usize,
) -> Result<RpcOutcome<Vec<EmailChunkSummary>>, String> {
    use crate::openhuman::memory_store::chunks::store::with_connection;
    use anyhow::Context as _;

    let sender_pat = sender_filter
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s.to_lowercase()));
    let subject_pat = subject_filter
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s.to_lowercase()));
    let sql_limit = (limit * 3) as i64;

    let rows: Vec<(String, String)> = with_connection(config, |conn| {
        let mut sql = "SELECT id, content FROM mem_tree_chunks \
            WHERE source_kind='email' AND seq_in_source=0"
            .to_string();
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref sp) = sender_pat {
            sql.push_str(" AND lower(content) LIKE ?");
            bound.push(Box::new(sp.clone()));
        }
        if let Some(ref sp) = subject_pat {
            sql.push_str(" AND lower(content) LIKE ?");
            bound.push(Box::new(sp.clone()));
        }
        sql.push_str(" ORDER BY timestamp_ms DESC LIMIT ?");
        bound.push(Box::new(sql_limit));

        let mut stmt = conn.prepare(&sql).context("prepare search_email_chunks")?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(bound.iter().map(|b| b.as_ref())),
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("query search_email_chunks")?;
        Ok(rows)
    })
    .map_err(|e| e.to_string())?;

    let mut results: Vec<EmailChunkSummary> = Vec::new();
    for (id, content) in rows {
        if results.len() >= limit {
            break;
        }
        let ctx = extract_email_context(&content);
        if ctx.subject.is_empty() && ctx.sender.is_empty() {
            continue;
        }

        let date = extract_bracketed(&ctx.body_preview, "Date: ")
            .and_then(|d| d.get(..10).map(str::to_string))
            .unwrap_or_default();
        let preview = content
            .lines()
            .find(|l| !l.trim_start().starts_with('[') && !l.trim().is_empty())
            .unwrap_or("")
            .chars()
            .take(120)
            .collect::<String>();

        results.push(EmailChunkSummary {
            chunk_id: id,
            subject: ctx.subject,
            sender: ctx.sender,
            date,
            preview,
        });
    }

    Ok(RpcOutcome::single_log(
        results,
        "email_automation: search_email_chunks",
    ))
}

// ---------------------------------------------------------------------------
// Dry run — preview what a rule would generate for a given email body
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DryRunResult {
    pub title: String,
    pub description: Option<String>,
    pub parsed_vars: serde_json::Value,
    pub script_error: Option<String>,
}

pub fn dry_run_rpc(
    config: &Config,
    task_title_template: &str,
    task_description_template: Option<&str>,
    parse_script: Option<&str>,
    email_body: &str,
    chunk_id: Option<&str>,
) -> Result<RpcOutcome<DryRunResult>, String> {
    use crate::openhuman::memory_store::content::read::read_chunk_body;

    // If chunk_id provided, read full body from disk
    let body_owned;
    let body = if let Some(cid) = chunk_id {
        match read_chunk_body(config, cid) {
            Ok(b) => {
                body_owned = b;
                &body_owned
            }
            Err(e) => {
                log::warn!("[email_automation] dry_run: read_chunk_body failed: {e}");
                body_owned = email_body.to_string();
                &body_owned
            }
        }
    } else {
        body_owned = email_body.to_string();
        &body_owned
    };

    let ctx = extract_email_context(body);

    let (vars, script_error) = if let Some(script) = parse_script {
        if script.trim().is_empty() {
            (serde_json::Value::Null, None)
        } else {
            let v = run_parse_script(script, body);
            if v.is_null() {
                (
                    serde_json::Value::Null,
                    Some(
                        "Parse script returned no output or failed. Check the script.".to_string(),
                    ),
                )
            } else {
                (v, None)
            }
        }
    } else {
        (serde_json::Value::Null, None)
    };

    let title = render_template_with_vars(task_title_template, &ctx, &vars);
    let description = task_description_template.map(|t| render_template_with_vars(t, &ctx, &vars));

    Ok(RpcOutcome::single_log(
        DryRunResult {
            title,
            description,
            parsed_vars: vars,
            script_error,
        },
        "email_automation: dry_run",
    ))
}

// ---------------------------------------------------------------------------
// Generate rule suggestion from a specific email chunk
// ---------------------------------------------------------------------------

pub async fn refine_rule_rpc(
    config: &Config,
    current_title_template: &str,
    current_description_template: Option<&str>,
    current_parse_script: Option<&str>,
    email_body: &str,
    user_feedback: &str,
) -> Result<RpcOutcome<CreateRuleInput>, String> {
    use crate::openhuman::inference::provider::factory::create_chat_provider;
    use crate::openhuman::inference::provider::traits::{ChatMessage, ChatRequest};

    let (provider, model) =
        create_chat_provider("chat", config).map_err(|e| format!("create_chat_provider: {e}"))?;

    let script_section = current_parse_script
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("\nCurrent parse_script:\n```python\n{s}\n```"))
        .unwrap_or_default();

    let desc_section = current_description_template
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("\nCurrent task_description_template:\n{s}"))
        .unwrap_or_default();

    let prompt = format!(
        "You are refining an email automation rule based on user feedback.\n\n\
         Email body:\n{email_body}\n\n\
         Current rule:\n\
         task_title_template: {current_title_template}{desc_section}{script_section}\n\n\
         User feedback: {user_feedback}\n\n\
         Apply the feedback and produce an improved rule. Keep what works, fix what the user pointed out.\n\
         Rules:\n\
         - task_title_template: use {{{{subject}}}}, {{{{sender}}}}, or {{{{var_name}}}} placeholders. No hard-coded names.\n\
         - task_description_template: same placeholder support.\n\
         - parse_script: Python script receiving email_body as sys.argv[1], printing JSON dict to stdout.\n\
         - Only include parse_script if variables are needed. If no variables, set to empty string.\n\n\
         Respond with JSON only:\n\
         {{\n\
           \"name\": \"<rule name>\",\n\
           \"sender_contains\": \"<sender keyword or empty>\",\n\
           \"subject_contains\": \"<subject keyword>\",\n\
           \"task_title_template\": \"<improved title>\",\n\
           \"task_description_template\": \"<improved description>\",\n\
           \"parse_script\": \"<improved Python script or empty string>\"\n\
         }}"
    );

    let messages = vec![ChatMessage::user(&prompt)];
    let request = ChatRequest {
        messages: &messages,
        tools: None,
        max_tokens: Some(2048),
        stream: None,
        hint_thread_id: None,
    };

    let response = provider
        .chat(request, &model, 0.0)
        .await
        .map_err(|e| format!("LLM call failed: {e}"))?;

    let text = response.text.unwrap_or_default();
    let json_str = if text.trim().starts_with("```") {
        text.trim()
            .lines()
            .skip(1)
            .take_while(|l| !l.starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text.trim().to_string()
    };

    let json: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("parse LLM response: {e}\nRaw: {json_str}"))?;

    let result = CreateRuleInput {
        name: json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Refined rule")
            .to_string(),
        enabled: true,
        sender_contains: json
            .get("sender_contains")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        subject_contains: json
            .get("subject_contains")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        body_contains: None,
        task_title_template: json
            .get("task_title_template")
            .and_then(|v| v.as_str())
            .unwrap_or(current_title_template)
            .to_string(),
        task_description_template: json
            .get("task_description_template")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        assignee: "ai".to_string(),
        bucket_id: None,
        llm_fallback_enabled: false,
        parse_script: json
            .get("parse_script")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        batch_mode: false,
        batch_window_secs: super::types::default_batch_window_secs(),
        batch_parse_mode: super::types::BatchParseMode::FirstOnly,
        settings_profile: None,
        model: None,
        fallback_direction: None,
        fallback_end: None,
    };

    log::info!("[email_automation] refined rule: {:?}", result.name);
    Ok(RpcOutcome::single_log(
        result,
        "email_automation: refine_rule",
    ))
}

pub async fn generate_rule_from_email_rpc(
    config: &Config,
    chunk_id: &str,
) -> Result<RpcOutcome<CreateRuleInput>, String> {
    generate_rule_from_emails_rpc(config, &[chunk_id.to_string()]).await
}

/// Generate a rule suggestion from multiple email chunks (reduces overfitting).
pub async fn generate_rule_from_emails_rpc(
    config: &Config,
    chunk_ids: &[String],
) -> Result<RpcOutcome<CreateRuleInput>, String> {
    use crate::openhuman::inference::provider::factory::create_chat_provider;
    use crate::openhuman::inference::provider::traits::{ChatMessage, ChatRequest};
    use crate::openhuman::memory_store::content::read::read_chunk_body;

    if chunk_ids.is_empty() {
        return Err("chunk_ids must not be empty".to_string());
    }

    // Read all selected emails
    let mut email_sections = Vec::new();
    for (i, chunk_id) in chunk_ids.iter().enumerate() {
        let body = read_chunk_body(config, chunk_id)
            .map_err(|e| format!("read_chunk_body({chunk_id}): {e}"))?;
        let ctx = extract_email_context(&body);
        email_sections.push(format!(
            "--- Email {} ---\nSubject: {}\nSender: {}\nBody:\n{}",
            i + 1,
            ctx.subject,
            ctx.sender,
            body.chars().take(1500).collect::<String>()
        ));
    }

    let emails_text = email_sections.join("\n\n");
    let email_count = chunk_ids.len();

    let (provider, model) =
        create_chat_provider("chat", config).map_err(|e| format!("create_chat_provider: {e}"))?;

    let prompt = format!(
        "Analyze these {email_count} email(s) to create an automation rule config.\n\n\
         {emails_text}\n\n\
         You must produce THREE things:\n\n\
         A) RULE TEMPLATES (generic — works for ALL future similar emails):\n\
            - task_title_template: use {{{{subject}}}}, {{{{sender}}}}, or {{{{var_name}}}} from parse_script. Never hard-code specific names.\n\
            - task_description_template: same placeholder support. No hard-coded names/IDs.\n\
            Example title: 'Leave approval needed: {{{{employees}}}}'\n\
            Example description: 'Approve leave for {{{{employees}}}}. Link: {{{{approval_url}}}}'\n\n\
         B) PARSE SCRIPT — a Python script that extracts variables from the email body:\n\
            - Receives email_body as sys.argv[1]\n\
            - Prints a JSON dict to stdout with extracted variables\n\
            - Variables match the {{{{var_name}}}} placeholders in templates above\n\
            - Must handle variations between emails of the same type\n\
            - Example variables: employees (list of 'Name (ID): request'), approval_url, mass_approval_url\n\
            - IMPORTANT: the script should handle the case where the email has multiple employees\n\n\
         C) SENDER/SUBJECT MATCHING PATTERNS:\n\
            - sender_contains: domain or keyword matching the TYPE, not a specific person\n\
            - subject_contains: keyword matching the email type\n\n\
         Respond with JSON only:\n\
         {{\n\
           \"name\": \"<short rule name>\",\n\
           \"sender_contains\": \"<sender domain or keyword>\",\n\
           \"subject_contains\": \"<subject keyword>\",\n\
           \"task_title_template\": \"<title with {{{{var_name}}}} placeholders>\",\n\
           \"task_description_template\": \"<description with {{{{var_name}}}} placeholders and URLs>\",\n\
           \"parse_script\": \"<complete Python script as a single string>\"\n\
         }}"
    );

    let messages = vec![ChatMessage::user(&prompt)];
    let request = ChatRequest {
        messages: &messages,
        tools: None,
        max_tokens: Some(2048),
        stream: None,
        hint_thread_id: None,
    };

    let response = provider
        .chat(request, &model, 0.0)
        .await
        .map_err(|e| format!("LLM call failed: {e}"))?;

    let text = response.text.unwrap_or_default();
    let text = text.trim();

    let json_str = if text.starts_with("```") {
        text.lines()
            .skip(1)
            .take_while(|l| !l.starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text.to_string()
    };

    let json: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("parse LLM response: {e}\nRaw: {json_str}"))?;

    let suggestion = CreateRuleInput {
        name: json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Auto-generated rule")
            .to_string(),
        enabled: true,
        sender_contains: json
            .get("sender_contains")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        subject_contains: json
            .get("subject_contains")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        body_contains: None,
        task_title_template: json
            .get("task_title_template")
            .and_then(|v| v.as_str())
            .unwrap_or("Task: {{subject}}")
            .to_string(),
        task_description_template: json
            .get("task_description_template")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        assignee: "ai".to_string(),
        bucket_id: None,
        llm_fallback_enabled: false,
        parse_script: json
            .get("parse_script")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        batch_mode: false,
        batch_window_secs: super::types::default_batch_window_secs(),
        batch_parse_mode: super::types::BatchParseMode::FirstOnly,
        settings_profile: None,
        model: None,
        fallback_direction: None,
        fallback_end: None,
    };

    log::info!(
        "[email_automation] generated rule from {} email(s): {:?}",
        email_count,
        suggestion.name
    );

    Ok(RpcOutcome::single_log(
        suggestion,
        "email_automation: generate_rule_from_emails",
    ))
}

// ---------------------------------------------------------------------------
// Batch queue drain
// ---------------------------------------------------------------------------

/// Drain all ready batch queues: for each rule with batch_mode=true, check if
/// the window has elapsed and if so create a combined task.
pub fn drain_batch_queue(config: &Config) {
    use super::types::BatchParseMode;

    let rules = match store::list_enabled_rules(config) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[email_automation] drain_batch_queue: list_rules failed: {e}");
            return;
        }
    };

    let batch_rules: Vec<_> = rules.into_iter().filter(|r| r.batch_mode).collect();
    if batch_rules.is_empty() {
        return;
    }

    let rule_ids = match store::list_batch_rule_ids(config) {
        Ok(ids) => ids,
        Err(e) => {
            log::warn!("[email_automation] drain_batch_queue: list_batch_rule_ids failed: {e}");
            return;
        }
    };

    for rule in &batch_rules {
        if !rule_ids.contains(&rule.id) {
            continue;
        }

        let entries = match store::pop_ready_batch_entries(config, &rule.id, rule.batch_window_secs)
        {
            Ok(e) => e,
            Err(e) => {
                log::warn!(
                    "[email_automation] drain_batch_queue: pop_ready failed rule={}: {e}",
                    rule.id
                );
                continue;
            }
        };
        if entries.is_empty() {
            continue;
        }

        log::info!(
            "[email_automation] drain_batch_queue: rule='{}' draining {} emails",
            rule.name,
            entries.len()
        );

        // Build a synthetic EmailContext from the first entry for template rendering
        let first_body = &entries[0].email_body;
        let first_ctx = extract_email_context(first_body);

        let vars = match rule.batch_parse_mode {
            BatchParseMode::FirstOnly => {
                if let Some(script) = &rule.parse_script {
                    run_parse_script(script, first_body)
                } else {
                    serde_json::Value::Null
                }
            }
            BatchParseMode::All => {
                // Run parse_script on every email, collect results into {{items}} array
                if let Some(script) = &rule.parse_script {
                    let item_list: Vec<serde_json::Value> = entries
                        .iter()
                        .map(|e| run_parse_script(script, &e.email_body))
                        .collect();
                    // Build vars with an "items" key containing the list as a formatted string
                    let items_str = item_list
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            // Format each item as "N. key1: val1 | key2: val2 ..."
                            if let Some(obj) = v.as_object() {
                                let fields: Vec<String> = obj
                                    .iter()
                                    .map(|(k, v)| {
                                        format!("{}: {}", k, v.as_str().unwrap_or(&v.to_string()))
                                    })
                                    .collect();
                                format!("{}. {}", i + 1, fields.join(" | "))
                            } else {
                                format!("{}. {}", i + 1, v)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    serde_json::json!({ "items": items_str, "count": entries.len() })
                } else {
                    // No parse_script: just list subjects
                    let items_str = entries
                        .iter()
                        .enumerate()
                        .map(|(i, e)| {
                            let ctx = extract_email_context(&e.email_body);
                            format!("{}. Subject: {} | From: {}", i + 1, ctx.subject, ctx.sender)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    serde_json::json!({ "items": items_str, "count": entries.len() })
                }
            }
        };

        // Inject count into vars
        let vars = if let serde_json::Value::Object(mut map) = vars {
            map.insert("count".to_string(), serde_json::json!(entries.len()));
            serde_json::Value::Object(map)
        } else {
            serde_json::json!({ "count": entries.len() })
        };

        let title = render_template_with_vars(&rule.task_title_template, &first_ctx, &vars);
        let description = rule
            .task_description_template
            .as_deref()
            .map(|t| render_template_with_vars(t, &first_ctx, &vars));

        match create_task_from_rule(
            config,
            &title,
            description.as_deref(),
            &rule.assignee,
            rule.bucket_id.as_deref(),
            rule.settings_profile.as_deref(),
            rule.model.as_deref(),
            rule.fallback_direction.as_deref(),
            rule.fallback_end.as_deref(),
        ) {
            Ok(task) => {
                // Mark all source_ids as processed
                for entry in &entries {
                    let _ =
                        store::mark_email_processed(config, &entry.source_id, &rule.id, &task.id);
                }
                // Move all emails to ai-processed folder (best-effort)
                if rule.assignee == "ai" {
                    let config_mv = config.clone();
                    let source_ids: Vec<String> =
                        entries.iter().map(|e| e.source_id.clone()).collect();
                    tokio::spawn(async move {
                        for source_id in source_ids {
                            move_email_to_ai_processed(&config_mv, &source_id).await;
                        }
                    });
                }
                // Remove from queue
                let ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
                if let Err(e) = store::delete_batch_entries(config, &ids) {
                    log::warn!(
                        "[email_automation] drain_batch_queue: delete_batch_entries failed: {e}"
                    );
                }
                log::info!(
                    "[email_automation] drain_batch_queue: created task '{}' for {} emails",
                    title,
                    entries.len()
                );
            }
            Err(e) => {
                log::warn!("[email_automation] drain_batch_queue: create_task failed: {e}");
            }
        }
    }
}
