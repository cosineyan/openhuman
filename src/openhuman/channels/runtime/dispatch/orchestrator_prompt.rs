//! Per-turn system-prompt assembly for channel runtimes, aligned with the
//! web-chat orchestrator prompt.
//!
//! The legacy channel prompt ([`crate::openhuman::context::channels_prompt`])
//! is a compact hand-rolled string that omits the orchestrator's behavioral
//! policy (`orchestrator/prompt.md`) and the dispatcher's native tool-call
//! protocol. That thinness makes the model answer from context instead of
//! calling tools (e.g. a `profile <name>` request returns a vague reply
//! instead of invoking `retrieve_memory`).
//!
//! This module builds the SAME orchestrator system prompt web-chat uses, by
//! reusing the shared [`SystemPromptBuilder::from_dynamic`] +
//! [`PromptContext`] path against the orchestrator agent definition. It does
//! NOT fetch per-turn learned context (kept out to preserve prefix-cache
//! stability on high-frequency channels — see the parity plan); channel turns
//! still inject a recall block into the user message via
//! `build_memory_context`.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use crate::openhuman::agent::dispatcher::ToolDispatcher;
use crate::openhuman::agent::harness::definition::{AgentDefinitionRegistry, PromptSource};
use crate::openhuman::context::prompt::{
    render_connected_identities, ConnectedIntegration, LearnedContextData, PromptContext,
    PromptTool, SystemPromptBuilder,
};
use crate::openhuman::tools::Tool;

/// Agent id whose behavioral prompt channel turns adopt. Channels always
/// dispatch to the orchestrator (see `runtime::dispatch::routing`).
const ORCHESTRATOR_AGENT_ID: &str = "orchestrator";

/// Build the orchestrator system prompt for a channel turn.
///
/// Mirrors the `PromptContext` assembly in
/// `agent/harness/session/turn/context.rs::build_system_prompt`, but with
/// `learned = LearnedContextData::default()` (no per-turn fetch) and defaults
/// for the profile/personality fields. Returns an error if the orchestrator
/// definition isn't registered or the builder fails, so the caller can fall
/// back to the legacy static prompt without regressing basic chat.
pub(crate) fn build_orchestrator_channel_prompt(
    workspace_dir: &Path,
    model_name: &str,
    tools_registry: &[Box<dyn Tool>],
    extra_tools: &[Box<dyn Tool>],
    visible_tool_names: &HashSet<String>,
    dispatcher: &dyn ToolDispatcher,
    connected_integrations: &[ConnectedIntegration],
) -> Result<String> {
    let registry = AgentDefinitionRegistry::global()
        .ok_or_else(|| anyhow::anyhow!("AgentDefinitionRegistry not initialised"))?;
    let definition = registry
        .get(ORCHESTRATOR_AGENT_ID)
        .ok_or_else(|| anyhow::anyhow!("orchestrator agent definition not registered"))?;

    // Only the Dynamic source carries the orchestrator persona builder. The
    // orchestrator ships as a built-in Dynamic agent; bail to the caller's
    // fallback for any other shape rather than emitting a bare prompt.
    let prompt_builder = match &definition.system_prompt {
        PromptSource::Dynamic(build) => SystemPromptBuilder::from_dynamic(*build),
        _ => {
            anyhow::bail!(
                "orchestrator system_prompt is not Dynamic — cannot reuse persona builder"
            )
        }
    };

    // Combine the channel tool registry with the per-turn delegation tools
    // (synthesised by routing for the orchestrator's `subagents`) so both the
    // dispatcher protocol text and the rendered catalogue see the full set —
    // exactly what the visible-tool whitelist was computed against.
    let mut combined_specs = Vec::with_capacity(tools_registry.len() + extra_tools.len());
    let mut prompt_tools: Vec<PromptTool<'_>> =
        Vec::with_capacity(tools_registry.len() + extra_tools.len());
    for tool in tools_registry.iter().chain(extra_tools.iter()) {
        combined_specs.push(tool.spec());
        prompt_tools.push(PromptTool {
            name: tool.name(),
            description: tool.description(),
            parameters_schema: Some(tool.parameters_schema().to_string()),
        });
    }

    // Dispatcher-generated tool-call protocol instructions (native / XML /
    // P-Format) — the text that teaches the model HOW to call tools, absent
    // from the legacy channel prompt. Mirror the web path: prefer the
    // spec-based instructions, falling back to the tool-based form (Native /
    // P-Format ignore the tools arg, XML supplies `_for_specs`, so the empty
    // fallback slice is never actually consumed).
    let dispatcher_instructions = dispatcher
        .prompt_instructions_for_specs(&combined_specs)
        .unwrap_or_else(|| dispatcher.prompt_instructions(&[]));

    let ctx = PromptContext {
        workspace_dir,
        model_name,
        agent_id: ORCHESTRATOR_AGENT_ID,
        tools: &prompt_tools,
        workflows: &[],
        dispatcher_instructions: &dispatcher_instructions,
        learned: LearnedContextData::default(),
        visible_tool_names,
        tool_call_format: dispatcher.tool_call_format(),
        connected_integrations,
        connected_identities_md: render_connected_identities(),
        include_profile: !definition.omit_profile,
        include_memory_md: !definition.omit_memory_md,
        curated_snapshot: None,
        user_identity: crate::openhuman::app_state::peek_cached_current_user_identity(),
        personality_soul_md: None,
        personality_memory_md: None,
        personality_roster: vec![],
    };

    let mut prompt = prompt_builder.build(&ctx)?;

    // Channel-path tool-name bridge. The orchestrator persona instructs the
    // model to call native tools like `memory_tree` (mode=profile_person). But
    // on the channel path the underlying Claude Code process reaches OpenHuman
    // tools ONLY through MCP, where they are named `mcp__openhuman__<tool>`
    // (dots → underscores). Spell out the MCP names for the high-value flows so
    // the model calls a tool that actually exists in its toolset instead of an
    // absent native name.
    prompt.push_str(CHANNEL_MCP_TOOL_ADDENDUM);
    Ok(prompt)
}

/// Appended to the channel orchestrator prompt: maps the persona's native tool
/// references onto the MCP tool names the Claude Code subprocess can actually
/// call. Kept terse and byte-stable (no per-turn interpolation) so it doesn't
/// disturb prefix caching.
const CHANNEL_MCP_TOOL_ADDENDUM: &str = "\n\n## Channel Tool Names (MCP)\n\n\
On this channel your OpenHuman tools are exposed via MCP. Call them by their \
MCP names (not the bare native names mentioned above):\n\
- Person profile / \"who is\" / \"tell me about\" someone → call \
`mcp__openhuman__memory_profile_person` with `{\"name\": \"<person>\"}` (or \
`email`). This returns the live org chart (manager, reports, department) — use \
it instead of answering from history.\n\
- Recall past context → `mcp__openhuman__memory_recall` / \
`mcp__openhuman__memory_search` with a `query`.\n\
- Delegate specialist work → `mcp__openhuman__agent_run_subagent` with \
`{\"agent_id\": \"<id>\", \"prompt\": \"<task>\"}`.\n\
Emit these as real tool calls; do not just describe intent.\n";
