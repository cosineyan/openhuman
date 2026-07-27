use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::rpc::RpcOutcome;

use super::ops;
use super::types::{CreateRuleInput, RulePatch};

const NAMESPACE: &str = "email_automation";

fn to_json<T: Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    let val = serde_json::json!({
        "result": serde_json::to_value(&outcome.value).map_err(|e| e.to_string())?,
        "logs": outcome.logs,
    });
    Ok(val)
}

fn schemas(function: &'static str) -> ControllerSchema {
    match function {
        "list_rules" => ControllerSchema {
            namespace: NAMESPACE,
            function: "list_rules",
            description: "List all email-to-task automation rules.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "rules",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("EmailAutomationRule"))),
                comment: "All rules ordered by creation time.",
                required: true,
            }],
        },
        "create_rule" => ControllerSchema {
            namespace: NAMESPACE,
            function: "create_rule",
            description: "Create a new email-to-task rule.",
            inputs: vec![
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::String,
                    comment: "Human-readable rule name.",
                    required: true,
                },
                FieldSchema {
                    name: "task_title_template",
                    ty: TypeSchema::String,
                    comment: "Task title template. Supports {{subject}}, {{sender}}, {{body_preview}}.",
                    required: true,
                },
                FieldSchema {
                    name: "sender_contains",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Match sender (case-insensitive substring).",
                    required: false,
                },
                FieldSchema {
                    name: "subject_contains",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Match subject (case-insensitive substring).",
                    required: false,
                },
                FieldSchema {
                    name: "body_contains",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Match body preview (case-insensitive substring).",
                    required: false,
                },
                FieldSchema {
                    name: "assignee",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Task assignee: 'ai' (default) or 'me'.",
                    required: false,
                },
                FieldSchema {
                    name: "enabled",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Bool)),
                    comment: "Whether the rule is enabled (default true).",
                    required: false,
                },
                FieldSchema {
                    name: "task_description_template",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Task description template.",
                    required: false,
                },
                FieldSchema {
                    name: "parse_script",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Python parse script for extracting variables from email body.",
                    required: false,
                },
                FieldSchema {
                    name: "llm_fallback_enabled",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Bool)),
                    comment: "If true, LLM decides whether to create a task when no rule matches.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "rule",
                ty: TypeSchema::Ref("EmailAutomationRule"),
                comment: "The created rule.",
                required: true,
            }],
        },
        "update_rule" => ControllerSchema {
            namespace: NAMESPACE,
            function: "update_rule",
            description: "Update an existing email-to-task rule (partial patch).",
            inputs: vec![
                FieldSchema { name: "id", ty: TypeSchema::String, comment: "Rule id.", required: true },
                FieldSchema { name: "name", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Rule name.", required: false },
                FieldSchema { name: "enabled", ty: TypeSchema::Option(Box::new(TypeSchema::Bool)), comment: "Enable/disable.", required: false },
                FieldSchema { name: "sender_contains", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Sender filter.", required: false },
                FieldSchema { name: "subject_contains", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Subject filter.", required: false },
                FieldSchema { name: "body_contains", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Body filter.", required: false },
                FieldSchema { name: "task_title_template", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Title template.", required: false },
                FieldSchema { name: "task_description_template", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Description template.", required: false },
                FieldSchema { name: "assignee", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Task assignee.", required: false },
                FieldSchema { name: "bucket_id", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Bucket override.", required: false },
                FieldSchema { name: "parse_script", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Python parse script.", required: false },
            ],
            outputs: vec![FieldSchema {
                name: "rule",
                ty: TypeSchema::Ref("EmailAutomationRule"),
                comment: "The updated rule.",
                required: true,
            }],
        },
        "delete_rule" => ControllerSchema {
            namespace: NAMESPACE,
            function: "delete_rule",
            description: "Delete a rule by id.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Rule id to delete.",
                required: true,
            }],
            outputs: vec![],
        },
        "run_now" => ControllerSchema {
            namespace: NAMESPACE,
            function: "run_now",
            description: "Manually scan recent emails and apply all enabled rules.",
            inputs: vec![
                FieldSchema {
                    name: "last_n",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Number of emails to scan (default 50). Ignored when hours is set.",
                    required: false,
                },
                FieldSchema {
                    name: "hours",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Scan all emails received in the last N hours (e.g. 24). Takes priority over last_n.",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "emails_scanned",
                    ty: TypeSchema::U64,
                    comment: "Number of distinct emails evaluated.",
                    required: true,
                },
                FieldSchema {
                    name: "tasks_created",
                    ty: TypeSchema::U64,
                    comment: "Number of tasks created.",
                    required: true,
                },
            ],
        },
        "search_email_chunks" => ControllerSchema {
            namespace: NAMESPACE,
            function: "search_email_chunks",
            description: "List recent email chunks for the rule picker, optionally filtered by sender or subject keyword.",
            inputs: vec![
                FieldSchema {
                    name: "sender_filter",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Case-insensitive substring match on sender name/email.",
                    required: false,
                },
                FieldSchema {
                    name: "subject_filter",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Case-insensitive substring match on subject.",
                    required: false,
                },
                FieldSchema {
                    name: "limit",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Max results (default 20).",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "chunks",
                ty: TypeSchema::Json,
                comment: "List of EmailChunkSummary objects.",
                required: true,
            }],
        },
        "generate_rule_from_email" => ControllerSchema {
            namespace: NAMESPACE,
            function: "generate_rule_from_email",
            description: "Use LLM to generate a rule suggestion from a specific email chunk.",
            inputs: vec![FieldSchema {
                name: "chunk_id",
                ty: TypeSchema::String,
                comment: "Chunk id of the email to analyze.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "rule",
                ty: TypeSchema::Json,
                comment: "Suggested CreateRuleInput fields.",
                required: true,
            }],
        },
        "generate_rule_from_emails" => ControllerSchema {
            namespace: NAMESPACE,
            function: "generate_rule_from_emails",
            description: "Use LLM to generate a rule suggestion from multiple email chunks (reduces overfitting).",
            inputs: vec![FieldSchema {
                name: "chunk_ids",
                ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                comment: "List of chunk ids to analyze together.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "rule",
                ty: TypeSchema::Json,
                comment: "Suggested CreateRuleInput fields.",
                required: true,
            }],
        },
        "dry_run" => ControllerSchema {
            namespace: NAMESPACE,
            function: "dry_run",
            description: "Preview what a rule would generate for a given email body.",
            inputs: vec![
                FieldSchema { name: "task_title_template", ty: TypeSchema::String, comment: "Title template to test.", required: true },
                FieldSchema { name: "task_description_template", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Description template.", required: false },
                FieldSchema { name: "parse_script", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Python parse script.", required: false },
                FieldSchema { name: "email_body", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Raw email body.", required: false },
                FieldSchema { name: "chunk_id", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Chunk id to use as email body.", required: false },
            ],
            outputs: vec![FieldSchema { name: "result", ty: TypeSchema::Json, comment: "DryRunResult.", required: true }],
        },
        "refine_rule" => ControllerSchema {
            namespace: NAMESPACE,
            function: "refine_rule",
            description: "Refine an existing rule using LLM based on user feedback.",
            inputs: vec![
                FieldSchema { name: "task_title_template", ty: TypeSchema::String, comment: "Current title template.", required: true },
                FieldSchema { name: "task_description_template", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Current description template.", required: false },
                FieldSchema { name: "parse_script", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Current parse script.", required: false },
                FieldSchema { name: "email_body", ty: TypeSchema::String, comment: "Email body used for context.", required: true },
                FieldSchema { name: "user_feedback", ty: TypeSchema::String, comment: "User instructions for improvement.", required: true },
            ],
            outputs: vec![FieldSchema { name: "rule", ty: TypeSchema::Json, comment: "Improved CreateRuleInput.", required: true }],
        },
        other => panic!("unknown email_automation schema function: {other}"),
    }
}

fn parse_value<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params))
        .map_err(|e| format!("invalid email_automation params: {e}"))
}

// ── Handlers ────────────────────────────────────────────────────────────────

fn handle_list_rules(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        to_json(ops::list_rules_rpc(&config)?)
    })
}

fn handle_create_rule(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let input: CreateRuleInput = parse_value(params)?;
        to_json(ops::create_rule_rpc(&config, input)?)
    })
}

fn handle_update_rule(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("missing id")?
            .to_string();
        let mut patch_params = params;
        patch_params.remove("id");
        let patch: RulePatch = parse_value(patch_params)?;
        to_json(ops::update_rule_rpc(&config, &id, patch)?)
    })
}

fn handle_delete_rule(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("missing id")?
            .to_string();
        to_json(ops::delete_rule_rpc(&config, &id)?)
    })
}

fn handle_run_now(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let last_n = params.get("last_n").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
        let hours = params.get("hours").and_then(|v| v.as_u64());
        to_json(ops::run_now(Arc::new(config), last_n, hours).await?)
    })
}

fn handle_search_email_chunks(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let sender_filter = params
            .get("sender_filter")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let subject_filter = params
            .get("subject_filter")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;
        to_json(ops::search_email_chunks_rpc(
            &config,
            sender_filter.as_deref(),
            subject_filter.as_deref(),
            limit,
        )?)
    })
}

fn handle_generate_rule_from_email(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let chunk_id = params
            .get("chunk_id")
            .and_then(|v| v.as_str())
            .ok_or("missing chunk_id")?
            .to_string();
        to_json(ops::generate_rule_from_email_rpc(&config, &chunk_id).await?)
    })
}

fn handle_generate_rule_from_emails(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let chunk_ids: Vec<String> = params
            .get("chunk_ids")
            .and_then(|v| v.as_array())
            .ok_or("missing chunk_ids")?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        to_json(ops::generate_rule_from_emails_rpc(&config, &chunk_ids).await?)
    })
}

fn handle_dry_run(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let task_title_template = params
            .get("task_title_template")
            .and_then(|v| v.as_str())
            .ok_or("missing task_title_template")?
            .to_string();
        let task_description_template = params
            .get("task_description_template")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let parse_script = params
            .get("parse_script")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let email_body = params
            .get("email_body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let chunk_id = params
            .get("chunk_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        to_json(ops::dry_run_rpc(
            &config,
            &task_title_template,
            task_description_template.as_deref(),
            parse_script.as_deref(),
            &email_body,
            chunk_id.as_deref(),
        )?)
    })
}

fn handle_refine_rule(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let task_title_template = params.get("task_title_template").and_then(|v| v.as_str()).ok_or("missing task_title_template")?.to_string();
        let task_description_template = params.get("task_description_template").and_then(|v| v.as_str()).map(str::to_string);
        let parse_script = params.get("parse_script").and_then(|v| v.as_str()).map(str::to_string);
        let email_body = params.get("email_body").and_then(|v| v.as_str()).ok_or("missing email_body")?.to_string();
        let user_feedback = params.get("user_feedback").and_then(|v| v.as_str()).ok_or("missing user_feedback")?.to_string();
        to_json(ops::refine_rule_rpc(
            &config,
            &task_title_template,
            task_description_template.as_deref(),
            parse_script.as_deref(),
            &email_body,
            &user_feedback,
        ).await?)
    })
}

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list_rules"),
        schemas("create_rule"),
        schemas("update_rule"),
        schemas("delete_rule"),
        schemas("run_now"),
        schemas("search_email_chunks"),
        schemas("generate_rule_from_email"),
        schemas("generate_rule_from_emails"),
        schemas("dry_run"),
        schemas("refine_rule"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController { schema: schemas("list_rules"),                  handler: handle_list_rules                  },
        RegisteredController { schema: schemas("create_rule"),                 handler: handle_create_rule                 },
        RegisteredController { schema: schemas("update_rule"),                 handler: handle_update_rule                 },
        RegisteredController { schema: schemas("delete_rule"),                 handler: handle_delete_rule                 },
        RegisteredController { schema: schemas("run_now"),                     handler: handle_run_now                     },
        RegisteredController { schema: schemas("search_email_chunks"),         handler: handle_search_email_chunks         },
        RegisteredController { schema: schemas("generate_rule_from_email"),    handler: handle_generate_rule_from_email    },
        RegisteredController { schema: schemas("generate_rule_from_emails"),   handler: handle_generate_rule_from_emails   },
        RegisteredController { schema: schemas("dry_run"),                     handler: handle_dry_run                     },
        RegisteredController { schema: schemas("refine_rule"),                 handler: handle_refine_rule                 },
    ]
}
