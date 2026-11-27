//! Project directory reads and records (data contract document 02, B3).

use serde::{Deserialize, Serialize};

use crate::Call;

/// A project as reported by `project.list`, `project.inspect`, and `/api/v1/organizations/{org}/projects`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: String,
    pub number: u64,
    /// Human-readable reference `P-<number>`.
    pub key: String,
    pub slug: String,
    pub name: String,
    pub owner_team_id: String,
    #[serde(default)]
    pub contributing_team_ids: Vec<String>,
    #[serde(default)]
    pub archived: bool,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// The project's default definition of done (`TASK-88`): absent inherits the
    /// organization's, `[]` opts out, a list overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_dod: Option<Vec<String>>,
}

/// `project.list`: lists projects visible in this organization, ordered by number.
pub struct ProjectList;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectListParams {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectListResult {
    pub projects: Vec<ProjectRecord>,
}

impl Call for ProjectList {
    const METHOD: &'static str = "project.list";
    type Params = ProjectListParams;
    type Result = ProjectListResult;
}

/// `project.inspect`: inspects one project by UUIDv7 ID, `P-<number>` key, or canonical slug.
pub struct ProjectInspect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInspectParams {
    pub project: String,
}

impl Call for ProjectInspect {
    const METHOD: &'static str = "project.inspect";
    type Params = ProjectInspectParams;
    type Result = ProjectRecord;
}

/// Strict browser request payload for `POST /api/v1/organizations/{org}/projects`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCreateWebParams {
    pub project_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributing_team_ids: Vec<String>,
}

impl std::fmt::Debug for ProjectCreateWebParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectCreateWebParams")
            .field("project_id", &self.project_id)
            .field("name", &"<redacted>")
            .field("slug", &self.slug.as_ref().map(|_| "<redacted>"))
            .field("owner_team_id", &self.owner_team_id)
            .field("create_team_id", &self.create_team_id)
            .field("parent_team_id", &self.parent_team_id)
            .field("contributing_team_ids", &self.contributing_team_ids)
            .finish()
    }
}

/// Strict browser envelope for `POST /api/v1/organizations/{org}/projects`.
pub type ProjectCreateWebRequest = crate::web::CommandEnvelope<ProjectCreateWebParams>;
