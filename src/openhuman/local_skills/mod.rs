use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::rpc::RpcOutcome;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSkill {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub author: Option<String>,
    pub body: String,
    pub plugin_name: String,
    pub version: String,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn claude_home() -> Option<std::path::PathBuf> {
    Some(dirs::home_dir()?.join(".claude"))
}

/// Parse YAML frontmatter from a SKILL.md file.
/// Returns (frontmatter fields, body_after_frontmatter).
fn parse_skill_md(content: &str) -> (HashMap<String, String>, String) {
    let mut fields = HashMap::new();
    let body;

    if content.starts_with("---") {
        let rest = &content[3..];
        if let Some(end_pos) = rest.find("\n---") {
            let fm = &rest[..end_pos];
            body = rest[end_pos + 4..].trim_start_matches('\n').to_string();

            // Simple YAML key: value parser (handles quoted strings)
            for line in fm.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    let key = k.trim().to_string();
                    let val = v.trim().trim_matches('"').to_string();
                    if !key.is_empty() && !val.is_empty() {
                        fields.insert(key, val);
                    }
                }
            }
        } else {
            body = content.to_string();
        }
    } else {
        body = content.to_string();
    }

    (fields, body)
}

/// Load all user-scoped installed skills from ~/.claude/plugins.
pub fn list_local_skills_impl() -> Vec<LocalSkill> {
    let claude_home = match claude_home() {
        Some(p) => p,
        None => return vec![],
    };

    let installed_path = claude_home.join("plugins").join("installed_plugins.json");
    let installed_text = match std::fs::read_to_string(&installed_path) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let installed: Value = match serde_json::from_str(&installed_text) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let plugins = match installed.get("plugins").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return vec![],
    };

    let mut seen = std::collections::HashSet::new();
    let mut skills = Vec::new();

    for (plugin_name, entries) in plugins {
        let entries_arr = match entries.as_array() {
            Some(a) => a,
            None => continue,
        };

        // Collect user-scoped entries (dedup by installPath)
        for entry in entries_arr {
            let scope = entry.get("scope").and_then(|v| v.as_str()).unwrap_or("");
            if scope != "user" {
                continue;
            }
            let install_path = match entry.get("installPath").and_then(|v| v.as_str()) {
                Some(p) => std::path::PathBuf::from(p),
                None => continue,
            };
            let version = entry
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if !seen.insert(install_path.clone()) {
                continue;
            }

            let skills_dir = install_path.join("skills");
            if !skills_dir.is_dir() {
                continue;
            }

            let dir_entries = match std::fs::read_dir(&skills_dir) {
                Ok(d) => d,
                Err(_) => continue,
            };

            for entry in dir_entries.flatten() {
                let skill_dir = entry.path();
                if !skill_dir.is_dir() {
                    continue;
                }
                let skill_md = skill_dir.join("SKILL.md");
                if !skill_md.exists() {
                    continue;
                }
                let content = match std::fs::read_to_string(&skill_md) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let (fields, body) = parse_skill_md(&content);
                let name = fields
                    .get("name")
                    .cloned()
                    .unwrap_or_else(|| skill_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                let description = fields.get("description").cloned().unwrap_or_default();
                let when_to_use = fields.get("when_to_use").cloned();
                // author is nested under metadata:, try both
                let author = fields.get("author").cloned();

                skills.push(LocalSkill {
                    name,
                    description,
                    when_to_use,
                    author,
                    body,
                    plugin_name: plugin_name.clone(),
                    version: version.clone(),
                });
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

// ─── RPC ─────────────────────────────────────────────────────────────────────

pub fn all_local_skills_registered_controllers() -> Vec<RegisteredController> {
    vec![RegisteredController {
        schema: controller_schema(),
        handler: handle_list_local_skills,
    }]
}

pub fn all_local_skills_controller_schemas() -> Vec<ControllerSchema> {
    vec![controller_schema()]
}

fn controller_schema() -> ControllerSchema {
    ControllerSchema {
        namespace: "local_skills",
        function: "list",
        description: "List locally installed Claude Code skills from ~/.claude/plugins (user scope).",
        inputs: vec![],
        outputs: vec![FieldSchema {
            name: "skills",
            ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
            comment: "Array of LocalSkill objects.",
            required: true,
        }],
    }
}

fn handle_list_local_skills(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let skills = list_local_skills_impl();
        let val = serde_json::json!({
            "result": serde_json::to_value(&skills).map_err(|e| e.to_string())?,
            "logs": [],
        });
        Ok(val)
    })
}
