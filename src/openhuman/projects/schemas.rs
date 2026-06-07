use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::projects::ops;
use crate::openhuman::projects::types::{BucketPatch, TaskPatch};
use crate::rpc::RpcOutcome;

// ---------------------------------------------------------------------------
// Public registry entry points
// ---------------------------------------------------------------------------

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("get_board"),
        schemas("create_task"),
        schemas("update_task"),
        schemas("move_task"),
        schemas("delete_task"),
        schemas("update_bucket"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("get_board"),
            handler: handle_get_board,
        },
        RegisteredController {
            schema: schemas("create_task"),
            handler: handle_create_task,
        },
        RegisteredController {
            schema: schemas("update_task"),
            handler: handle_update_task,
        },
        RegisteredController {
            schema: schemas("move_task"),
            handler: handle_move_task,
        },
        RegisteredController {
            schema: schemas("delete_task"),
            handler: handle_delete_task,
        },
        RegisteredController {
            schema: schemas("update_bucket"),
            handler: handle_update_bucket,
        },
    ]
}

// ---------------------------------------------------------------------------
// Schema definitions
// ---------------------------------------------------------------------------

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "get_board" => ControllerSchema {
            namespace: "projects",
            function: "get_board",
            description: "Return the full Kanban board for the default project — \
                          project metadata, all buckets, and tasks grouped by bucket.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "board",
                ty: TypeSchema::Json,
                comment: "BucketsWithTasks: { project, buckets: [{ bucket, tasks[] }] }.",
                required: true,
            }],
        },
        "create_task" => ControllerSchema {
            namespace: "projects",
            function: "create_task",
            description: "Create a new task in the default project. \
                          Defaults to the first bucket (To Do) when bucket_id is omitted.",
            inputs: vec![
                FieldSchema {
                    name: "title",
                    ty: TypeSchema::String,
                    comment: "Task title.",
                    required: true,
                },
                FieldSchema {
                    name: "description",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Optional longer description.",
                    required: false,
                },
                FieldSchema {
                    name: "bucket_id",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Target bucket id; defaults to the first bucket.",
                    required: false,
                },
                FieldSchema {
                    name: "priority",
                    ty: TypeSchema::Option(Box::new(TypeSchema::I64)),
                    comment: "Priority level (default 0).",
                    required: false,
                },
                FieldSchema {
                    name: "due_date",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Due date as RFC 3339 string (e.g. 2026-06-07T12:00:00Z).",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "task",
                ty: TypeSchema::Json,
                comment: "Newly created Task record.",
                required: true,
            }],
        },
        "update_task" => ControllerSchema {
            namespace: "projects",
            function: "update_task",
            description: "Apply a partial patch to an existing task.",
            inputs: vec![
                task_id_input("Identifier of the task to update."),
                FieldSchema {
                    name: "patch",
                    ty: TypeSchema::Json,
                    comment: "TaskPatch: any subset of { title, description, bucket_id, \
                              priority, due_date, hex_color, position, done }.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "task",
                ty: TypeSchema::Json,
                comment: "Updated Task record.",
                required: true,
            }],
        },
        "move_task" => ControllerSchema {
            namespace: "projects",
            function: "move_task",
            description: "Move a task to a different bucket, optionally repositioning it.",
            inputs: vec![
                task_id_input("Identifier of the task to move."),
                FieldSchema {
                    name: "bucket_id",
                    ty: TypeSchema::String,
                    comment: "Destination bucket id.",
                    required: true,
                },
                FieldSchema {
                    name: "position",
                    ty: TypeSchema::Option(Box::new(TypeSchema::F64)),
                    comment: "Float position within the new bucket; omit to append.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "task",
                ty: TypeSchema::Json,
                comment: "Updated Task record after the move.",
                required: true,
            }],
        },
        "delete_task" => ControllerSchema {
            namespace: "projects",
            function: "delete_task",
            description: "Permanently delete a task by id.",
            inputs: vec![task_id_input("Identifier of the task to delete.")],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Object {
                    fields: vec![
                        FieldSchema {
                            name: "task_id",
                            ty: TypeSchema::String,
                            comment: "Identifier of the deleted task.",
                            required: true,
                        },
                        FieldSchema {
                            name: "deleted",
                            ty: TypeSchema::Bool,
                            comment: "True when the task was successfully deleted.",
                            required: true,
                        },
                    ],
                },
                comment: "Deletion confirmation payload.",
                required: true,
            }],
        },
        "update_bucket" => ControllerSchema {
            namespace: "projects",
            function: "update_bucket",
            description: "Apply a partial patch to a bucket (rename, reorder, done-status).",
            inputs: vec![
                FieldSchema {
                    name: "bucket_id",
                    ty: TypeSchema::String,
                    comment: "Identifier of the bucket to update.",
                    required: true,
                },
                FieldSchema {
                    name: "patch",
                    ty: TypeSchema::Json,
                    comment: "BucketPatch: any subset of { title, position, is_done_bucket }.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "bucket",
                ty: TypeSchema::Json,
                comment: "Updated Bucket record.",
                required: true,
            }],
        },
        _other => ControllerSchema {
            namespace: "projects",
            function: "unknown",
            description: "Unknown projects controller function.",
            inputs: vec![FieldSchema {
                name: "function",
                ty: TypeSchema::String,
                comment: "Unknown function requested for schema lookup.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "error",
                ty: TypeSchema::String,
                comment: "Lookup error details.",
                required: true,
            }],
        },
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn handle_get_board(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        tracing::debug!("[rpc][projects] get_board entry");
        to_json(ops::get_board(&config)?)
    })
}

fn handle_create_task(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let input = ops::CreateTaskInput {
            title: get_str(&params, "title")?.to_string(),
            description: params
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            bucket_id: params
                .get("bucket_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            priority: params
                .get("priority")
                .and_then(|v| v.as_i64()),
            due_date: params
                .get("due_date")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        };
        tracing::debug!(title = %input.title, "[rpc][projects] create_task entry");
        to_json(ops::create_task(&config, input)?)
    })
}

fn handle_update_task(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let task_id = get_str(&params, "task_id")?.to_string();
        let patch: TaskPatch = read_required(&params, "patch")?;
        tracing::debug!(task_id = %task_id, "[rpc][projects] update_task entry");
        to_json(ops::update_task(&config, &task_id, patch)?)
    })
}

fn handle_move_task(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let task_id = get_str(&params, "task_id")?.to_string();
        let bucket_id = get_str(&params, "bucket_id")?.to_string();
        let position = params
            .get("position")
            .and_then(|v| v.as_f64());
        tracing::debug!(
            task_id = %task_id,
            bucket_id = %bucket_id,
            "[rpc][projects] move_task entry"
        );
        to_json(ops::move_task(&config, &task_id, &bucket_id, position)?)
    })
}

fn handle_delete_task(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let task_id = get_str(&params, "task_id")?.to_string();
        tracing::debug!(task_id = %task_id, "[rpc][projects] delete_task entry");
        // delete_task returns RpcOutcome<()>; we emit a confirmation object instead.
        let _outcome = ops::delete_task(&config, &task_id)?;
        let result = serde_json::json!({ "task_id": task_id, "deleted": true });
        to_json(RpcOutcome::single_log(result, format!("task deleted: {task_id}")))
    })
}

fn handle_update_bucket(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let bucket_id = get_str(&params, "bucket_id")?.to_string();
        let patch: BucketPatch = read_required(&params, "patch")?;
        tracing::debug!(bucket_id = %bucket_id, "[rpc][projects] update_bucket entry");
        to_json(ops::update_bucket(&config, &bucket_id, patch)?)
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn task_id_input(comment: &'static str) -> FieldSchema {
    FieldSchema {
        name: "task_id",
        ty: TypeSchema::String,
        comment,
        required: true,
    }
}

/// Extract a required string parameter.
fn get_str<'a>(params: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing or non-string required param '{key}'"))
}

fn read_required<T: DeserializeOwned>(params: &Map<String, Value>, key: &str) -> Result<T, String> {
    let value = params
        .get(key)
        .cloned()
        .ok_or_else(|| format!("missing required param '{key}'"))?;
    serde_json::from_value(value).map_err(|e| format!("invalid '{key}': {e}"))
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}
