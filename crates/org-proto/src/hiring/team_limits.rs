//! Wire contracts for manager-approved `team.max_members` increase requests and actionable
//! full-team capacity hints (`TASK-210`).

use serde::{Deserialize, Serialize};

use crate::Call;

/// Maximum open-capacity teams listed in a full-team error hint.
pub const MAX_SUGGESTED_OPEN_TEAMS: usize = 5;

/// A durable team `max_members` increase request returned by `team.limit_request.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamLimitRequestRecord {
    pub id: String,
    pub revision: u64,
    pub state: String,
    pub team_id: String,
    pub requested_by_employee_id: String,
    pub approver_employee_id: String,
    pub current_max_members: u64,
    pub requested_max_members: u64,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Reply of `team.limit_request.create` and `team.limit_request.decide`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamLimitRequestOpResult {
    pub request_id: String,
    pub team_id: String,
    pub state: String,
    pub approver_employee_id: String,
    pub requested_max_members: u64,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_revision: Option<u64>,
    pub generation: u64,
}

pub struct TeamLimitRequestCreate;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamLimitRequestCreateParams {
    pub command_id: String,
    pub request_id: String,
    pub team_id: String,
    pub requested_max_members: u64,
    pub reason: String,
}
impl Call for TeamLimitRequestCreate {
    const METHOD: &'static str = "team.limit_request.create";
    type Params = TeamLimitRequestCreateParams;
    type Result = TeamLimitRequestOpResult;
}

pub struct TeamLimitRequestDecide;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamLimitRequestDecideParams {
    pub command_id: String,
    pub request_id: String,
    pub expected_version: u64,
    pub approve: bool,
    #[serde(default)]
    pub reason: Option<String>,
}
impl Call for TeamLimitRequestDecide {
    const METHOD: &'static str = "team.limit_request.decide";
    type Params = TeamLimitRequestDecideParams;
    type Result = TeamLimitRequestOpResult;
}

pub struct TeamLimitRequestList;
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamLimitRequestListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver_employee_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamLimitRequestListResult {
    pub requests: Vec<TeamLimitRequestRecord>,
}
impl Call for TeamLimitRequestList {
    const METHOD: &'static str = "team.limit_request.list";
    type Params = TeamLimitRequestListParams;
    type Result = TeamLimitRequestListResult;
}

/// Pure input for [`format_team_full_hint`].
pub struct TeamFullHintInput<'a> {
    pub team_id: &'a str,
    pub current_max: u64,
    pub team_revision: u64,
    /// Caller-authorized teams with open slots: `(name, id, active_count, max_members)`.
    pub open_teams: &'a [(String, String, u64, u64)],
    pub extra_open_count: usize,
    pub can_set_directly: bool,
    pub can_request_increase: bool,
}

/// Formats actionable next steps when hiring or moving an agent into a full team (`TASK-210`).
pub fn format_team_full_hint(input: &TeamFullHintInput<'_>) -> String {
    let open_part = if input.open_teams.is_empty() {
        "No other authorized teams have open capacity.".to_owned()
    } else {
        let listed: Vec<String> = input
            .open_teams
            .iter()
            .map(|(name, id, used, max)| format!("{name} ({id}: {used}/{max})"))
            .collect();
        let more = if input.extra_open_count > 0 {
            format!(" (+{} more)", input.extra_open_count)
        } else {
            String::new()
        };
        format!("Teams with open capacity: {}{more}.", listed.join(", "))
    };
    let subteam_part = format!(
        "Create a sub-team: `team create --parent-team-id {} --name <name>`.",
        input.team_id
    );
    let limit_part = format_limit_step(input);
    format!("{open_part} {subteam_part} {limit_part}")
}

fn format_limit_step(input: &TeamFullHintInput<'_>) -> String {
    if input.current_max >= 100 {
        return "This team is already at the maximum cap of 100 members.".to_owned();
    }
    let next = input.current_max + 1;
    let direct = format!(
        "`team set-limit --team {} --max-members {next} --if-match {}`",
        input.team_id, input.team_revision
    );
    let request = format!(
        "`team request-limit --team {} --max-members {next} --reason \"<why>\"`",
        input.team_id
    );
    match (input.can_set_directly, input.can_request_increase) {
        (true, true) => {
            format!("Raise the limit directly with {direct}, or request manager approval with {request}.")
        }
        (true, false) => format!("Raise the limit directly with {direct}."),
        (false, true) => {
            format!("Request a manager-approved limit increase with {request} (or ask a manager/owner to run {direct}).")
        }
        (false, false) => {
            format!("Ask a manager/owner to raise the limit with {direct}, or ask the team lead to request one with {request}.")
        }
    }
}
