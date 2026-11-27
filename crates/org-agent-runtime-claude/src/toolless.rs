//! Sans-I/O strict tool-less prompt adapter for Claude Code (`WATCH-4`, `TASK-259`).
//!
//! Declares Claude Code's zero-tool, zero-MCP, no-slash-command, no-session-persistence argv and
//! parses its JSON stdout envelope without performing any I/O or process spawning.

use org_agent_runtime::{ToollessPromptAdapter, ToollessPromptOutput, ToollessPromptUsage};

pub(crate) struct ClaudeToollessPrompt;

pub(crate) static CLAUDE_TOOLLESS_PROMPT: ClaudeToollessPrompt = ClaudeToollessPrompt;

impl ToollessPromptAdapter for ClaudeToollessPrompt {
    fn availability_args(&self) -> &'static [&'static str] {
        &["--version"]
    }

    fn run_args(&self, model_key: &str) -> Vec<String> {
        let mut args = vec![
            "-p".to_owned(),
            "--tools".to_owned(),
            String::new(),
            "--strict-mcp-config".to_owned(),
            "--disable-slash-commands".to_owned(),
            "--output-format".to_owned(),
            "json".to_owned(),
            "--no-session-persistence".to_owned(),
        ];
        if !model_key.is_empty() && model_key.starts_with("claude-") {
            args.push("--model".to_owned());
            args.push(model_key.to_owned());
        }
        args
    }

    fn parse_output(&self, stdout_bytes: &[u8]) -> Option<ToollessPromptOutput> {
        let env: serde_json::Value = serde_json::from_slice(stdout_bytes).ok()?;
        if env
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return None;
        }
        let raw = env.get("result").and_then(|v| v.as_str())?;
        let usage = env.get("usage").map(|u| ToollessPromptUsage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()),
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()),
        });
        Some(ToollessPromptOutput {
            text: strip_markdown_json_fence(raw),
            usage,
        })
    }
}

fn strip_markdown_json_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_owned();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_owned();
        }
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_args_enforce_toolless_flags_and_filter_model_prefix() {
        let with_model = CLAUDE_TOOLLESS_PROMPT.run_args("claude-sonnet-4-6");
        assert!(with_model.windows(2).any(|w| w == ["--tools", ""]));
        assert!(with_model.contains(&"--strict-mcp-config".to_owned()));
        assert!(with_model.contains(&"--disable-slash-commands".to_owned()));
        assert!(with_model.contains(&"--no-session-persistence".to_owned()));
        assert!(with_model
            .windows(2)
            .any(|w| w == ["--model", "claude-sonnet-4-6"]));

        let non_claude = CLAUDE_TOOLLESS_PROMPT.run_args("gpt-5");
        assert!(!non_claude.contains(&"--model".to_owned()));
        assert!(!CLAUDE_TOOLLESS_PROMPT
            .run_args("")
            .contains(&"--model".to_owned()));
    }

    #[test]
    fn parse_output_strips_fences_extracts_usage_and_rejects_errors() {
        let raw = br#"{"is_error":false,"result":"```json\n{\"ok\":true}\n```","usage":{"input_tokens":12,"output_tokens":34}}"#;
        let parsed = CLAUDE_TOOLLESS_PROMPT.parse_output(raw).unwrap();
        assert_eq!(parsed.text, r#"{"ok":true}"#);
        assert_eq!(
            parsed.usage,
            Some(ToollessPromptUsage {
                input_tokens: Some(12),
                output_tokens: Some(34),
            })
        );

        let err_env = br#"{"is_error":true,"result":"oops"}"#;
        assert!(CLAUDE_TOOLLESS_PROMPT.parse_output(err_env).is_none());
        assert!(CLAUDE_TOOLLESS_PROMPT.parse_output(b"not json").is_none());
        assert!(CLAUDE_TOOLLESS_PROMPT
            .parse_output(br#"{"is_error":false}"#)
            .is_none());
    }
}
