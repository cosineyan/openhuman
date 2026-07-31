use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::rpc::RpcOutcome;

use super::ops;
use super::types::{CreateTopicInput, UpdateTopicPatch};

const NAMESPACE: &str = "topic_threads";

fn to_json<T: Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    let val = serde_json::json!({
        "result": serde_json::to_value(&outcome.value).map_err(|e| e.to_string())?,
        "logs": outcome.logs,
    });
    Ok(val)
}

fn parse_value<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params))
        .map_err(|e| format!("invalid topic_threads params: {e}"))
}

fn schemas(function: &'static str) -> ControllerSchema {
    match function {
        "list" => ControllerSchema {
            namespace: NAMESPACE,
            function: "list",
            description: "List all topic threads with their keywords, source pins, and entity pins.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "threads",
                ty: TypeSchema::Json,
                comment: "Array of TopicThreadDetail objects, newest first.",
                required: true,
            }],
        },
        "create" => ControllerSchema {
            namespace: NAMESPACE,
            function: "create",
            description: "Create a topic thread. Matching chunks are auto-aggregated into its backing tree.",
            inputs: vec![
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::String,
                    comment: "Human-readable topic name.",
                    required: true,
                },
                FieldSchema {
                    name: "description",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Free-text description of what the topic covers.",
                    required: false,
                },
                FieldSchema {
                    name: "keyword_logic",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "'or' (default) or 'and' — how keywords combine.",
                    required: false,
                },
                FieldSchema {
                    name: "keywords",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Keyword strings matched case-insensitively against chunk bodies.",
                    required: false,
                },
                FieldSchema {
                    name: "source_ids",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Pinned source ids — chunks from these are always included.",
                    required: false,
                },
                FieldSchema {
                    name: "entity_ids",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Pinned canonical entity ids (kind:surface) — chunks referencing them are included.",
                    required: false,
                },
                FieldSchema {
                    name: "meeting_names",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Pinned meeting-name substrings — transcripts whose [Meeting: X] contains one are included.",
                    required: false,
                },
                FieldSchema {
                    name: "backfill_days",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "If set (7/14/30), backfill matching historical chunks within N days after creating.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "thread",
                ty: TypeSchema::Json,
                comment: "The created TopicThreadDetail.",
                required: true,
            }],
        },
        "get" => ControllerSchema {
            namespace: NAMESPACE,
            function: "get",
            description: "Fetch a single topic thread by id.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Topic id.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "thread",
                ty: TypeSchema::Json,
                comment: "TopicThreadDetail, or null if not found.",
                required: false,
            }],
        },
        "update" => ControllerSchema {
            namespace: NAMESPACE,
            function: "update",
            description: "Update a topic thread (partial). List fields, when present, fully replace the stored set.",
            inputs: vec![
                FieldSchema {
                    name: "id",
                    ty: TypeSchema::String,
                    comment: "Topic id.",
                    required: true,
                },
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "New name.",
                    required: false,
                },
                FieldSchema {
                    name: "description",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "New description.",
                    required: false,
                },
                FieldSchema {
                    name: "keyword_logic",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "'or' or 'and'.",
                    required: false,
                },
                FieldSchema {
                    name: "keywords",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Replacement keyword set.",
                    required: false,
                },
                FieldSchema {
                    name: "source_ids",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Replacement source-pin set.",
                    required: false,
                },
                FieldSchema {
                    name: "entity_ids",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Replacement entity-pin set.",
                    required: false,
                },
                FieldSchema {
                    name: "meeting_names",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Replacement meeting-name-pin set.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "thread",
                ty: TypeSchema::Json,
                comment: "The updated TopicThreadDetail.",
                required: true,
            }],
        },
        "delete" => ControllerSchema {
            namespace: NAMESPACE,
            function: "delete",
            description: "Delete a topic thread by id. The backing summary tree is left intact.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Topic id to delete.",
                required: true,
            }],
            outputs: vec![],
        },
        "timeline" => ControllerSchema {
            namespace: NAMESPACE,
            function: "timeline",
            description: "Return the topic's summary timeline (highest level first) with full bodies.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Topic id.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "nodes",
                ty: TypeSchema::Json,
                comment: "Array of TopicTimelineNode objects.",
                required: true,
            }],
        },
        "discover_conversations" => ControllerSchema {
            namespace: NAMESPACE,
            function: "discover_conversations",
            description: "List Teams conversations (1:1 + group chats) recorded during sync, for the pin picker.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "conversations",
                ty: TypeSchema::Json,
                comment: "Array of TeamsConversation { conversation_id, source_id, label, chat_type, pin_value }.",
                required: true,
            }],
        },
        "discover_people" => ControllerSchema {
            namespace: NAMESPACE,
            function: "discover_people",
            description: "List person + email entities ranked by mention count, for the people picker.",
            inputs: vec![FieldSchema {
                name: "limit",
                ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                comment: "Max entities per kind (default 200).",
                required: false,
            }],
            outputs: vec![FieldSchema {
                name: "people",
                ty: TypeSchema::Json,
                comment: "Array of PersonEntity { entity_id, surface, kind, count }.",
                required: true,
            }],
        },
        "resolve_chat_link" => ControllerSchema {
            namespace: NAMESPACE,
            function: "resolve_chat_link",
            description: "Resolve a pasted Teams chat link into a conversation pin with a real label.",
            inputs: vec![FieldSchema {
                name: "url",
                ty: TypeSchema::String,
                comment: "A Teams chat deep link (contains a 19:...@thread.v2 conversation id).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "conversation",
                ty: TypeSchema::Json,
                comment: "TeamsConversation { conversation_id, source_id, label, chat_type, pin_value }.",
                required: true,
            }],
        },
        "discover_meetings" => ControllerSchema {
            namespace: NAMESPACE,
            function: "discover_meetings",
            description: "List distinct meeting names parsed from transcripts, for the meeting picker.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "meetings",
                ty: TypeSchema::Json,
                comment: "Array of MeetingInfo { meeting_name, count, last_seen_ms }.",
                required: true,
            }],
        },
        "backfill" => ControllerSchema {
            namespace: NAMESPACE,
            function: "backfill",
            description: "Scan historical chunks in the last N days and route matches into the topic.",
            inputs: vec![
                FieldSchema {
                    name: "id",
                    ty: TypeSchema::String,
                    comment: "Topic id.",
                    required: true,
                },
                FieldSchema {
                    name: "days",
                    ty: TypeSchema::U64,
                    comment: "Look-back window in days (e.g. 7, 14, 30).",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "BackfillResult { scanned, matched, enqueued }.",
                required: true,
            }],
        },
        other => panic!("unknown topic_threads schema function: {other}"),
    }
}

// ── Handlers ────────────────────────────────────────────────────────────────

fn handle_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        to_json(ops::list_threads_rpc(&config)?)
    })
}

fn handle_create(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let input: CreateTopicInput = parse_value(params)?;
        to_json(ops::create_thread_rpc(&config, input).await?)
    })
}

fn handle_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("missing id")?
            .to_string();
        to_json(ops::get_thread_rpc(&config, &id)?)
    })
}

fn handle_update(params: Map<String, Value>) -> ControllerFuture {
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
        let patch: UpdateTopicPatch = parse_value(patch_params)?;
        to_json(ops::update_thread_rpc(&config, &id, patch)?)
    })
}

fn handle_delete(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("missing id")?
            .to_string();
        to_json(ops::delete_thread_rpc(&config, &id)?)
    })
}

fn handle_timeline(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("missing id")?
            .to_string();
        to_json(ops::timeline_rpc(&config, &id)?)
    })
}

fn handle_discover_conversations(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        to_json(ops::discover_conversations_rpc(&config)?)
    })
}

fn handle_discover_people(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as u32;
        to_json(ops::discover_people_rpc(&config, limit).await?)
    })
}

fn handle_resolve_chat_link(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or("missing url")?
            .to_string();
        to_json(ops::resolve_chat_link_rpc(&config, &url).await?)
    })
}

fn handle_discover_meetings(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        to_json(ops::discover_meetings_rpc(&config)?)
    })
}

fn handle_backfill(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| e.to_string())?;
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("missing id")?
            .to_string();
        let days = params.get("days").and_then(|v| v.as_u64()).unwrap_or(14) as u32;
        to_json(ops::backfill_topic_rpc(&config, &id, days).await?)
    })
}

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("create"),
        schemas("get"),
        schemas("update"),
        schemas("delete"),
        schemas("timeline"),
        schemas("discover_conversations"),
        schemas("discover_people"),
        schemas("resolve_chat_link"),
        schemas("discover_meetings"),
        schemas("backfill"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list"),
            handler: handle_list,
        },
        RegisteredController {
            schema: schemas("create"),
            handler: handle_create,
        },
        RegisteredController {
            schema: schemas("get"),
            handler: handle_get,
        },
        RegisteredController {
            schema: schemas("update"),
            handler: handle_update,
        },
        RegisteredController {
            schema: schemas("delete"),
            handler: handle_delete,
        },
        RegisteredController {
            schema: schemas("timeline"),
            handler: handle_timeline,
        },
        RegisteredController {
            schema: schemas("discover_conversations"),
            handler: handle_discover_conversations,
        },
        RegisteredController {
            schema: schemas("discover_people"),
            handler: handle_discover_people,
        },
        RegisteredController {
            schema: schemas("resolve_chat_link"),
            handler: handle_resolve_chat_link,
        },
        RegisteredController {
            schema: schemas("discover_meetings"),
            handler: handle_discover_meetings,
        },
        RegisteredController {
            schema: schemas("backfill"),
            handler: handle_backfill,
        },
    ]
}
