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
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Rule id.",
                required: true,
            }],
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
            inputs: vec![FieldSchema {
                name: "last_n",
                ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                comment: "Number of emails to scan (default 50).",
                required: false,
            }],
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
        let last_n = params
            .get("last_n")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;
        to_json(ops::run_now(Arc::new(config), last_n).await?)
    })
}

// ── Registry ────────────────────────────────────────────────────────────────

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list_rules"),
        schemas("create_rule"),
        schemas("update_rule"),
        schemas("delete_rule"),
        schemas("run_now"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController { schema: schemas("list_rules"),   handler: handle_list_rules   },
        RegisteredController { schema: schemas("create_rule"),  handler: handle_create_rule  },
        RegisteredController { schema: schemas("update_rule"),  handler: handle_update_rule  },
        RegisteredController { schema: schemas("delete_rule"),  handler: handle_delete_rule  },
        RegisteredController { schema: schemas("run_now"),      handler: handle_run_now      },
    ]
}
