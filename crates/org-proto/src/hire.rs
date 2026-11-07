//! Hire: HR proposal → dry-run preview → provisioning.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{AgentId, MachineId, ProjectKey, RoleId, WorkspacePath};
use crate::model::{Agent, ReportsTo, Tools};

/// Which project the owner asked for, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectRequest {
    Existing { key: ProjectKey },
    New,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HireRequest {
    /// "What do you need?" text. Empty when a role is picked from the catalog.
    pub description: String,
    /// `null` = let HR decide.
    pub project: Option<ProjectRequest>,
    /// Set when the owner picks a role from the catalog (skips HR's proposal).
    pub role_id: Option<RoleId>,
}

/// A field HR proposed: `settled`, or `needs_choice` when it couldn't settle it (OQ-26).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Proposed<T> {
    Settled { value: T },
    NeedsChoice { value: Option<T> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NewProjectDraft {
    pub key: ProjectKey,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NewRoleDraft {
    pub id: RoleId,
    pub title: String,
    pub department: String,
    pub summary: String,
    pub responsibilities: Vec<String>,
    pub boundaries: Vec<String>,
    pub tools: Tools,
    pub directories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoleChoice {
    Catalog { role_id: RoleId },
    New { draft: NewRoleDraft },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectChoice {
    Existing { key: ProjectKey },
    New { draft: NewProjectDraft },
}

/// The agent's name as HR proposed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProposedName {
    pub value: String,
    /// True when the owner gave no name and HR picked one from the name pool.
    pub picked_by_hr: bool,
}

/// HR's proposal (step 2). HR writes nothing while proposing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HireSuggestion {
    pub machine_id: MachineId,
    pub draft_id: String,
    pub role: Proposed<RoleChoice>,
    /// With level, e.g. "Principal Product Designer".
    pub title: Proposed<String>,
    /// `picked_by_hr` when the owner gave no name.
    pub name: Proposed<ProposedName>,
    /// `needs_choice` with `value: null` means HR has no suggestion.
    // Rust: JSON can't tell that apart from "HR suggests no focus", so producers never build
    // `NeedsChoice { value: Some(None) }` (it reads back as `None`).
    pub focus: Proposed<Option<String>>,
    pub project: Proposed<ProjectChoice>,
    pub reports_to: Proposed<ReportsTo>,
    /// HR's one-line reason.
    pub reason: String,
}

/// What the owner settled on; input to the dry run and to provisioning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HirePlan {
    pub role: RoleChoice,
    pub title: String,
    /// Empty: HR picks one from the name pool.
    pub name: String,
    pub focus: Option<String>,
    pub project: ProjectChoice,
    pub reports_to: ReportsTo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HireField {
    Role,
    Title,
    Name,
    Focus,
    Project,
    ProjectKey,
    ReportsTo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FieldError {
    pub field: HireField,
    /// Plain words, e.g. "ada is already a name on this project".
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FileChange {
    Created,
    Updated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PlannedFile {
    pub path: WorkspacePath,
    pub change: FileChange,
}

/// A new role as the preview will write it: the draft plus where it goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NewRolePreview {
    #[serde(flatten)]
    pub draft: NewRoleDraft,
    pub path: WorkspacePath,
}

/// Same content as `hr.py hire … --dry-run`. Instant; no model call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HirePreview {
    pub machine_id: MachineId,
    pub agent_id: AgentId,
    pub name: String,
    pub title: String,
    pub focus: Option<String>,
    pub project: ProjectKey,
    pub home: WorkspacePath,
    pub registration: WorkspacePath,
    pub reports_to: ReportsTo,
    pub model: String,
    pub tools: Tools,
    pub peers: Vec<AgentId>,
    pub session_command: String,
    pub files: Vec<PlannedFile>,
    pub new_project: Option<NewProjectDraft>,
    pub new_role: Option<NewRolePreview>,
    pub errors: Vec<FieldError>,
    /// Opaque token for the workspace state the preview was computed against. Provisioning
    /// refuses (nothing written) if the workspace changed since (HIRE-16).
    pub workspace_revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HireStep {
    CreateProject,
    SaveRole,
    CreateHome,
    AddToRoster,
    Register,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Running,
    Done,
}

/// Every variant names its Hire draft: the `draft_id` from `propose_hire`, which `hire` takes
/// back, so one panel follows one draft from HR's proposal to the hired agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HireProgress {
    HrWorking {
        draft_id: String,
        message: String,
    },
    SuggestionReady {
        draft_id: String,
        suggestion: HireSuggestion,
    },
    HrFailed {
        draft_id: String,
        message: String,
        details: String,
    },
    Step {
        draft_id: String,
        step: HireStep,
        state: StepState,
    },
    Hired {
        draft_id: String,
        agent: Agent,
        files: Vec<PlannedFile>,
    },
    WorkspaceChanged {
        draft_id: String,
        preview: HirePreview,
    },
    Failed {
        draft_id: String,
        step: HireStep,
        reason: String,
        /// Files that exist now, from comparing the preview's list with the disk.
        created: Vec<WorkspacePath>,
        details: String,
    },
}

impl HireProgress {
    pub fn draft_id(&self) -> &str {
        match self {
            HireProgress::HrWorking { draft_id, .. }
            | HireProgress::SuggestionReady { draft_id, .. }
            | HireProgress::HrFailed { draft_id, .. }
            | HireProgress::Step { draft_id, .. }
            | HireProgress::Hired { draft_id, .. }
            | HireProgress::WorkspaceChanged { draft_id, .. }
            | HireProgress::Failed { draft_id, .. } => draft_id,
        }
    }
}
