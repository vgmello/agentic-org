//! Durable manager-approved `team.max_members` increase requests (`TASK-210`), stored at
//! `state/teams/<chain>/limit-requests/<request-id>.yaml`.

use org_store::{DurablePath, Id, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    id, line, optional_id, optional_line, optional_time, team_folder, time,
    yaml::{self, Fields, RecordError},
    SCHEMA_VERSION,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamLimitRequestState {
    Pending,
    Approved,
    Denied,
}

impl TeamLimitRequestState {
    pub const ALL: [Self; 3] = [Self::Pending, Self::Approved, Self::Denied];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Approved | Self::Denied)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TeamLimitRequest {
    pub id: Id,
    pub revision: u64,
    pub state: TeamLimitRequestState,
    pub team_id: Id,
    pub requested_by_employee_id: Id,
    pub approver_employee_id: Id,
    pub current_max_members: u64,
    pub requested_max_members: u64,
    pub reason: String,
    pub decision_reason: Option<String>,
    pub decided_by_employee_id: Option<Id>,
    pub decided_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn team_limit_request_path(chain: &[Id], request: &Id) -> Result<DurablePath, StoreError> {
    if chain.is_empty() {
        return Err(StoreError::invalid("a team limit request names a team"));
    }
    DurablePath::parse(&format!(
        "{}/limit-requests/{request}.yaml",
        team_folder(chain)
    ))
}

pub fn encode_team_limit_request(r: &TeamLimitRequest) -> Vec<u8> {
    let decided_by = r.decided_by_employee_id.as_ref().map(Id::as_str);
    let fields: Fields = [
        ("schema_version", json!(SCHEMA_VERSION)),
        ("revision", json!(r.revision)),
        ("id", json!(r.id.as_str())),
        ("state", json!(r.state.as_str())),
        ("team_id", json!(r.team_id.as_str())),
        (
            "requested_by_employee_id",
            json!(r.requested_by_employee_id.as_str()),
        ),
        (
            "approver_employee_id",
            json!(r.approver_employee_id.as_str()),
        ),
        ("current_max_members", json!(r.current_max_members)),
        ("requested_max_members", json!(r.requested_max_members)),
        ("reason", json!(r.reason)),
        ("decision_reason", json!(r.decision_reason)),
        ("decided_by_employee_id", json!(decided_by)),
        ("decided_at", json!(r.decided_at)),
        ("created_at", json!(r.created_at)),
        ("updated_at", json!(r.updated_at)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    yaml::encode(&fields)
}

fn check_invariants(reader: &yaml::Reader, r: &TeamLimitRequest) -> Result<(), RecordError> {
    let valid_range =
        (1..=100).contains(&r.current_max_members) && (1..=100).contains(&r.requested_max_members);
    if !valid_range {
        return Err(reader.error("requested_max_members", "outside 1..=100"));
    }
    if r.requested_max_members <= r.current_max_members {
        return Err(reader.error("requested_max_members", "must exceed current_max_members"));
    }
    let decided = r.state.is_terminal();
    if decided != r.decided_by_employee_id.is_some() || decided != r.decided_at.is_some() {
        return Err(reader.error(
            "decided_by_employee_id",
            "decision metadata required iff state is approved or denied",
        ));
    }
    if !decided && r.decision_reason.is_some() {
        return Err(reader.error("decision_reason", "only allowed after decision"));
    }
    Ok(())
}

pub fn decode_team_limit_request(bytes: &[u8]) -> Result<TeamLimitRequest, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    if reader.number("schema_version")? != SCHEMA_VERSION {
        return Err(reader.error("schema_version", "not 1"));
    }
    let revision = reader.number("revision")?;
    if revision == 0 {
        return Err(reader.error("revision", "below 1"));
    }
    let state_text = reader.text("state")?;
    let state = TeamLimitRequestState::parse(&state_text)
        .ok_or_else(|| reader.error("state", "not a known team limit request state"))?;
    let record = TeamLimitRequest {
        id: id(&mut reader, "id")?,
        revision,
        state,
        team_id: id(&mut reader, "team_id")?,
        requested_by_employee_id: id(&mut reader, "requested_by_employee_id")?,
        approver_employee_id: id(&mut reader, "approver_employee_id")?,
        current_max_members: reader.number("current_max_members")?,
        requested_max_members: reader.number("requested_max_members")?,
        reason: line(&mut reader, "reason")?,
        decision_reason: optional_line(&mut reader, "decision_reason")?,
        decided_by_employee_id: optional_id(&mut reader, "decided_by_employee_id")?,
        decided_at: optional_time(&mut reader, "decided_at")?,
        created_at: time(&mut reader, "created_at")?,
        updated_at: time(&mut reader, "updated_at")?,
    };
    check_invariants(&reader, &record)?;
    reader.finish()?;
    Ok(record)
}
