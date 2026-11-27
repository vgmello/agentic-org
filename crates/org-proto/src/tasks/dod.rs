//! Default definitions of done (`TASK-88`): the organization's and each project's list of
//! step texts a new task copies into its checklist's `dod` section, and the one rule every
//! list of definition-of-done items follows.
//!
//! A project's `default_dod` absent inherits the organization's; `[]` opts the project out;
//! a list overrides. A task copies the list that applies when it is created, in the create's
//! own commit, so an office creates it offline and a later change to a default never changes
//! a task already created.

use serde::{Deserialize, Serialize};

use super::{TASK_MAX_DOD_ITEMS, TASK_STEP_TEXT_MAX_CHARS};
use crate::calls::Call;
use crate::{ApiError, ErrorCode};

/// The items of a definition of done, each trimmed: refused when one is empty, holds a
/// control character (a line break included) or is over [`TASK_STEP_TEXT_MAX_CHARS`], or
/// when there are more than [`TASK_MAX_DOD_ITEMS`].
pub fn normalize_dod_items(raw: &[String]) -> Result<Vec<String>, ApiError> {
    if raw.len() > TASK_MAX_DOD_ITEMS {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("a definition of done has at most {TASK_MAX_DOD_ITEMS} items"),
        ));
    }
    raw.iter()
        .map(|item| {
            let text = item.trim();
            if text.is_empty() {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "a definition-of-done item cannot be empty",
                ));
            }
            if text.chars().any(char::is_control) {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "a definition-of-done item is one line without control characters",
                ));
            }
            if text.chars().count() > TASK_STEP_TEXT_MAX_CHARS {
                return Err(ApiError::new(
                    ErrorCode::TooLarge,
                    format!(
                        "a definition-of-done item exceeds {TASK_STEP_TEXT_MAX_CHARS} characters"
                    ),
                ));
            }
            Ok(text.to_owned())
        })
        .collect()
}

/// `organization.default_dod.get`: the organization's default definition of done.
pub struct OrganizationDefaultDodGet;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationDefaultDodGetParams {}

/// The organization's default definition of done, and the organization record's revision a
/// change names as `expected_revision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationDefaultDodResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_dod: Option<Vec<String>>,
    pub revision: u64,
}

impl Call for OrganizationDefaultDodGet {
    const METHOD: &'static str = "organization.default_dod.get";
    type Params = OrganizationDefaultDodGetParams;
    type Result = OrganizationDefaultDodResult;
}

/// `organization.default_dod.set`: replace the organization's default definition of done
/// (absent or `null` clears it). The owner only, at HQ.
pub struct OrganizationDefaultDodSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationDefaultDodSetParams {
    pub command_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_dod: Option<Vec<String>>,
}

impl Call for OrganizationDefaultDodSet {
    const METHOD: &'static str = "organization.default_dod.set";
    type Params = OrganizationDefaultDodSetParams;
    type Result = OrganizationDefaultDodResult;
}

/// `project.default_dod.set`: replace a project's default definition of done (absent or
/// `null` inherits the organization's; `[]` opts out). The owner, or the lead of the
/// project's owning team or of a team above it, at HQ.
pub struct ProjectDefaultDodSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDefaultDodSetParams {
    pub command_id: String,
    /// The project's UUIDv7 ID, `P-<number>` key or slug.
    pub project: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_dod: Option<Vec<String>>,
}

impl Call for ProjectDefaultDodSet {
    const METHOD: &'static str = "project.default_dod.set";
    type Params = ProjectDefaultDodSetParams;
    type Result = crate::projects::ProjectRecord;
}
