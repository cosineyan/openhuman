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
        if !ctx.sender.to_lowercase().contains(&sender_pat.to_lowercase()) {
            return false;
        }
    }
    if let Some(subject_pat) = &rule.subject_contains {
        if !ctx.subject.to_lowercase().contains(&subject_pat.to_lowercase()) {
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
fn render_template_with_vars(template: &str, ctx: &EmailContext, vars: &serde_json::Value) -> String {
    let mut result = render_template(template, ctx);
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
    // Keep temp file alive until after execution
    let _ = tmp.flush();

    let output = match std::process::Command::new("python3")
        .arg(&tmp_path)
        .arg(email_body)
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
            log::debug!("[email_automation] parse_script returned: {}", stdout.trim());
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
) -> Result<Task, String> {
    let input = CreateTaskInput {
        title: title.to_string(),
        description: description.map(str::to_string),
        bucket_id: bucket_id.map(str::to_string),
        priority: None,
        due_date: None,
        parent_task_id: None,
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
            // Run parse_script if present to extract email-specific variables
            let vars = if let Some(script) = &rule.parse_script {
                let body = &ctx.body_preview;
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
            ) {
                Ok(_) => {
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
                );
            }
        });
    }

    None
}

// ---------------------------------------------------------------------------
// LLM fallback
// ---------------------------------------------------------------------------

async fn llm_classify_email(config: &Config, ctx: &EmailContext) -> Option<(String, Option<String>)> {
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

pub async fn run_now(config: Arc<Config>, last_n: usize) -> Result<RpcOutcome<RunNowResult>, String> {
    use std::collections::HashSet;

    let config_ref = &*config;
    let chunks = list_chunks(
        config_ref,
        &ListChunksQuery {
            source_kind: Some(SourceKind::Email),
            limit: Some(last_n * 5),
            ..ListChunksQuery::default()
        },
    )
    .map_err(|e| format!("list_chunks: {e}"))?;

    let mut seen_sources: HashSet<String> = HashSet::new();
    let mut emails_scanned = 0usize;
    let mut hits: Vec<RuleHit> = Vec::new();

    for chunk in chunks {
        if !seen_sources.insert(chunk.metadata.source_id.clone()) {
            continue;
        }
        emails_scanned += 1;

        let ctx = extract_email_context(&chunk.content);
        if let Some(hit) = process_email(config_ref, &ctx) {
            hits.push(hit);
        }
    }

    let tasks_created = hits.len();
    log::info!(
        "[email_automation] run_now scanned={emails_scanned} created={tasks_created}"
    );

    Ok(RpcOutcome::single_log(
        RunNowResult { emails_scanned, tasks_created, hits },
        format!("email_automation run_now: scanned={emails_scanned} created={tasks_created}"),
    ))
}

// ---------------------------------------------------------------------------
// RpcOutcome wrappers
// ---------------------------------------------------------------------------

pub fn list_rules_rpc(config: &Config) -> Result<RpcOutcome<Vec<EmailAutomationRule>>, String> {
    let rules = store::list_rules(config).map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(rules, "email_automation: list_rules"))
}

pub fn create_rule_rpc(config: &Config, input: CreateRuleInput) -> Result<RpcOutcome<EmailAutomationRule>, String> {
    let rule = store::create_rule(config, input).map_err(|e| e.to_string())?;
    log::info!("[email_automation] rule created id={}", rule.id);
    Ok(RpcOutcome::single_log(rule, "email_automation: create_rule"))
}

pub fn update_rule_rpc(config: &Config, id: &str, patch: RulePatch) -> Result<RpcOutcome<EmailAutomationRule>, String> {
    let rule = store::update_rule(config, id, patch).map_err(|e| e.to_string())?;
    log::info!("[email_automation] rule updated id={id}");
    Ok(RpcOutcome::single_log(rule, "email_automation: update_rule"))
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
pub fn search_email_chunks_rpc(
    config: &Config,
    sender_filter: Option<&str>,
    subject_filter: Option<&str>,
    limit: usize,
) -> Result<RpcOutcome<Vec<EmailChunkSummary>>, String> {
    use crate::openhuman::memory_store::content::read::read_chunk_body;

    let fetch_limit = (limit * 8).max(150);
    let chunks = list_chunks(
        config,
        &ListChunksQuery {
            source_kind: Some(SourceKind::Email),
            limit: Some(fetch_limit),
            ..ListChunksQuery::default()
        },
    )
    .map_err(|e| format!("list_chunks: {e}"))?;

    let sender_lower = sender_filter.map(|s| s.to_lowercase());
    let subject_lower = subject_filter.map(|s| s.to_lowercase());

    let mut results: Vec<EmailChunkSummary> = Vec::new();
    let mut seen_sources: std::collections::HashSet<String> = std::collections::HashSet::new();

    for chunk in chunks {
        if results.len() >= limit {
            break;
        }
        // Deduplicate by source_id (message ID) — each email is one source
        if !seen_sources.insert(chunk.metadata.source_id.clone()) {
            continue;
        }

        // Read the full body to get the [Subject:] [From:] prefix
        let body = match read_chunk_body(config, &chunk.id) {
            Ok(b) => b,
            Err(_) => {
                // Fallback: use content preview (may already have [Subject:] prefix)
                chunk.content.clone()
            }
        };

        let ctx = extract_email_context(&body);

        // Skip chunks where we couldn't extract any identifying info
        if ctx.subject.is_empty() && ctx.sender.is_empty() {
            continue;
        }

        // Apply filters
        if let Some(ref sf) = sender_lower {
            if !ctx.sender.to_lowercase().contains(sf.as_str()) {
                continue;
            }
        }
        if let Some(ref sf) = subject_lower {
            if !ctx.subject.to_lowercase().contains(sf.as_str()) {
                continue;
            }
        }

        // Extract date from body prefix
        let date = extract_bracketed(&ctx.body_preview, "Date: ")
            .and_then(|d| d.get(..10).map(str::to_string))
            .unwrap_or_default();

        // Extract body preview (first non-prefix line)
        let preview = body
            .lines()
            .find(|l| !l.trim_start().starts_with('[') && !l.trim().is_empty())
            .unwrap_or("")
            .chars()
            .take(120)
            .collect::<String>();

        results.push(EmailChunkSummary {
            chunk_id: chunk.id,
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
// Generate rule suggestion from a specific email chunk
// ---------------------------------------------------------------------------

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

    let (provider, model) = create_chat_provider("chat", config)
        .map_err(|e| format!("create_chat_provider: {e}"))?;

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
        name: json.get("name").and_then(|v| v.as_str()).unwrap_or("Auto-generated rule").to_string(),
        enabled: true,
        sender_contains: json.get("sender_contains").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string),
        subject_contains: json.get("subject_contains").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string),
        body_contains: None,
        task_title_template: json.get("task_title_template").and_then(|v| v.as_str()).unwrap_or("Task: {{subject}}").to_string(),
        task_description_template: json.get("task_description_template").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string),
        assignee: "ai".to_string(),
        bucket_id: None,
        llm_fallback_enabled: false,
        parse_script: json.get("parse_script").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string),
    };

    log::info!("[email_automation] generated rule from {} email(s): {:?}", email_count, suggestion.name);

    Ok(RpcOutcome::single_log(suggestion, "email_automation: generate_rule_from_emails"))
}
