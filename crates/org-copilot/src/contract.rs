//! The shapes of the `copilot.*` calls (see `docs/design/smart-features.md`).
//!
//! They live here until the office wires the calls; then they move to `org-proto`, additive.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::Usage;

/// One entry of `copilot.features`: what the app may offer a button for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInfo {
    pub id: String,
    pub version: u32,
    pub title: String,
}

/// A piece of workspace text the office resolved from a `context_ref`, passed as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItem {
    /// What it is (`task`, `project`, ...). A feature accepts only the kinds it lists.
    pub kind: String,
    pub label: String,
    pub text: String,
}

/// `copilot.draft`, with the `context_refs` already resolved by the office.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DraftRequest {
    pub feature: String,
    pub input: Value,
    #[serde(default)]
    pub context: Vec<ContextItem>,
}

/// The answer of `copilot.draft`: a draft to show, never an action taken.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DraftResponse {
    /// The feature's `draft_schema` value.
    pub draft: Value,
    /// Short remarks from the model (assumptions, gaps), at most 5.
    pub notes: Vec<String>,
    /// For the run log and the "Copilot" cost line; not part of the wire answer.
    #[serde(skip)]
    pub usage: Option<Usage>,
}
