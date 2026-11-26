//! Projects: their teams, keys, repositories and areas.

use serde::{Deserialize, Serialize};

/// A team working on a project. Usually one per project; several teams can share one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTeam {
    pub id: String,
    pub project_id: String,
    pub team_id: String,
    pub version: u64,
}

/// The longest project key, in characters. A key is 2 to this many lowercase ASCII letters and
/// digits, starting with a letter.
pub const PROJECT_KEY_MAX_CHARS: usize = 10;

/// Project keys no project may have: they would read as another kind of reference
/// (`REQ-14`, `FEAT-0007`, `ADR-0004`, …).
pub const RESERVED_PROJECT_KEYS: &[&str] = &["req", "feat", "adr", "eval", "debt", "lim", "exc"];

/// The longest project `description`, in characters.
pub const PROJECT_DESCRIPTION_MAX_CHARS: usize = 1_000;

/// The longest area name, in characters (after trimming).
pub const AREA_NAME_MAX_CHARS: usize = 40;

/// The branch-name prefixes the office makes and owns: task branches (`task/…`), docs
/// branches (`docs/…`) and the tags it leaves after a merge or an archive (`merged/…`,
/// `archive/…`). A project's `integration_branch` may not start with one
/// ([`has_reserved_branch_prefix`]).
pub const RESERVED_BRANCH_PREFIXES: &[&str] = &["task/", "docs/", "merged/", "archive/"];

/// Whether `name` starts with one of [`RESERVED_BRANCH_PREFIXES`], so it can't be a project's
/// `integration_branch`: `project.create` and `project.update` refuse it with
/// `invalid_request` (docs/design/processes.md, "Project rule"), the one rule the office and
/// the app's mocks are to share.
///
/// The office reads the integration branch as the branch `refs/heads/<name>`, never through
/// git's short-name lookup, so `name` can't reach a tag or another ref. As defence in depth,
/// this is stricter than a plain prefix test: everything is compared ignoring ASCII case (refs
/// can live on a case-insensitive file system), and one leading `refs/heads/` or `heads/` is
/// looked through.
pub fn has_reserved_branch_prefix(name: &str) -> bool {
    let name = ["refs/heads/", "heads/"]
        .iter()
        .find_map(|lead| strip_prefix_ignoring_ascii_case(name, lead))
        .unwrap_or(name);
    RESERVED_BRANCH_PREFIXES
        .iter()
        .any(|prefix| strip_prefix_ignoring_ascii_case(name, prefix).is_some())
}

/// `name` after `prefix`, when it starts with it ignoring ASCII case.
fn strip_prefix_ignoring_ascii_case<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let n = prefix.len();
    (name.len() >= n && name.is_char_boundary(n) && name[..n].eq_ignore_ascii_case(prefix))
        .then(|| &name[n..])
}

/// A project: the full record. It replaced `ProjectSummary` (`{ id, name }`) additively: every
/// other field has a default, so a record from before reads with an empty `key`, no repository,
/// not archived and `version` 0. [`ProjectSummary`] is kept as another name for it.
///
/// `#[non_exhaustive]`: fields may be added. Code outside this crate builds one with
/// [`Project::new`] and sets the fields it needs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Project {
    pub id: String,
    /// The short key used in references and branches (`chk` in `chk-T-12`): 2 to
    /// [`PROJECT_KEY_MAX_CHARS`] lowercase ASCII letters and digits, starting with a letter, not
    /// one of [`RESERVED_PROJECT_KEYS`], unique in the organization. It never changes. Empty
    /// only on a record from before keys existed.
    #[serde(default)]
    pub key: String,
    /// Follows the organization-name rules; unique among projects, ignoring case.
    pub name: String,
    /// At most [`PROJECT_DESCRIPTION_MAX_CHARS`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The project's repository, as HQ resolved it from the [`RepoSpec`] it was created with.
    /// Absent: the project has no repository (its tasks and areas work; docs and worktrees
    /// don't).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<ProjectRepo>,
    /// The project's integration branch: task branches start from it, the office reads the
    /// project's instruction files, guidance files and processes from it (and the Docs tab its
    /// docs), and a task branch counts as merged once its work is reachable from it
    /// (docs/design/processes.md, A56 to A58). Absent: the repository's default branch.
    /// Never starts with a reserved prefix ([`has_reserved_branch_prefix`]): `project.create`
    /// and `project.update` refuse one with `invalid_request`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_branch: Option<String>,
    /// No new work items in it; everything else still works.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    /// When HQ created it (RFC 3339 UTC with milliseconds, HQ's clock). Missing on records from
    /// before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// 0 only on a record from before projects had versions.
    #[serde(default)]
    pub version: u64,
}

impl Project {
    /// A project with this id and name and every other field at its default.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Self::default()
        }
    }
}

/// The name the project record had before it became [`Project`]. Same type, same wire shape.
pub type ProjectSummary = Project;

/// Where a project's repository is hosted. A value this build doesn't know, in any shape,
/// decodes as [`RepoHost::Unknown`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RepoHost {
    /// A bare repository on HQ's Mac.
    Local,
    /// A GitHub repository.
    Github,
    /// Missing or not known to this build.
    #[default]
    Unknown,
}

impl<'de> Deserialize<'de> for RepoHost {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Like `AgentStatus`: reading never fails on a well-formed document.
        let value = serde_json::Value::deserialize(d)?;
        Ok(match value.as_str() {
            Some("local") => RepoHost::Local,
            Some("github") => RepoHost::Github,
            _ => RepoHost::Unknown,
        })
    }
}

/// A project's repository. It never holds a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRepo {
    #[serde(default)]
    pub host: RepoHost,
    pub default_branch: String,
    /// Local only: the bare repository's absolute path on HQ's Mac. **Visible
    /// organization-wide**, like an agent's `home`. Clients show it and never open it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// GitHub only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GithubRepo>,
}

/// A GitHub repository, as a [`ProjectRepo`] names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubRepo {
    pub owner: String,
    pub name: String,
    pub repo_id: u64,
    #[serde(default)]
    pub private: bool,
}

/// How `project.create` gets its repository. HQ resolves it into the [`ProjectRepo`] the
/// event carries; a client never sends a `ProjectRepo`. On the wire `{"new_local": {}}` or
/// `{"existing_local": {"path": "<absolute path>"}}`. More kinds (GitHub) are added later, so
/// a server that doesn't know a kind refuses the command with `invalid_request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RepoSpec {
    /// HQ creates a bare repository with an initial commit on `main` under its workspace,
    /// before the command (recording the intent first and reusing the folder on a retry).
    NewLocal {},
    /// An existing repository on HQ's Mac, checked before the command (it exists, is a git
    /// repository and has its default branch).
    ExistingLocal { path: String },
}

/// A part of a project, for routing bugs: one owning team, an optional lead. Areas are flat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    pub project_id: String,
    /// 1 to [`AREA_NAME_MAX_CHARS`] characters (organization-name rules), unique in its
    /// project among areas that aren't deleted, ignoring case.
    pub name: String,
    /// One of the project's teams. Empty only while the project has no team, or after its team
    /// left as the project's last one or was deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// A member of `team_id`; never the owner or an offboarded agent. Empty: the team's lead
    /// stands in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_member_id: Option<String>,
    /// The project's General area: exactly one per project, made by `project.create`, never
    /// deleted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub general: bool,
    /// A tombstone: a deleted area stays so closed bugs can still show its name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
    /// Set on a deleted area: where its open bugs went.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<String>,
    pub version: u64,
}
