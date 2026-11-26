//! Organization processes and standards (`doc-34`, `FC-110`, `FC-111`, `FC-112`, `FC-184`).
//!
//! Pure domain crate: no SQL, Git, network, filesystem I/O, or clock.
//! Implements single-chain inheritance resolution (`organization -> accountable team chain -> project`),
//! non-overridable standard protection, write validation (including agent project-scope rules),
//! instruction rendering (full mandatory section + 24 KiB capped overridable guidance), the
//! difference a task's chain makes to an agent's standing set, the built-in standing prompt
//! text, and the built-in product skills catalog (`agent-os skill list|read`).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod delta;
mod model;
mod prompts;
mod render;
mod resolve;
mod skills;
mod write;

pub use delta::{render_task_deltas, task_delta, TaskDelta};
pub use model::{
    summarize_body, DocumentKind, DocumentStatus, ProcessKey, Scope, StructureDocInput,
};
pub use prompts::{
    BuiltinPrompt, BUILTIN_INSTRUCTIONS_MAX_BYTES, BUILTIN_PROMPTS, CORE_RULES,
    CORE_RULES_MAX_BYTES, HR_LEAD, MANAGED_SESSION,
};
pub use render::{render_instructions, RenderError, RenderInput, Rendered, OVERRIDABLE_CAP_BYTES};
pub use resolve::{
    resolve_single_chain, EffectiveEntry, EffectiveSet, Layer, LayerOutcome, SingleChainInputs,
    Winner, WinnerKind,
};
pub use skills::{find_builtin_skill, list_builtin_skills, BuiltinSkill, SKILL_MAX_BYTES};
pub use write::{validate_write, CallerKind, WriteCheckInput, WriteViolation};

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "builtin_text_tests.rs"]
mod builtin_text_tests;
