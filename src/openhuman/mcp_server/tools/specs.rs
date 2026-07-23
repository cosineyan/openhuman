use serde_json::{json, Value};

use crate::openhuman::tools::SEARXNG_MAX_RESULTS;

use super::types::{McpToolSpec, DEFAULT_LIMIT, MAX_LIMIT};

pub fn tool_specs() -> Vec<McpToolSpec> {
    let mut specs = base_tool_specs();
    specs.push(searxng_tool_spec());
    specs
}

pub fn base_tool_specs() -> Vec<McpToolSpec> {
    vec![
        McpToolSpec {
            name: "core.list_tools",
            title: "List Core Tools",
            description: "List the live core agent tool catalog that OpenHuman exposes to its orchestrator session.",
            rpc_method: None,
            input_schema: no_args_schema(),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "core.tool_instructions",
            title: "Get Tool Instructions",
            description: "Emit the markdown tool-use instructions block that OpenHuman injects into prompt-guided agents.",
            rpc_method: None,
            input_schema: no_args_schema(),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "agent.list_subagents",
            title: "List Subagents",
            description: "List registered sub-agent definitions that the core can dispatch for specialized work.",
            rpc_method: None,
            input_schema: no_args_schema(),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "agent.run_subagent",
            title: "Run Subagent",
            description: "Run a registered OpenHuman sub-agent directly from the core and return its final response.",
            rpc_method: None,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "Registered sub-agent id (for example `researcher`, `planner`, `code_executor`)."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Task prompt for the sub-agent. Include the context it needs because this is a fresh session."
                    }
                },
                "required": ["agent_id", "prompt"],
                "additionalProperties": false
            }),
            // Sub-agent execution is the one Act-policy surface on the MCP
            // server today (see `enforce_act_policy` dispatch in `call_tool`).
            // Sub-agents can call further tools, so destructive/openWorld are
            // both true; running the same agent twice is not a no-op so
            // idempotent is false.
            annotations: json!({
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": false,
                "openWorldHint": true
            }),
        },
        McpToolSpec {
            name: "memory.search",
            title: "Search Memory",
            description: "Keyword-search OpenHuman's local memory tree and return matching chunks ordered by recency.",
            rpc_method: Some("openhuman.memory_tree_search"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Substring to match against stored memory chunks.",
                        "minLength": 1
                    },
                    "limit": {
                        "type": "integer",
                        "description": format!("Maximum chunks to return. Defaults to {DEFAULT_LIMIT}; capped at {MAX_LIMIT}."),
                        "minimum": 1,
                        "maximum": MAX_LIMIT
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "memory.recall",
            title: "Recall Memory",
            description: "Semantically recall local memory-tree chunks relevant to a natural-language query.",
            rpc_method: Some("openhuman.memory_tree_recall"),
            input_schema: query_schema("Natural-language query to embed and rerank against memory summaries."),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "tree.read_chunk",
            title: "Read Memory Chunk",
            description: "Read one memory-tree chunk by id. Use this to inspect the source text behind search or recall results. The response includes a `body` field with the full original text (not truncated), and a `chunk` field with metadata.",
            rpc_method: Some("openhuman.memory_tree_get_chunk"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "chunk_id": {
                        "type": "string",
                        "description": "Chunk id returned by memory.search or memory.recall."
                    }
                },
                "required": ["chunk_id"],
                "additionalProperties": false
            }),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "tree.suppress_chunk",
            title: "Suppress Memory Chunk",
            description: "Mark a memory chunk as suppressed so it is excluded from all future search and recall results. \
                          Use this when you identify that a chunk contains incorrect or unwanted information. \
                          The data is preserved and can be restored with tree.restore_chunk. \
                          First use memory.recall or tree.browse to find the chunk_id, then call this tool.",
            rpc_method: Some("openhuman.memory_tree_suppress_chunk"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "chunk_id": {
                        "type": "string",
                        "description": "The chunk id to suppress (32 hex chars)."
                    }
                },
                "required": ["chunk_id"],
                "additionalProperties": false
            }),
            annotations: json!({
                "title": "Suppress Memory Chunk",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }),
        },
        McpToolSpec {
            name: "tree.restore_chunk",
            title: "Restore Memory Chunk",
            description: "Restore a previously suppressed memory chunk back to active status, \
                          making it visible in search and recall again. Use this to undo a suppress_chunk call.",
            rpc_method: Some("openhuman.memory_tree_restore_chunk"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "chunk_id": {
                        "type": "string",
                        "description": "The chunk id to restore (32 hex chars)."
                    }
                },
                "required": ["chunk_id"],
                "additionalProperties": false
            }),
            annotations: json!({
                "title": "Restore Memory Chunk",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }),
        },
        McpToolSpec {
            name: "tree.browse",
            title: "Browse Memory",
            description: "Paginated listing of raw memory-tree chunks in reverse-chronological order, \
                          with optional filters by source kind, source id, entity id, time window, \
                          and substring keyword. Returns individual chunks — NOTE: one meeting or email \
                          is split into many chunks, so a k=30 limit may only cover a few sources. \
                          Use this to inspect specific chunk content or search within a known source. \
                          For listing meetings/emails/chats by date or type (e.g. \"what meetings \
                          happened yesterday\"), prefer `memory.query_source` which returns one summary \
                          per source (one per meeting) instead of raw chunks.",
            rpc_method: Some("openhuman.memory_tree_list_chunks"),
            input_schema: tree_browse_schema(),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "tree.top_entities",
            title: "Top Memory Entities",
            description: "List the most-referenced canonical entities (people, organizations, \
                          topics, emails) across the local memory tree. Call this for entity \
                          discovery before drilling in with `tree.browse` (passing `entity_ids`) \
                          or `memory.search`. Returns entities ordered by reference count.",
            rpc_method: Some("openhuman.memory_tree_top_entities"),
            input_schema: tree_top_entities_schema(),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "tree.list_sources",
            title: "List Memory Sources",
            description: "List every distinct ingest source (Gmail account, Slack channel, Notion \
                          workspace, email thread, …) that has data in the memory tree, with \
                          chunk counts and last-activity timestamps. Use this when the user asks \
                          \"what data sources do I have\" or to discover source ids to pass into \
                          `tree.browse`.",
            rpc_method: Some("openhuman.memory_tree_list_sources"),
            input_schema: tree_list_sources_schema(),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "memory.smart_walk",
            title: "Smart Memory Walk",
            description: "Multi-strategy memory retrieval — combines vector search, keyword \
                          search, entity lookup, and tree browsing to answer natural-language \
                          queries across raw chat messages, wiki summaries, documents, and \
                          episodic memories. Use this for complex questions like \"what did \
                          Robert say recently\" or \"summarize my Teams chats about project X\". \
                          More powerful than memory.search or memory.recall for open-ended \
                          questions.",
            rpc_method: Some("openhuman.memory_tree_smart_walk"),
            input_schema: memory_smart_walk_schema(),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "memory.query_source",
            title: "Query Memory Source Tree",
            description: "Return summaries from the wiki summary tree, filtered by source type \
                          (chat, email, document, transcript, calendar) and optionally by source_id \
                          or time window. Returns ONE summary per source (one per meeting, one per \
                          email thread, one per chat conversation) — ideal for listing meetings by \
                          date, finding emails from a person, or getting an overview of activity. \
                          Use `time_window_days=1` for yesterday, `time_window_days=7` for last week. \
                          Add `query` for semantic rerank. For meeting transcripts use \
                          source_kind='transcript'; for calendar events use source_kind='calendar'. \
                          Good for: \"what meetings happened yesterday\", \"summarise my Teams chats \
                          this week\", \"emails about project X last month\".",
            rpc_method: Some("openhuman.memory_tree_query_source"),
            input_schema: memory_query_source_schema(),
            annotations: read_only_local_annotations(),
        },
        McpToolSpec {
            name: "memory.store",
            title: "Store Memory",
            description: "Create a new memory document from content. The document is stored in \
                          the specified namespace (default `mcp`) and can be retrieved via \
                          `memory.search` or `memory.recall`.",
            rpc_method: Some("openhuman.memory_doc_put"),
            input_schema: memory_store_schema(),
            annotations: write_local_annotations(),
        },
        McpToolSpec {
            name: "memory.note",
            title: "Annotate Memory Chunk",
            description: "Append a note to an existing memory chunk by storing a linked annotation \
                          document. The note references the original chunk_id for provenance and \
                          can be retrieved alongside it.",
            rpc_method: Some("openhuman.memory_doc_put"),
            input_schema: memory_note_schema(),
            annotations: write_local_annotations(),
        },
        McpToolSpec {
            name: "tree.tag",
            title: "Tag Memory Chunk",
            description: "Apply one or more category tags to an existing memory chunk. \
                          Stored as an upsertable tag-record document linked to the target \
                          chunk_id, so re-tagging the same chunk replaces the prior tag set \
                          rather than accumulating duplicate annotations. Differs from \
                          `memory.note` in that the payload is a categorical label list — \
                          queryable via the document `tags` field — rather than free-form text.",
            rpc_method: Some("openhuman.memory_doc_put"),
            input_schema: tree_tag_schema(),
            annotations: write_local_annotations(),
        },
    ]
}

/// Annotation preset for the read-only, closed-world tools that just read
/// OpenHuman's local memory tree or agent registry. The MCP spec defaults are
/// `readOnlyHint: false` / `openWorldHint: true`, so both fields must be set
/// explicitly to communicate the actual shape to clients. Destructive and
/// idempotent hints are deliberately omitted — per the spec they are
/// meaningful only when `readOnlyHint == false`.
pub fn read_only_local_annotations() -> Value {
    json!({
        "readOnlyHint": true,
        "openWorldHint": false
    })
}

/// Annotation preset for the MCP write tools (`memory.store`, `memory.note`,
/// `tree.tag`) that upsert documents into OpenHuman's local memory tree.
/// Writes are keyed deterministically (slug-from-title, `mcp-note-<chunk_id>`,
/// `mcp-tag-<chunk_id>`) so repeating a call with identical arguments yields
/// the same stored state — `idempotentHint: true`. The upsert can replace a
/// previously stored document for the same key, which is a destructive update
/// in MCP-spec terms — `destructiveHint: true`. Local-only, no external I/O —
/// `openWorldHint: false`.
pub fn write_local_annotations() -> Value {
    json!({
        "readOnlyHint": false,
        "destructiveHint": true,
        "idempotentHint": true,
        "openWorldHint": false
    })
}

pub fn searxng_tool_spec() -> McpToolSpec {
    McpToolSpec {
        name: "searxng_search",
        title: "SearXNG Search",
        description: "Search the configured self-hosted SearXNG instance and return normalized title, URL, snippet, and source results. Requires searxng.enabled=true in OpenHuman config.",
        rpc_method: Some("openhuman.tools_searxng_search"),
        input_schema: searxng_search_schema(),
        // SearXNG queries an external (self-hosted but network-reachable)
        // search engine: read-only (no state mutation), open-world (results
        // come from outside OpenHuman). Per spec, destructive/idempotent
        // hints are meaningful only when readOnlyHint=false, so omit them.
        annotations: json!({
            "readOnlyHint": true,
            "openWorldHint": true
        }),
    }
}

pub fn list_tools_result_for_config(config: &crate::openhuman::config::Config) -> Value {
    let mut specs = base_tool_specs();
    if config.searxng.enabled {
        specs.push(searxng_tool_spec());
    }
    list_tools_result_from_specs(specs)
}

pub fn list_tools_result_from_specs(specs: Vec<McpToolSpec>) -> Value {
    let tools = specs
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "title": tool.title,
                "description": tool.description,
                "inputSchema": tool.input_schema,
                "annotations": tool.annotations,
            })
        })
        .collect::<Vec<_>>();
    json!({ "tools": tools })
}

// ── Schema builder helpers ────────────────────────────────────────────────────

pub fn no_args_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

pub fn query_schema(query_description: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": query_description,
                "minLength": 1
            },
            "k": {
                "type": "integer",
                "description": format!("Maximum chunks to return. Defaults to {DEFAULT_LIMIT}; capped at {MAX_LIMIT}."),
                "minimum": 1,
                "maximum": MAX_LIMIT
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn tree_browse_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "source_kinds": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Restrict to one or more source kinds (e.g. `email`, `chat`, `document`). Omit to include all kinds."
            },
            "source_ids": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Restrict to specific logical source ids (e.g. a Slack channel id). Use `tree.list_sources` to discover these."
            },
            "entity_ids": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Restrict to chunks referencing any of these canonical entity ids (e.g. `person:Alice`, `email:alice@example.com`). Use `tree.top_entities` to discover these."
            },
            "since_ms": {
                "type": "integer",
                "minimum": 0,
                "description": "Inclusive lower bound on chunk timestamp, in milliseconds since Unix epoch."
            },
            "until_ms": {
                "type": "integer",
                "minimum": 0,
                "description": "Inclusive upper bound on chunk timestamp, in milliseconds since Unix epoch."
            },
            "query": {
                "type": "string",
                "minLength": 1,
                "description": "Substring keyword filter over the chunk preview text."
            },
            "k": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAX_LIMIT,
                "description": format!("Maximum chunks per page. Defaults to {DEFAULT_LIMIT}; capped at {MAX_LIMIT}.")
            },
            "offset": {
                "type": "integer",
                "minimum": 0,
                "description": "Pagination offset (number of rows to skip). Defaults to 0."
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

fn tree_top_entities_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": {
                "type": "string",
                "minLength": 1,
                "description": "Restrict to a single entity kind (`person`, `email`, `topic`, `org`, …). Omit to span all kinds."
            },
            "k": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAX_LIMIT,
                "description": format!("Maximum entities to return. Defaults to {DEFAULT_LIMIT}; capped at {MAX_LIMIT}.")
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

fn tree_list_sources_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "user_email_hint": {
                "type": "string",
                "minLength": 1,
                "description": "When provided, the user's own email is stripped from email-thread display names so the other party shows up instead. Optional."
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

fn memory_store_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": {
                "type": "string",
                "minLength": 1,
                "description": "Human-readable title for the memory document."
            },
            "content": {
                "type": "string",
                "minLength": 1,
                "description": "The text content to store as a memory document."
            },
            "namespace": {
                "type": "string",
                "minLength": 1,
                "description": "Namespace to store the document in. Defaults to `mcp` when omitted."
            },
            "tags": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional tags for categorisation and filtering."
            }
        },
        "required": ["title", "content"],
        "additionalProperties": false
    })
}

fn memory_note_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "chunk_id": {
                "type": "string",
                "minLength": 1,
                "description": "ID of the memory chunk to annotate. Use an ID from memory.search or memory.recall results."
            },
            "note_text": {
                "type": "string",
                "minLength": 1,
                "description": "The note text to attach to the chunk."
            }
        },
        "required": ["chunk_id", "note_text"],
        "additionalProperties": false
    })
}

fn tree_tag_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "chunk_id": {
                "type": "string",
                "minLength": 1,
                "description": "ID of the memory chunk to tag. Use an ID from `memory.search`, `memory.recall`, or `tree.browse` results."
            },
            "tags": {
                "type": "array",
                "items": {
                    "type": "string",
                    "minLength": 1
                },
                "minItems": 1,
                "description": "One or more category labels to attach (e.g. `[\"todo\", \"q3-planning\"]`). Re-tagging the same chunk replaces the prior tag set; supply the complete desired set on each call."
            }
        },
        "required": ["chunk_id", "tags"],
        "additionalProperties": false
    })
}

fn memory_smart_walk_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "minLength": 1,
                "description": "Natural-language question to answer. E.g. 'What did Robert say recently about the project?'"
            },
            "namespace": {
                "type": "string",
                "description": "Memory namespace. Default: \"default\"."
            },
            "max_turns": {
                "type": "integer",
                "minimum": 1,
                "maximum": 25,
                "description": "Max LLM turns for the walk. Default 12."
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn memory_query_source_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "source_id": {
                "type": "string",
                "description": "Exact source id to filter by (e.g. a specific chat or email thread)."
            },
            "source_kind": {
                "type": "string",
                "enum": ["chat", "email", "document", "transcript", "calendar"],
                "description": "Filter by source type: chat (Teams), email (Outlook), document, transcript (meeting transcripts), or calendar."
            },
            "time_window_days": {
                "type": "integer",
                "minimum": 1,
                "description": "Only return summaries from the last N days."
            },
            "query": {
                "type": "string",
                "description": "Optional query to rerank results by relevance."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 50,
                "description": "Maximum results to return."
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

fn searxng_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "minLength": 1,
                "description": "Search query string."
            },
            "categories": {
                "type": "array",
                "items": {
                    "type": "string",
                    "enum": ["web", "general", "news", "images"]
                },
                "description": "Optional SearXNG categories. `web` maps to SearXNG `general`."
            },
            "language": {
                "type": "string",
                "minLength": 1,
                "description": "Optional language code, e.g. `en`, `zh-CN`, or `fr`."
            },
            "max_results": {
                "type": "integer",
                "minimum": 1,
                "maximum": SEARXNG_MAX_RESULTS,
                "description": format!("Maximum results to return. Defaults to searxng.max_results; capped at {SEARXNG_MAX_RESULTS}.")
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}
