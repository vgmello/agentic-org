//! Sans-I/O conversation transcript parsers for ACP runtimes (`agy`, `gemini`, `kimi`, `pi`).

mod agy_parse;
mod agy_subagents;
mod generic_parse;

use org_agent_runtime::transcript::{
    LoadedSubagentTranscript, ParsedTranscript, SubagentTranscriptRef, TranscriptDiscoveryLayout,
    TranscriptParser,
};

use self::agy_parse::parse_agy_jsonl;
use self::agy_subagents::{attach_agy_subagents, extract_agy_subagent_refs};
use self::generic_parse::parse_generic_acp_jsonl;

/// Sans-I/O Antigravity transcript parser registered by [`crate::AgyRuntime`].
pub(crate) struct AgyTranscriptParser;

/// Static parser instance returned by [`crate::AgyRuntime::transcript_parser`].
pub(crate) static AGY_TRANSCRIPT_PARSER: AgyTranscriptParser = AgyTranscriptParser;

impl TranscriptParser for AgyTranscriptParser {
    fn format_id(&self) -> &'static str {
        "agy"
    }

    fn discovery_layout(&self) -> TranscriptDiscoveryLayout {
        TranscriptDiscoveryLayout::ConversationDir {
            brain_root: ".gemini/antigravity-cli/brain",
            transcript_rel_path: ".system_generated/logs/transcript.jsonl",
        }
    }

    fn extract_subagent_refs(
        &self,
        raw: &str,
        parent_session_id: &str,
    ) -> Vec<SubagentTranscriptRef> {
        extract_agy_subagent_refs(raw, parent_session_id)
    }

    fn parse(&self, raw: &str) -> ParsedTranscript {
        parse_agy_jsonl(raw)
    }

    fn attach_subagents(
        &self,
        parent: &mut ParsedTranscript,
        subagents: Vec<LoadedSubagentTranscript>,
    ) {
        attach_agy_subagents(parent, subagents);
    }
}

/// Declarative sans-I/O JSONL transcript parser for standard ACP CLIs (`gemini`, `kimi`, `pi`).
pub(crate) struct GenericAcpTranscriptParser {
    format_id: &'static str,
    search_roots: &'static [&'static str],
}

/// Static parser instance returned by [`crate::GeminiRuntime::transcript_parser`].
pub(crate) static GEMINI_TRANSCRIPT_PARSER: GenericAcpTranscriptParser =
    GenericAcpTranscriptParser {
        format_id: "gemini",
        search_roots: &[".gemini/sessions"],
    };

/// Static parser instance returned by [`crate::KimiRuntime::transcript_parser`].
pub(crate) static KIMI_TRANSCRIPT_PARSER: GenericAcpTranscriptParser = GenericAcpTranscriptParser {
    format_id: "kimi",
    search_roots: &[".kimi/sessions"],
};

/// Static parser instance returned by [`crate::PiRuntime::transcript_parser`].
pub(crate) static PI_TRANSCRIPT_PARSER: GenericAcpTranscriptParser = GenericAcpTranscriptParser {
    format_id: "pi",
    search_roots: &[".pi/agent/sessions", ".pi/sessions"],
};

impl TranscriptParser for GenericAcpTranscriptParser {
    fn format_id(&self) -> &'static str {
        self.format_id
    }

    fn discovery_layout(&self) -> TranscriptDiscoveryLayout {
        TranscriptDiscoveryLayout::RecursiveJsonl {
            search_roots: self.search_roots,
            skip_parent_dir: None,
            subagents_subdir: None,
            subagent_stem_prefix: None,
        }
    }

    fn parse(&self, raw: &str) -> ParsedTranscript {
        parse_generic_acp_jsonl(self.format_id, raw)
    }
}
