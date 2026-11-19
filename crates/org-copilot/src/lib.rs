//! # org-copilot: the copilot's rules
//!
//! The office's smart features: a person asks for a **draft**, the user's own agent runtime
//! (`claude -p`, `codex exec`) writes it in the background, and the person confirms. The copilot
//! proposes; it never acts. The design is `docs/design/smart-features.md`.
//!
//! What is here:
//! - [`Registry`] / [`Feature`]: each feature has a versioned prompt template, an input schema
//!   and an output schema. [`builtin`] has `text.improve` and `bug.from_sentence`.
//! - [`prompt`]: renders the template with the input escaped and fenced as data, never as
//!   instructions.
//! - [`Schema`]: validates input and the runtime's answer against a small JSON Schema subset.
//! - [`CopilotRuntime`]: the port a runtime adapter implements. [`FakeRuntime`] is the scripted one for tests.
//! - [`Limits`] / [`RunGate`]: a 60 s deadline the crate owns (the adapter runs on a worker thread and gets a [`CancelToken`]), size caps, one run per person per feature.
//! - [`Copilot`]: ties them together. `features()` and `draft()` are the two calls.
//!
//! What is not here: any process or network code (the adapters are elsewhere), logging (the
//! office implements [`RunLog`]; a [`RunRecord`] has no body in it), and the wire types (they
//! move to `org-proto` when the office wires the calls).
//!
//! Depends on `serde` and `serde_json` only. Nothing depends on a runtime or on the office.

#![deny(unsafe_code)]

mod builtin;
mod contract;
mod fake;
mod feature;
mod limits;
pub mod prompt;
mod runtime;
mod schema;
mod secrets;
mod service;

pub use builtin::{builtin, SENTENCE_MAX_CHARS, TEXT_MAX_CHARS};
pub use contract::{ContextItem, DraftRequest, DraftResponse, FeatureInfo};
pub use fake::{FakeRuntime, Reply};
pub use feature::{Feature, FeatureError, Registry, MAX_NOTES, MAX_NOTE_CHARS};
pub use limits::{Limits, RunGate, RunPermit};
pub use runtime::{CancelToken, CopilotRuntime, RunOutput, RunRequest, RuntimeError, Usage};
pub use schema::{Schema, SchemaError, Violation};
pub use secrets::{contains_secret, text_has_secret};
pub use service::{Copilot, CopilotError, FailReason, InvalidReason, Outcome, RunLog, RunRecord};

#[cfg(test)]
mod tests;
