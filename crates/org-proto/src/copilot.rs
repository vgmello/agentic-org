//! Wire contract for `copilot.features` and `copilot.draft` (`TASK-60` AC #3).
//!
//! The copilot only produces editable drafts (`text.improve`, `bug.from_sentence`) and never
//! persists or executes actions. When no tool-less agent runtime is configured, `copilot.features`
//! returns an empty list and `copilot.draft` fails closed with `ErrorCode::CopilotUnavailable`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Call, Empty};

/// One entry of `copilot.features`: what the browser may offer a smart button for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopilotFeatureInfo {
    pub id: String,
    pub version: u32,
    pub title: String,
}

/// Result of `copilot.features`. Empty when no runtime is set up.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CopilotFeaturesResult {
    #[serde(default)]
    pub features: Vec<CopilotFeatureInfo>,
}

/// `copilot.features`: list smart-drafting features currently offered by this office.
pub struct CopilotFeatures;

impl Call for CopilotFeatures {
    const METHOD: &'static str = "copilot.features";
    type Params = Empty;
    type Result = CopilotFeaturesResult;
}

/// One piece of workspace text handed to `copilot.draft` as fenced data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopilotContextItem {
    pub kind: String,
    pub label: String,
    pub text: String,
}

/// Parameters of `copilot.draft`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopilotDraftParams {
    pub feature: String,
    pub input: Value,
    #[serde(default)]
    pub context: Vec<CopilotContextItem>,
}

/// Result of `copilot.draft`: a validated draft value and short model notes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotDraftResult {
    pub draft: Value,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// `copilot.draft`: request a single bounded, fenced, schema-validated draft.
pub struct CopilotDraft;

impl Call for CopilotDraft {
    const METHOD: &'static str = "copilot.draft";
    type Params = CopilotDraftParams;
    type Result = CopilotDraftResult;
}
