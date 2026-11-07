//! The workspace model: plain data, serializable, no I/O.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

/// Everything read from a workspace. Built by [`crate::Workspace::load`] and kept
/// current with [`WorkspaceModel::refresh`].
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkspaceModel {
    /// The canonical workspace root (see [`WorkspaceModel::root`]).
    pub(crate) root: PathBuf,
    /// Projects by key (the folder name under `projects/`).
    pub projects: BTreeMap<String, Project>,
    /// The role catalog by role id (`hr/roles/<id>.toml`, without `_template.toml`).
    pub roles: BTreeMap<String, Role>,
    /// The HR staff function (`.claude/agents/hr.md`), if it could be read.
    pub hr: Option<StaffFunction>,
    /// Everything that was missing or could not be read, one entry per issue,
    /// each tied to the file it is about. Never fatal.
    pub problems: Vec<Problem>,
}

/// A project: `projects/<key>/`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Project {
    /// The folder name, which is the project key.
    pub key: String,
    /// Workspace-relative folder, e.g. `projects/pay`.
    pub dir: String,
    /// `project.json`, if present and readable.
    pub profile: Option<ProjectProfile>,
    /// `CLAUDE.md` (the project brief), if present and readable.
    pub brief: Option<ProjectBrief>,
    /// Agents by id (the folder name under `agents/`).
    pub agents: BTreeMap<String, Agent>,
}

/// `projects/<key>/project.json`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProjectProfile {
    pub key: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub created_on: Option<String>,
    pub home: Option<String>,
    /// Fields this crate doesn't know, kept as they are.
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// `projects/<key>/CLAUDE.md`.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ProjectBrief {
    /// The first level-1 heading (`# <name>`).
    pub title: Option<String>,
    /// The body of the `## Goals` section, trimmed.
    pub goals: Option<String>,
    /// The body of the `## Context` section, trimmed.
    pub context: Option<String>,
    /// The team roster between the `hr:team` markers. `None` when the markers are missing.
    pub team: Option<Roster>,
}

/// The team roster table HR keeps in a project brief.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Roster {
    pub members: Vec<RosterMember>,
    /// Table rows that could not be read.
    pub unreadable: Vec<UnreadableRow>,
}

/// One roster row: `| Agent | Name | Role | Focus | Reports to |`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RosterMember {
    pub line_no: usize,
    /// The agent id, with the surrounding backticks removed.
    pub agent: String,
    pub name: String,
    /// The `Role` column, which holds the agent's title.
    pub title: String,
    /// `None` for `-` or an empty cell.
    pub focus: Option<String>,
    pub reports_to: String,
}

/// A row of a Markdown table that could not be read.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UnreadableRow {
    /// 1-based line number in the file.
    pub line_no: usize,
    /// The line as written.
    pub raw: String,
    pub reason: String,
}

/// An agent: `projects/<key>/agents/<id>/`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Agent {
    /// The folder name, which is the agent id.
    pub id: String,
    /// Workspace-relative folder, e.g. `projects/pay/agents/pay-staff-engineer-sam`.
    pub dir: String,
    /// `agent.json`, if present and readable.
    pub profile: Option<AgentProfile>,
    /// `tasks/BACKLOG.md`, if present and readable.
    pub backlog: Option<Backlog>,
}

/// `agent.json`. Every field is optional so that a hand-edited file still loads;
/// a field with the wrong type is reported as a problem and left `None`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AgentProfile {
    pub id: Option<String>,
    pub name: Option<String>,
    pub project: Option<String>,
    pub role: Option<String>,
    pub title: Option<String>,
    pub department: Option<String>,
    pub focus: Option<String>,
    pub reports_to: Option<String>,
    pub model: Option<String>,
    pub tools: Option<Tools>,
    pub status: Option<AgentStatus>,
    pub hired_on: Option<String>,
    pub offboarded_on: Option<String>,
    pub home: Option<String>,
    pub registration: Option<String>,
    /// Fields this crate doesn't know, kept as they are.
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl AgentProfile {
    /// Whether HR lists the agent as active (hr.py treats anything else as not active).
    pub fn is_active(&self) -> bool {
        self.status == Some(AgentStatus::Active)
    }
}

/// `agent.json` `tools`: the string `"inherit"` or a list of tool names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tools {
    Inherit,
    List(Vec<String>),
}

impl Serialize for Tools {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Inherit => s.serialize_str("inherit"),
            Self::List(tools) => tools.serialize(s),
        }
    }
}

/// `agent.json` `status`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AgentStatus {
    Active,
    Offboarded,
    /// Any other value, as written.
    Other(String),
}

impl AgentStatus {
    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "active" => Self::Active,
            "offboarded" => Self::Offboarded,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Offboarded => "offboarded",
            Self::Other(s) => s,
        }
    }
}

impl Serialize for AgentStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// An agent's `tasks/BACKLOG.md`.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Backlog {
    /// The name in the `# Backlog · <name>` heading, if any.
    pub owner: Option<String>,
    /// Rows in file order: tasks, and rows that could not be read.
    pub items: Vec<BacklogItem>,
}

impl Backlog {
    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.items.iter().filter_map(|item| match item {
            BacklogItem::Task(task) => Some(task),
            BacklogItem::Unreadable { .. } => None,
        })
    }

    pub fn unreadable(&self) -> impl Iterator<Item = &BacklogItem> {
        self.items
            .iter()
            .filter(|item| matches!(item, BacklogItem::Unreadable { .. }))
    }
}

/// One row of a backlog table.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BacklogItem {
    Task(Task),
    /// A row the parser could not read. The UI shows it as "can't read this row".
    Unreadable {
        line_no: usize,
        raw: String,
        reason: String,
    },
}

impl BacklogItem {
    /// The row's 1-based line number in the file.
    pub fn line_no(&self) -> usize {
        match self {
            Self::Task(task) => task.line_no,
            Self::Unreadable { line_no, .. } => *line_no,
        }
    }
}

/// A readable backlog row: `| # | Task | Status | Requested by | Notes |`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Task {
    /// 1-based line number in the file.
    pub line_no: usize,
    /// The `#` column as a number, when it is one (`3`, `#3`, `3.`).
    pub number: Option<u32>,
    pub task: String,
    /// The status, normalized from the leading word of the status cell.
    pub status: TaskStatus,
    /// The status cell as written, e.g. `in-progress (awaiting go)`.
    pub status_raw: String,
    pub requested_by: String,
    pub notes: String,
    /// Things the parser tolerated in this row, e.g. a missing trailing cell.
    pub warnings: Vec<String>,
}

/// A task status. The template defines `todo`, `in-progress`, `blocked` and `done`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    Todo,
    InProgress,
    Blocked,
    Done,
    /// Any other status, as written (trimmed).
    Other(String),
}

impl TaskStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in-progress",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Other(s) => s,
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for TaskStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// A role spec: `hr/roles/<id>.toml`. Missing or mistyped required fields are
/// reported as problems and left empty.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Role {
    /// The file stem, e.g. `staff-engineer`.
    pub id: String,
    pub title: Option<String>,
    pub department: Option<String>,
    pub summary: Option<String>,
    pub mission: Option<String>,
    pub responsibilities: Vec<String>,
    pub working_style: Vec<String>,
    pub boundaries: Vec<String>,
    pub aliases: Vec<String>,
    /// `inherit` when not set.
    pub model: String,
    /// `None` means the role inherits every tool.
    pub tools: Option<Vec<String>>,
    /// Role-specific folders and their purpose.
    pub directories: BTreeMap<String, String>,
    /// Keys this crate doesn't know, kept as JSON values.
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// A staff function defined by a Claude Code agent file, such as HR (`.claude/agents/hr.md`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StaffFunction {
    /// Workspace-relative path of the definition.
    pub path: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub model: Option<String>,
    /// `None` when the frontmatter has no `tools` line (every tool is inherited).
    pub tools: Option<Vec<String>>,
}

/// Something the loader couldn't read or found inconsistent, tied to a file.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Problem {
    /// Workspace-relative path (forward slashes) of the file or folder concerned.
    pub path: String,
    pub severity: Severity,
    pub kind: ProblemKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Data is missing from the model because of this.
    Error,
    /// The model is complete, but something was tolerated or looks off.
    Warning,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// An expected file is missing.
    Missing,
    /// The file exists but could not be read (I/O error, too large, outside the workspace).
    Unreadable,
    /// The file could be read but not parsed (invalid JSON or TOML, no table).
    Malformed,
    /// A field is missing or has the wrong type.
    InvalidField,
    /// One or more table rows could not be read.
    UnreadableRows,
    /// The file disagrees with its location, e.g. an `id` that isn't the folder name.
    Inconsistent,
}

/// A problem found by a parser, before it is tied to a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    pub kind: ProblemKind,
    pub message: String,
}

impl Issue {
    pub fn error(kind: ProblemKind, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            kind,
            message: message.into(),
        }
    }

    pub fn warning(kind: ProblemKind, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            kind,
            message: message.into(),
        }
    }

    /// Ties the issue to a workspace-relative path.
    pub fn at(self, path: &str) -> Problem {
        Problem {
            path: path.to_string(),
            severity: self.severity,
            kind: self.kind,
            message: self.message,
        }
    }
}

/// The result of parsing one file's text: a value plus the issues tolerated along the way.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed<T> {
    pub value: T,
    pub issues: Vec<Issue>,
}

impl<T> Parsed<T> {
    pub fn new(value: T, issues: Vec<Issue>) -> Self {
        Self { value, issues }
    }
}

impl WorkspaceModel {
    /// The canonical workspace root that [`WorkspaceModel::refresh`] reads from.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A path under the root, relative to it (e.g. for a watcher's absolute
    /// event paths). `None` if the path isn't under the canonical root.
    pub fn relative_path(&self, path: impl AsRef<Path>) -> Option<PathBuf> {
        path.as_ref()
            .strip_prefix(&self.root)
            .ok()
            .map(Path::to_path_buf)
    }

    pub fn project(&self, key: &str) -> Option<&Project> {
        self.projects.get(key)
    }

    /// Finds an agent by id in any project.
    pub fn agent(&self, id: &str) -> Option<&Agent> {
        self.projects.values().find_map(|p| p.agents.get(id))
    }

    pub fn agents(&self) -> impl Iterator<Item = &Agent> {
        self.projects.values().flat_map(|p| p.agents.values())
    }

    /// Problems about one workspace-relative path.
    pub fn problems_for<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a Problem> + 'a {
        self.problems.iter().filter(move |p| p.path == path)
    }
}
