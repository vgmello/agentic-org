//! Wire contracts for C2 HR hiring, role catalog, team member limits, `may_hire` permission,
//! and agent lifecycle / execution configuration (`TASK-41` / C2, `ADR-0029`, `FC-031`,
//! `FC-033`, `FC-034`, `FC-097`, `FC-108`).

use serde::{Deserialize, Serialize};

use crate::Call;

/// A role record returned by `role.list` and `role.inspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleRecord {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub name: String,
    pub body: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by_id: Option<String>,
}

/// Proposal payload carried on hire requests and confirmations.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HireProposalWire {
    pub name: String,
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_in_team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reports_to_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub office_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_config: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// A hire request record returned by `hr.hire.list` and `hr.hire.inspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HireRequestRecord {
    pub id: String,
    pub revision: u64,
    pub state: String,
    pub requested_by_employee_id: String,
    pub home_team_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_id: Option<String>,
    pub proposal: HireProposalWire,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_step: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Agent execution configuration returned by `agent.execution.get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionRecord {
    pub employee_id: String,
    pub model_profile_id: String,
    pub model_id: String,
    #[serde(default)]
    pub tools_config: Vec<String>,
    pub permission_preset: String,
    pub enabled: bool,
    pub system_prompt_markdown: String,
    pub revision: u64,
}

/// Model profile summary returned by `model_profile.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfileRecord {
    pub id: String,
    pub key: String,
    pub name: String,
    pub description: String,
    pub is_auto_generated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_for_connection_id: Option<String>,
    pub archived: bool,
    pub connection_ids: Vec<String>,
    pub revision: u64,
}

/// Reply of a hire write (`hr.hire.draft`, `hr.hire.confirm`, `hr.hire`, `hr.hire.withdraw`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HireOpResult {
    pub request_id: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_step: Option<String>,
    pub revision: u64,
    pub generation: u64,
}

/// Reply of a role, team-limit, `may_hire`, execution, or agent lifecycle write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityOpResult {
    pub id: String,
    pub revision: u64,
    pub generation: u64,
}

// ---- Role calls ----

pub struct RoleList;
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleListParams {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleListResult {
    pub roles: Vec<RoleRecord>,
}
impl Call for RoleList {
    const METHOD: &'static str = "role.list";
    type Params = RoleListParams;
    type Result = RoleListResult;
}

pub struct RoleInspect;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleInspectParams {
    pub role: String,
}
impl Call for RoleInspect {
    const METHOD: &'static str = "role.inspect";
    type Params = RoleInspectParams;
    type Result = RoleRecord;
}

pub struct RoleCreate;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleCreateParams {
    pub command_id: String,
    pub role_id: String,
    #[serde(default)]
    pub key: Option<String>,
    pub name: String,
    #[serde(default)]
    pub body: String,
}
impl Call for RoleCreate {
    const METHOD: &'static str = "role.create";
    type Params = RoleCreateParams;
    type Result = EntityOpResult;
}

pub struct RoleUpdate;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleUpdateParams {
    pub command_id: String,
    pub role_id: String,
    pub expected_version: u64,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub clear_key: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
}
impl Call for RoleUpdate {
    const METHOD: &'static str = "role.update";
    type Params = RoleUpdateParams;
    type Result = EntityOpResult;
}

// ---- HR hire calls ----

pub struct HrHireList;
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HrHireListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HrHireListResult {
    pub hires: Vec<HireRequestRecord>,
}
impl Call for HrHireList {
    const METHOD: &'static str = "hr.hire.list";
    type Params = HrHireListParams;
    type Result = HrHireListResult;
}

pub struct HrHireInspect;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HrHireInspectParams {
    pub request_id: String,
}
impl Call for HrHireInspect {
    const METHOD: &'static str = "hr.hire.inspect";
    type Params = HrHireInspectParams;
    type Result = HireRequestRecord;
}

pub struct HrHireDraft;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HrHireDraftParams {
    pub command_id: String,
    pub request_id: String,
    pub home_team_id: String,
    #[serde(default)]
    pub role_id: Option<String>,
    pub proposal: HireProposalWire,
    pub reason: String,
}
impl Call for HrHireDraft {
    const METHOD: &'static str = "hr.hire.draft";
    type Params = HrHireDraftParams;
    type Result = HireOpResult;
}

pub struct HrHireConfirm;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HrHireConfirmParams {
    pub command_id: String,
    pub request_id: String,
    pub employee_id: String,
    pub expected_version: u64,
    #[serde(default)]
    pub fail_at_step: Option<String>,
}
impl Call for HrHireConfirm {
    const METHOD: &'static str = "hr.hire.confirm";
    type Params = HrHireConfirmParams;
    type Result = HireOpResult;
}

pub struct HrHire;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HrHireParams {
    pub command_id: String,
    pub request_id: String,
    pub employee_id: String,
    pub home_team_id: String,
    #[serde(default)]
    pub role_id: Option<String>,
    pub proposal: HireProposalWire,
    pub reason: String,
}
impl Call for HrHire {
    const METHOD: &'static str = "hr.hire";
    type Params = HrHireParams;
    type Result = HireOpResult;
}

pub struct HrHireWithdraw;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HrHireWithdrawParams {
    pub command_id: String,
    pub request_id: String,
    pub expected_version: u64,
    pub closed_reason: String,
}
impl Call for HrHireWithdraw {
    const METHOD: &'static str = "hr.hire.withdraw";
    type Params = HrHireWithdrawParams;
    type Result = HireOpResult;
}

// ---- Permissions, team limits, agent lifecycle & execution ----

pub struct MemberSetMayHire;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSetMayHireParams {
    pub command_id: String,
    pub member_id: String,
    pub expected_version: u64,
    pub may_hire: bool,
}
impl Call for MemberSetMayHire {
    const METHOD: &'static str = "member.set_may_hire";
    type Params = MemberSetMayHireParams;
    type Result = EntityOpResult;
}

pub struct TeamSetMaxMembers;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamSetMaxMembersParams {
    pub command_id: String,
    pub team_id: String,
    pub expected_version: u64,
    pub max_members: u64,
}
impl Call for TeamSetMaxMembers {
    const METHOD: &'static str = "team.set_max_members";
    type Params = TeamSetMaxMembersParams;
    type Result = EntityOpResult;
}

pub struct AgentUpdate;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentUpdateParams {
    pub command_id: String,
    pub member_id: String,
    pub expected_version: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub role_id: Option<String>,
    #[serde(default)]
    pub clear_role_id: bool,
    #[serde(default)]
    pub reports_to_member_id: Option<String>,
    #[serde(default)]
    pub clear_reports_to: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub clear_title: bool,
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub clear_focus: bool,
    #[serde(default)]
    pub role_text: Option<String>,
    #[serde(default)]
    pub clear_role_text: bool,
    #[serde(default)]
    pub title_in_team: Option<String>,
    #[serde(default)]
    pub clear_title_in_team: bool,
    #[serde(default)]
    pub body: Option<String>,
}
impl Call for AgentUpdate {
    const METHOD: &'static str = "member.agent_update";
    type Params = AgentUpdateParams;
    type Result = EntityOpResult;
}

pub struct AgentMove;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMoveParams {
    pub command_id: String,
    pub member_id: String,
    pub expected_version: u64,
    pub to_team_id: String,
}
impl Call for AgentMove {
    const METHOD: &'static str = "member.agent_move";
    type Params = AgentMoveParams;
    type Result = EntityOpResult;
}

pub struct AgentSetStatus;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSetStatusParams {
    pub command_id: String,
    pub member_id: String,
    pub expected_version: u64,
    pub paused: bool,
}
impl Call for AgentSetStatus {
    const METHOD: &'static str = "member.agent_set_status";
    type Params = AgentSetStatusParams;
    type Result = EntityOpResult;
}

pub struct AgentOffboard;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOffboardParams {
    pub command_id: String,
    pub member_id: String,
    pub expected_version: u64,
}
impl Call for AgentOffboard {
    const METHOD: &'static str = "member.agent_offboard";
    type Params = AgentOffboardParams;
    type Result = EntityOpResult;
}

pub struct AgentExecutionGet;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionGetParams {
    pub employee_id: String,
}
impl Call for AgentExecutionGet {
    const METHOD: &'static str = "member.agent_execution_get";
    type Params = AgentExecutionGetParams;
    type Result = AgentExecutionRecord;
}

pub struct AgentExecutionSet;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExecutionSetParams {
    pub command_id: String,
    pub employee_id: String,
    #[serde(default)]
    pub expected_version: Option<u64>,
    pub model_profile_id: String,
    pub model_id: String,
    #[serde(default)]
    pub tools_config: Vec<String>,
    #[serde(default)]
    pub permission_preset: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub system_prompt_markdown: String,
}
fn default_enabled() -> bool {
    true
}
impl Call for AgentExecutionSet {
    const METHOD: &'static str = "member.agent_execution_set";
    type Params = AgentExecutionSetParams;
    type Result = EntityOpResult;
}

pub struct ProviderConnectionCreate;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConnectionCreateParams {
    pub command_id: String,
    pub connection_id: String,
    pub default_profile_id: String,
    pub key: String,
    pub name: String,
    pub provider_type: String,
    pub auth_mode: String,
    #[serde(default)]
    pub subscription_identity: Option<String>,
    pub runtime_driver: String,
    #[serde(default)]
    pub endpoint_url: Option<String>,
    #[serde(default = "default_turns")]
    pub max_concurrent_turns: u64,
}
fn default_turns() -> u64 {
    1
}
impl Call for ProviderConnectionCreate {
    const METHOD: &'static str = "provider.connection.create";
    type Params = ProviderConnectionCreateParams;
    type Result = EntityOpResult;
}

pub struct ModelProfileList;
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfileListParams {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfileListResult {
    pub profiles: Vec<ModelProfileRecord>,
}
impl Call for ModelProfileList {
    const METHOD: &'static str = "model_profile.list";
    type Params = ModelProfileListParams;
    type Result = ModelProfileListResult;
}
