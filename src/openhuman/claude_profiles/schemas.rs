//! RPC controller schemas + handlers for the `claude_profiles` domain.
//!
//! Thin delegators to [`super::ops`]. RPC method names auto-derive to
//! `openhuman.claude_profiles_<function>`.

use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;

use super::ops;
use super::types::CreateProfileInput;

fn profiles_output() -> FieldSchema {
    FieldSchema {
        name: "profiles",
        ty: TypeSchema::Json,
        comment: "Array of {profile:{id,name,path}, models:{opus?,sonnet?,haiku?,default?}, readable}. Never contains auth tokens.",
        required: true,
    }
}

fn profile_output() -> FieldSchema {
    FieldSchema {
        name: "profile",
        ty: TypeSchema::Json,
        comment: "{profile:{id,name,path}, models:{opus?,sonnet?,haiku?,default?}, readable}. Never contains auth tokens.",
        required: true,
    }
}

/// Schema definitions for every function in this namespace.
pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list_profiles" => ControllerSchema {
            namespace: "claude_profiles",
            function: "list_profiles",
            description: "List registered Claude Code settings profiles with models parsed from each settings.json (no secrets).",
            inputs: vec![],
            outputs: vec![profiles_output()],
        },
        "get_profile" => ControllerSchema {
            namespace: "claude_profiles",
            function: "get_profile",
            description: "Get one settings profile (with parsed models) by id.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Profile id.",
                required: true,
            }],
            outputs: vec![profile_output()],
        },
        "add_profile" => ControllerSchema {
            namespace: "claude_profiles",
            function: "add_profile",
            description: "Register a Claude Code settings.json file as a profile. Parses its model tiers; stores even if currently unreadable.",
            inputs: vec![
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::String,
                    comment: "User-facing label for the profile.",
                    required: true,
                },
                FieldSchema {
                    name: "path",
                    ty: TypeSchema::String,
                    comment: "Absolute path to a Claude Code settings.json.* file.",
                    required: true,
                },
            ],
            outputs: vec![profile_output()],
        },
        "remove_profile" => ControllerSchema {
            namespace: "claude_profiles",
            function: "remove_profile",
            description: "Remove a registered settings profile by id.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Profile id to remove.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "removed",
                ty: TypeSchema::Bool,
                comment: "True if a profile was removed.",
                required: true,
            }],
        },
        "preview_models" => ControllerSchema {
            namespace: "claude_profiles",
            function: "preview_models",
            description: "Parse the model tiers at a settings.json path WITHOUT registering it (live preview). Never returns secrets.",
            inputs: vec![FieldSchema {
                name: "path",
                ty: TypeSchema::String,
                comment: "Absolute path to a Claude Code settings.json.* file.",
                required: true,
            }],
            outputs: vec![
                FieldSchema {
                    name: "models",
                    ty: TypeSchema::Json,
                    comment: "{opus?,sonnet?,haiku?,default?} parsed from the file's env block. No tokens.",
                    required: true,
                },
                FieldSchema {
                    name: "readable",
                    ty: TypeSchema::Bool,
                    comment: "True if the file exists and is readable.",
                    required: true,
                },
            ],
        },
        "get_ladder" => ControllerSchema {
            namespace: "claude_profiles",
            function: "get_ladder",
            description: "Get the global fallback ladder (ordered (profile,tier) steps, resolved to models). Empty stored ladder auto-prefills from registered profiles. No secrets.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "ladder",
                ty: TypeSchema::Json,
                comment: "Ordered array of {profile_id, profile_name, tier, model?, readable}.",
                required: true,
            }],
        },
        "set_ladder" => ControllerSchema {
            namespace: "claude_profiles",
            function: "set_ladder",
            description: "Persist a new fallback ladder order (array of {profile_id, tier}). Overwrites wholesale.",
            inputs: vec![FieldSchema {
                name: "steps",
                ty: TypeSchema::Json,
                comment: "Ordered array of {profile_id, tier} objects.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "ok",
                ty: TypeSchema::Bool,
                comment: "True on success.",
                required: true,
            }],
        },
        "get_global_fallback" => ControllerSchema {
            namespace: "claude_profiles",
            function: "get_global_fallback",
            description: "Get the global default fallback policy applied to tasks that have no profile of their own. No secrets.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "global_fallback",
                ty: TypeSchema::Json,
                comment: "{enabled, start_profile?, start_tier?, direction?, end?}.",
                required: true,
            }],
        },
        "set_global_fallback" => ControllerSchema {
            namespace: "claude_profiles",
            function: "set_global_fallback",
            description: "Persist the global default fallback policy for tasks without a profile. Overwrites wholesale.",
            inputs: vec![FieldSchema {
                name: "global_fallback",
                ty: TypeSchema::Json,
                comment: "{enabled, start_profile?, start_tier?, direction?, end?}.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "ok",
                ty: TypeSchema::Bool,
                comment: "True on success.",
                required: true,
            }],
        },
        other => panic!("unknown claude_profiles function: {other}"),
    }
}

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list_profiles"),
        schemas("get_profile"),
        schemas("add_profile"),
        schemas("remove_profile"),
        schemas("preview_models"),
        schemas("get_ladder"),
        schemas("set_ladder"),
        schemas("get_global_fallback"),
        schemas("set_global_fallback"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list_profiles"),
            handler: handle_list_profiles,
        },
        RegisteredController {
            schema: schemas("get_profile"),
            handler: handle_get_profile,
        },
        RegisteredController {
            schema: schemas("add_profile"),
            handler: handle_add_profile,
        },
        RegisteredController {
            schema: schemas("remove_profile"),
            handler: handle_remove_profile,
        },
        RegisteredController {
            schema: schemas("preview_models"),
            handler: handle_preview_models,
        },
        RegisteredController {
            schema: schemas("get_ladder"),
            handler: handle_get_ladder,
        },
        RegisteredController {
            schema: schemas("set_ladder"),
            handler: handle_set_ladder,
        },
        RegisteredController {
            schema: schemas("get_global_fallback"),
            handler: handle_get_global_fallback,
        },
        RegisteredController {
            schema: schemas("set_global_fallback"),
            handler: handle_set_global_fallback,
        },
    ]
}

fn handle_list_profiles(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let profiles = ops::list_profiles(&config);
        serde_json::to_value(serde_json::json!({ "profiles": profiles })).map_err(|e| e.to_string())
    })
}

fn handle_get_profile(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("id is required")?
            .to_string();
        let config = config_rpc::load_config_with_timeout().await?;
        match ops::get_profile(&config, &id) {
            Some(profile) => serde_json::to_value(serde_json::json!({ "profile": profile }))
                .map_err(|e| e.to_string()),
            None => Err(format!("no profile with id {id}")),
        }
    })
}

fn handle_add_profile(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("name is required")?
            .to_string();
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("path is required")?
            .to_string();
        let config = config_rpc::load_config_with_timeout().await?;
        let profile = ops::add_profile(&config, CreateProfileInput { name, path })?;
        serde_json::to_value(serde_json::json!({ "profile": profile })).map_err(|e| e.to_string())
    })
}

fn handle_remove_profile(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("id is required")?
            .to_string();
        let config = config_rpc::load_config_with_timeout().await?;
        let removed = ops::remove_profile(&config, &id)?;
        Ok(serde_json::json!({ "removed": removed }))
    })
}

fn handle_preview_models(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("path is required")?
            .to_string();
        let (models, readable) = ops::preview_models(&path);
        serde_json::to_value(serde_json::json!({ "models": models, "readable": readable }))
            .map_err(|e| e.to_string())
    })
}

fn handle_get_ladder(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let ladder = ops::get_ladder(&config);
        serde_json::to_value(serde_json::json!({ "ladder": ladder })).map_err(|e| e.to_string())
    })
}

fn handle_set_ladder(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let steps: Vec<super::types::LadderStep> = params
            .get("steps")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| format!("invalid steps: {e}"))?
            .ok_or("steps is required")?;
        let config = config_rpc::load_config_with_timeout().await?;
        ops::set_ladder(&config, steps)?;
        Ok(serde_json::json!({ "ok": true }))
    })
}

fn handle_get_global_fallback(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let gf = ops::get_global_fallback(&config);
        serde_json::to_value(serde_json::json!({ "global_fallback": gf }))
            .map_err(|e| e.to_string())
    })
}

fn handle_set_global_fallback(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let gf: super::types::GlobalFallback = params
            .get("global_fallback")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| format!("invalid global_fallback: {e}"))?
            .ok_or("global_fallback is required")?;
        let config = config_rpc::load_config_with_timeout().await?;
        ops::set_global_fallback(&config, gf)?;
        Ok(serde_json::json!({ "ok": true }))
    })
}
