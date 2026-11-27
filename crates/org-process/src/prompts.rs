//! Built-in standing prompt text every managed agent receives (`doc-27` §1 tier 1, `INSTR-4`).
//!
//! The text lives in body-only Markdown files under `prompts/`, embedded at compile time, so
//! the release-static start of every agent's standing instructions is plain reviewable prose.
//! Budgets and the file-to-manifest bijection are checked by this crate's tests.

/// Day-1 standing rules: situation, authority, untrusted data, the `agent-os` model, on-demand
/// guides, where to write, background work and waiting.
pub const CORE_RULES: &str = include_str!("../prompts/core-rules.md");

/// Day-1 standing rules for external workspace sessions (`mode: external`): same authority,
/// `agent-os` grammar, guides and write locations, without office-managed wake or reap rules.
pub const EXTERNAL_RULES: &str = include_str!("../prompts/external-rules.md");

/// How a managed session behaves: workspace, discovery, replies, asks, mid-turn messages and
/// secrets.
pub const MANAGED_SESSION: &str = include_str!("../prompts/managed-session.md");

/// Extra duties of the organization's built-in HR lead (`INSTR-21`).
pub const HR_LEAD: &str = include_str!("../prompts/hr-lead.md");

/// Extra duties of the organization's built-in Chief of Staff, Tess (`TASK-80.1`, `AX-11`):
/// her voice, caller-scope discipline, batching and a topic index of the live commands. Part 5
/// of her standing prompt, after the release-static parts every agent shares byte for byte.
pub const CHIEF_OF_STAFF_GUIDANCE: &str = include_str!("../prompts/chief-of-staff-guidance.md");

/// Most bytes [`CHIEF_OF_STAFF_GUIDANCE`] may take (2 KiB).
pub const CHIEF_OF_STAFF_GUIDANCE_MAX_BYTES: usize = 2_048;

/// Most bytes [`CORE_RULES`] may take (2.5 KiB).
pub const CORE_RULES_MAX_BYTES: usize = 2_560;

/// Most bytes all built-in standing prompt text together may take (`INSTR-4`: 10 KiB).
pub const BUILTIN_INSTRUCTIONS_MAX_BYTES: usize = 10 * 1024;

/// One embedded prompt file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinPrompt {
    /// File name under `prompts/`.
    pub file: &'static str,
    /// The file's text.
    pub body: &'static str,
}

/// Every file under `prompts/`, exactly once.
pub const BUILTIN_PROMPTS: &[BuiltinPrompt] = &[
    BuiltinPrompt {
        file: "core-rules.md",
        body: CORE_RULES,
    },
    BuiltinPrompt {
        file: "external-rules.md",
        body: EXTERNAL_RULES,
    },
    BuiltinPrompt {
        file: "managed-session.md",
        body: MANAGED_SESSION,
    },
    BuiltinPrompt {
        file: "hr-lead.md",
        body: HR_LEAD,
    },
    BuiltinPrompt {
        file: "chief-of-staff-guidance.md",
        body: CHIEF_OF_STAFF_GUIDANCE,
    },
];
