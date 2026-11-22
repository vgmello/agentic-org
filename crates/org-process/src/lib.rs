//! Organization processes and standards (`doc-34`, `FC-110`, `FC-111`, `FC-112`, `FC-184`).
//!
//! Pure domain crate: no SQL, Git, network, filesystem I/O, or clock.
//! Implements single-chain inheritance resolution (`organization -> accountable team chain -> project`),
//! non-overridable standard protection, write validation (including agent project-scope rules),
//! instruction rendering (full mandatory section + 24 KiB capped overridable guidance), and the
//! built-in product skills catalog (`agent-os skill list|read`).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod model;
mod render;
mod resolve;
mod skills;
mod write;

pub use model::{
    summarize_body, DocumentKind, DocumentStatus, ProcessKey, Scope, StructureDocInput,
};
pub use render::{render_instructions, RenderError, RenderInput, Rendered, OVERRIDABLE_CAP_BYTES};
pub use resolve::{
    resolve_single_chain, EffectiveEntry, EffectiveSet, Layer, LayerOutcome, SingleChainInputs,
    Winner, WinnerKind,
};
pub use skills::{find_builtin_skill, list_builtin_skills, BuiltinSkill};
pub use write::{validate_write, CallerKind, WriteCheckInput, WriteViolation};

#[cfg(test)]
mod tests;
