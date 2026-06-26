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
        schema("mcp_chrome_status"),
        schema("set_aha_token"),
        schema("clear_aha_token"),
        schema("refresh_sharepoint"),
        schema("open_in_chrome"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController { schema: schema("token_status"), handler: handle_token_status },
        RegisteredController { schema: schema("auth_login"), handler: handle_auth_login },
        RegisteredController { schema: schema("auth_refresh"), handler: handle_auth_refresh },
        RegisteredController { schema: schema("auth_logout"), handler: handle_auth_logout },
        RegisteredController { schema: schema("mcp_chrome_status"), handler: handle_mcp_chrome_status },
        RegisteredController { schema: schema("set_aha_token"), handler: handle_set_aha_token },
        RegisteredController { schema: schema("clear_aha_token"), handler: handle_clear_aha_token },
        RegisteredController { schema: schema("refresh_sharepoint"), handler: handle_refresh_sharepoint },
        RegisteredController { schema: schema("open_in_chrome"), handler: handle_open_in_chrome },
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
        "mcp_chrome_status" => ControllerSchema {
            namespace: "m365",
            function: "mcp_chrome_status",
            description:
                "Check whether the mcp-chrome browser extension is reachable on port 12306. \
                          Returns { ok, port, error? }.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "{ ok: bool, port: number, error?: string }",
                required: true,
            }],
        },
        "set_aha_token" => ControllerSchema {
            namespace: "m365",
            function: "set_aha_token",
            description: "Save an Aha! API token for sap.aha.io access.",
            inputs: vec![FieldSchema {
                name: "token",
                ty: TypeSchema::String,
                comment: "Aha! personal API token from sap.aha.io/settings/api_keys.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "{ ok: true, saved: true }",
                required: true,
            }],
        },
        "clear_aha_token" => ControllerSchema {
            namespace: "m365",
            function: "clear_aha_token",
            description: "Remove the stored Aha! API token.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "{ ok: true, cleared: true }",
                required: true,
            }],
        },
        "refresh_sharepoint" => ControllerSchema {
            namespace: "m365",
            function: "refresh_sharepoint",
            description: "Re-exchange the Teams refresh token for a fresh SharePoint (sap.sharepoint.com) access token.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "{ ok: true }",
                required: true,
            }],
        },
        "open_in_chrome" => ControllerSchema {
            namespace: "m365",
            function: "open_in_chrome",
            description: "Open a URL in Chrome via mcp-chrome (for SSO login to Jira, Confluence, etc.).",
            inputs: vec![FieldSchema {
                name: "url",
                ty: TypeSchema::String,
                comment: "URL to open in Chrome.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "{ ok: bool, data?: { sessionId } }",
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

fn handle_mcp_chrome_status(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let result = ops::mcp_chrome_status().await;
        to_json(RpcOutcome {
            value: result,
            logs: vec![],
        })
    })
}

fn handle_set_aha_token(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let token = params
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        ops::set_aha_token(&token, &config)
            .await
            .map_err(|e| e.to_string())?;
        to_json(RpcOutcome {
            value: serde_json::json!({ "ok": true, "saved": true }),
            logs: vec![],
        })
    })
}

fn handle_clear_aha_token(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        ops::clear_aha_token(&config)
            .await
            .map_err(|e| e.to_string())?;
        to_json(RpcOutcome {
            value: serde_json::json!({ "ok": true, "cleared": true }),
            logs: vec![],
        })
    })
}

fn handle_refresh_sharepoint(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        ops::refresh_sharepoint(&config)
            .await
            .map_err(|e| e.to_string())?;
        to_json(RpcOutcome {
            value: serde_json::json!({ "ok": true }),
            logs: vec![],
        })
    })
}

fn handle_open_in_chrome(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let url = params.get("url").and_then(Value::as_str).unwrap_or("").to_string();
        let result = ops::open_in_chrome(&url).await.map_err(|e| e.to_string())?;
        to_json(RpcOutcome { value: result, logs: vec![] })
    })
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}
