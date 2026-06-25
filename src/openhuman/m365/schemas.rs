use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::rpc::RpcOutcome;

use super::ops;

// ---------------------------------------------------------------------------
// Public registry entry points
// ---------------------------------------------------------------------------

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schema("token_status"),
        schema("auth_login"),
        schema("auth_refresh"),
        schema("auth_logout"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schema("token_status"),
            handler: handle_token_status,
        },
        RegisteredController {
            schema: schema("auth_login"),
            handler: handle_auth_login,
        },
        RegisteredController {
            schema: schema("auth_refresh"),
            handler: handle_auth_refresh,
        },
        RegisteredController {
            schema: schema("auth_logout"),
            handler: handle_auth_logout,
        },
    ]
}

// ---------------------------------------------------------------------------
// Schema definitions
// ---------------------------------------------------------------------------

pub fn schema(function: &str) -> ControllerSchema {
    match function {
        "token_status" => ControllerSchema {
            namespace: "m365",
            function: "token_status",
            description: "Return the validity and remaining lifetime of cached M365 tokens \
                          (graph, rest, teams). Does not trigger any network calls.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "status",
                ty: TypeSchema::Json,
                comment:
                    "{ ok, graph: { valid, cached, expiresInMin }, rest: {...}, teams: {...} }",
                required: true,
            }],
        },
        "auth_login" => ControllerSchema {
            namespace: "m365",
            function: "auth_login",
            description: "Extract M365 tokens from Chrome (finds an open Outlook/Teams tab or \
                          opens one). Returns updated token status on success.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "status",
                ty: TypeSchema::Json,
                comment: "Same shape as token_status output.",
                required: true,
            }],
        },
        "auth_refresh" => ControllerSchema {
            namespace: "m365",
            function: "auth_refresh",
            description: "Force re-extract M365 tokens even if they have not expired yet.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "status",
                ty: TypeSchema::Json,
                comment: "Same shape as token_status output.",
                required: true,
            }],
        },
        "auth_logout" => ControllerSchema {
            namespace: "m365",
            function: "auth_logout",
            description: "Clear all cached M365 tokens from disk.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "{ ok: true }",
                required: true,
            }],
        },
        _ => ControllerSchema {
            namespace: "m365",
            function: "unknown",
            description: "Unknown m365 controller function.",
            inputs: vec![],
            outputs: vec![],
        },
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn handle_token_status(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        let result = ops::token_status(&config)
            .await
            .map_err(|e| e.to_string())?;
        to_json(RpcOutcome {
            value: result,
            logs: vec![],
        })
    })
}

fn handle_auth_login(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        let result = ops::auth_login(&config).await.map_err(|e| e.to_string())?;
        to_json(RpcOutcome {
            value: result,
            logs: vec![],
        })
    })
}

fn handle_auth_refresh(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        let result = ops::auth_refresh(&config)
            .await
            .map_err(|e| e.to_string())?;
        to_json(RpcOutcome {
            value: result,
            logs: vec![],
        })
    })
}

fn handle_auth_logout(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        ops::auth_logout(&config).await.map_err(|e| e.to_string())?;
        to_json(RpcOutcome {
            value: serde_json::json!({ "ok": true }),
            logs: vec![],
        })
    })
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}
