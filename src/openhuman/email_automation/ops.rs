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
    CreateRuleInput, EmailAutomationRule, EmailContext, RuleHit, RulePatch, RunNowResult,
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
            let title = render_template(&rule.task_title_template, ctx);
            let description = rule
                .task_description_template
                .as_deref()
                .map(|t| render_template(t, ctx));

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
