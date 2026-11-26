//! The definition files a Claude Code session reads from its runtime directory: the MCP servers
//! (`--mcp-config`) and the agent definition selected with `--agent` (`--agents`).

use serde_json::{json, Map, Value};

use org_agent_runtime::SessionSpec;

/// The main agent definition's prompt, byte 0 of Claude Code's system prompt under `--agent`.
/// Release-static for every agent and session so the prompt cache is shared; everything
/// agent-specific, [`SessionSpec::agent`]'s own prompt included, follows in the appended
/// standing prompt ([`SessionSpec::standing_prompt`], `doc-27` §1).
const MAIN_AGENT_PROMPT: &str =
    "You are an autonomous agent working in a managed session. Your standing instructions follow.";

pub(super) fn build_mcp_json(spec: &SessionSpec) -> Vec<u8> {
    let mut servers = Map::new();
    for (name, server) in &spec.mcp_servers {
        servers.insert(
            name.as_str().to_owned(),
            json!({
                "command": server.command.as_str(),
                "args": server.args,
            }),
        );
    }
    serde_json::to_vec(&json!({ "mcpServers": servers }))
        .expect("MCP JSON serialization cannot fail")
}

pub(super) fn build_agents_json(spec: &SessionSpec, expanded_tools: Option<&[String]>) -> String {
    let mut def = Map::new();
    def.insert(
        "description".to_owned(),
        Value::String(spec.agent.description.clone()),
    );
    def.insert(
        "prompt".to_owned(),
        Value::String(MAIN_AGENT_PROMPT.to_owned()),
    );
    if let Some(tools) = expanded_tools {
        def.insert(
            "tools".to_owned(),
            Value::Array(tools.iter().map(|t| Value::String(t.clone())).collect()),
        );
    }

    let mut agents = Map::new();
    agents.insert(spec.agent.name.as_str().to_owned(), Value::Object(def));
    serde_json::to_string(&Value::Object(agents)).expect("agents JSON serialization cannot fail")
}
