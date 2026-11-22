//! Organization processes, standards, and built-in skills wire contract (`doc-34` §6,
//! `doc-28` §3.5, `TASK-44`, `FC-110`, `FC-111`, `FC-112`, `FC-184`).

use serde::{Deserialize, Serialize};

use crate::Call;

/// Whether a structure document is an organization `process` or `standard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKindWire {
    Process,
    Standard,
    #[serde(other)]
    Unknown,
}

/// Lifecycle state of a structure document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentStatusWire {
    Draft,
    Current,
    Deprecated,
    #[serde(other)]
    Unknown,
}

/// Scope of a structure document (`doc-34` §2.2 & §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum ProcessScopeWire {
    Organization,
    Team { team_id: String },
    Project { project_id: String },
}

/// Resolution target for `process.effective`, `process.why`, and `process.show` (`doc-34` §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProcessForWire {
    Current,
    Team { team_id: String },
    Project { project_id: String },
    Task { task_id: String },
}

/// Wire representation of one organization process or standard document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessDocumentWire {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub revision: u64,
    pub kind: DocumentKindWire,
    pub title: String,
    pub status: DocumentStatusWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_name: Option<String>,
    pub level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_name: Option<String>,
    pub overridable: bool,
    pub summary: String,
    pub body_markdown: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub created_by_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub updated_by_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by_id: Option<String>,
}

/// One layer in the single-chain resolution trace (`process.why`, `PROC-8`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessLayerWire {
    pub document: ProcessDocumentWire,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One resolved row in `process.effective` / `process.view`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveProcessEntryWire {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub kind: DocumentKindWire,
    pub applies: bool,
    pub mandatory: bool,
    pub winner_kind: Option<String>,
    pub document: ProcessDocumentWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<ProcessDocumentWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    pub chain: Vec<ProcessLayerWire>,
}

// ── process.effective ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessEffectiveParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#for: Option<ProcessForWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DocumentKindWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessEffectiveResult {
    pub scope_chain: Vec<String>,
    pub entries: Vec<EffectiveProcessEntryWire>,
    pub problems: Vec<ProcessLayerWire>,
    pub computed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_since: Option<String>,
}

pub struct ProcessEffective;

impl Call for ProcessEffective {
    const METHOD: &'static str = "process.effective";
    type Params = ProcessEffectiveParams;
    type Result = ProcessEffectiveResult;
}

// ── process.why ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessWhyParams {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#for: Option<ProcessForWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessWhyResult {
    pub key: String,
    pub chain: Vec<ProcessLayerWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<ProcessDocumentWire>,
}

pub struct ProcessWhy;

impl Call for ProcessWhy {
    const METHOD: &'static str = "process.why";
    type Params = ProcessWhyParams;
    type Result = ProcessWhyResult;
}

// ── process.show ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessShowParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#for: Option<ProcessForWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessShowResult {
    pub document: ProcessDocumentWire,
    pub blob: String,
    pub content: String,
    pub total_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ProcessDocumentWire>,
}

pub struct ProcessShow;

impl Call for ProcessShow {
    const METHOD: &'static str = "process.show";
    type Params = ProcessShowParams;
    type Result = ProcessShowResult;
}

// ── process.view ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessViewParams {
    pub scope: ProcessScopeWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DocumentKindWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessViewResult {
    pub scope: ProcessScopeWire,
    pub scope_name: String,
    pub rows: Vec<EffectiveProcessEntryWire>,
    pub inactive: Vec<ProcessDocumentWire>,
    pub problems: Vec<ProcessLayerWire>,
    pub computed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_since: Option<String>,
}

pub struct ProcessView;

impl Call for ProcessView {
    const METHOD: &'static str = "process.view";
    type Params = ProcessViewParams;
    type Result = ProcessViewResult;
}

// ── process.write ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessWriteParams {
    pub command_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    pub kind: DocumentKindWire,
    pub scope: ProcessScopeWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub title: String,
    pub status: DocumentStatusWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    #[serde(default = "default_overridable")]
    pub overridable: bool,
    #[serde(default)]
    pub body_markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

fn default_overridable() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessWriteResult {
    pub document_id: String,
    pub revision: u64,
    pub document: ProcessDocumentWire,
}

pub struct ProcessWrite;

impl Call for ProcessWrite {
    const METHOD: &'static str = "process.write";
    type Params = ProcessWriteParams;
    type Result = ProcessWriteResult;
}

// ── skill.list / skill.read ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillListParams {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSummaryWire {
    pub key: String,
    pub title: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillListResult {
    pub skills: Vec<SkillSummaryWire>,
}

pub struct SkillList;

impl Call for SkillList {
    const METHOD: &'static str = "skill.list";
    type Params = SkillListParams;
    type Result = SkillListResult;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReadParams {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReadResult {
    pub key: String,
    pub title: String,
    pub description: String,
    pub source: String,
    pub body_markdown: String,
}

pub struct SkillRead;

impl Call for SkillRead {
    const METHOD: &'static str = "skill.read";
    type Params = SkillReadParams;
    type Result = SkillReadResult;
}
