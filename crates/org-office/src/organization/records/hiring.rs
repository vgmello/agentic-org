//! Durable records for C2 HR hiring (`hire_requests`), agent execution configurations
//! (`agent_execution_configs`), provider connections (`provider_connections`), and model
//! profiles (`model_profiles`) per data contracts document 04 and document 11.

use org_store::{DurablePath, Id, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    id, line, optional_id, optional_line, optional_time, path, team_folder, time,
    yaml::{self, Fields, RecordError},
    Meta, SCHEMA_VERSION,
};

/// Lifecycle state of a hire request (`doc-78` §Hire requests, `ADR-0029` decision 6).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HireState {
    Drafting,
    Hiring,
    Hired,
    Withdrawn,
    Failed,
}

impl HireState {
    pub const ALL: [Self; 5] = [
        Self::Drafting,
        Self::Hiring,
        Self::Hired,
        Self::Withdrawn,
        Self::Failed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Drafting => "drafting",
            Self::Hiring => "hiring",
            Self::Hired => "hired",
            Self::Withdrawn => "withdrawn",
            Self::Failed => "failed",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }
}

/// Permission preset on an agent's `execution.md` (`ADR-0032`, `doc-88` §3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPreset {
    #[default]
    SafeAsk,
    ReadOnly,
    WorkspaceEdits,
    AutonomousSandboxed,
}

impl PermissionPreset {
    pub const ALL: [Self; 4] = [
        Self::SafeAsk,
        Self::ReadOnly,
        Self::WorkspaceEdits,
        Self::AutonomousSandboxed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SafeAsk => "safe_ask",
            Self::ReadOnly => "read_only",
            Self::WorkspaceEdits => "workspace_edits",
            Self::AutonomousSandboxed => "autonomous_sandboxed",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }
}

/// Reasoning effort on an agent's `execution.md` (`TASK-181`): how hard the runtime thinks on
/// each turn. Absent keeps the runtime's default. These are the office's names (the same list
/// as [`org_proto::hiring::AGENT_EFFORT_LEVELS`]); each runtime adapter maps them onto its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl ReasoningEffort {
    pub const ALL: [Self; 5] = [Self::Low, Self::Medium, Self::High, Self::Xhigh, Self::Max];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// The level named exactly `text`; anything else (`""`, `none`, `High`) is no level.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == text)
    }
}

/// Authentication mode for a provider connection (`doc-88` §4, `schema.sql:1536`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthMode {
    SubscriptionCli,
    ApiKey,
    CloudIam,
}

impl ProviderAuthMode {
    pub const ALL: [Self; 3] = [Self::SubscriptionCli, Self::ApiKey, Self::CloudIam];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SubscriptionCli => "subscription_cli",
            Self::ApiKey => "api_key",
            Self::CloudIam => "cloud_iam",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == text)
    }
}

/// Structured proposal inside a `HireRequest` (`doc-78` §Hire requests).
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HireProposal {
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
    pub permission_preset: Option<PermissionPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_config: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// Durable hire request at `state/teams/<chain>/hires/<request-id>.yaml` (`doc-78`, `schema.sql`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HireRequest {
    pub id: Id,
    pub revision: u64,
    pub state: HireState,
    pub requested_by_employee_id: Id,
    pub home_team_id: Id,
    pub role_id: Option<Id>,
    pub proposal: HireProposal,
    pub reason: String,
    pub confirmed_at: Option<String>,
    pub closed_reason: Option<String>,
    pub employee_id: Option<Id>,
    pub failed_step: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn hire_request_path(chain: &[Id], request: &Id) -> Result<DurablePath, StoreError> {
    if chain.is_empty() {
        return Err(StoreError::invalid("a hire request names a team"));
    }
    DurablePath::parse(&format!("{}/hires/{request}.yaml", team_folder(chain)))
}

pub fn encode_hire_request(record: &HireRequest) -> Vec<u8> {
    let opt_id = |v: &Option<Id>| json!(v.as_ref().map(Id::as_str));
    let mut fields = Fields::new();
    fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
    fields.insert("revision".into(), json!(record.revision));
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("state".into(), json!(record.state.as_str()));
    fields.insert(
        "requested_by_employee_id".into(),
        json!(record.requested_by_employee_id.as_str()),
    );
    fields.insert("home_team_id".into(), json!(record.home_team_id.as_str()));
    fields.insert("role_id".into(), opt_id(&record.role_id));
    fields.insert(
        "proposal".into(),
        serde_json::to_value(&record.proposal).expect("HireProposal serializes"),
    );
    fields.insert("reason".into(), json!(record.reason));
    fields.insert("confirmed_at".into(), json!(record.confirmed_at));
    fields.insert("closed_reason".into(), json!(record.closed_reason));
    fields.insert("employee_id".into(), opt_id(&record.employee_id));
    fields.insert("failed_step".into(), json!(record.failed_step));
    fields.insert("created_at".into(), json!(record.created_at));
    fields.insert("updated_at".into(), json!(record.updated_at));
    yaml::encode(&fields)
}

pub fn decode_hire_request(bytes: &[u8]) -> Result<HireRequest, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    if reader.number("schema_version")? != SCHEMA_VERSION {
        return Err(reader.error("schema_version", "not 1"));
    }
    let revision = reader.number("revision")?;
    if revision == 0 {
        return Err(reader.error("revision", "below 1"));
    }
    let state_text = reader.text("state")?;
    let state = HireState::parse(&state_text)
        .ok_or_else(|| reader.error("state", "not a known hire state"))?;
    let proposal_val = reader.object("proposal")?;
    let proposal: HireProposal = serde_json::from_value(proposal_val)
        .map_err(|_| reader.error("proposal", "invalid hire proposal"))?;
    let record = HireRequest {
        id: id(&mut reader, "id")?,
        revision,
        state,
        requested_by_employee_id: id(&mut reader, "requested_by_employee_id")?,
        home_team_id: id(&mut reader, "home_team_id")?,
        role_id: optional_id(&mut reader, "role_id")?,
        proposal,
        reason: reader.text("reason")?,
        confirmed_at: optional_time(&mut reader, "confirmed_at")?,
        closed_reason: optional_line(&mut reader, "closed_reason")?,
        employee_id: optional_id(&mut reader, "employee_id")?,
        failed_step: optional_line(&mut reader, "failed_step")?,
        created_at: time(&mut reader, "created_at")?,
        updated_at: time(&mut reader, "updated_at")?,
    };
    validate_hire_state_invariants(&record, &reader)?;
    reader.finish()?;
    Ok(record)
}

fn validate_hire_state_invariants(
    record: &HireRequest,
    reader: &yaml::Reader,
) -> Result<(), RecordError> {
    let needs_confirmed = matches!(
        record.state,
        HireState::Hiring | HireState::Hired | HireState::Failed
    );
    if needs_confirmed != record.confirmed_at.is_some() {
        return Err(reader.error(
            "confirmed_at",
            "required iff state is hiring, hired, or failed",
        ));
    }
    if (record.state == HireState::Hired) != record.employee_id.is_some() {
        return Err(reader.error("employee_id", "required iff state is hired"));
    }
    if !matches!(record.state, HireState::Hiring | HireState::Failed)
        && record.failed_step.is_some()
    {
        return Err(reader.error("failed_step", "allowed only in hiring or failed"));
    }
    if matches!(record.state, HireState::Withdrawn | HireState::Failed)
        && record.closed_reason.as_deref().is_none_or(|s| s.is_empty())
    {
        return Err(reader.error("closed_reason", "required when withdrawn or failed"));
    }
    Ok(())
}

/// Durable agent execution configuration at `state/teams/<chain>/agents/<id>/execution.md`
/// (`doc-88` §3–§4, `schema.sql:1623`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AgentExecutionConfig {
    pub employee_id: Id,
    pub model_profile_id: Id,
    pub model_id: String,
    pub tools_config: Vec<String>,
    pub permission_preset: PermissionPreset,
    /// `None` keeps the runtime's default; the file then has no `effort` key at all, so a file
    /// written before `TASK-181` reads the same and encodes back to the same bytes.
    pub effort: Option<ReasoningEffort>,
    pub enabled: bool,
    pub meta: Meta,
    pub system_prompt_markdown: String,
}

pub fn agent_execution_path(chain: &[Id], employee: &Id) -> Result<DurablePath, StoreError> {
    if chain.is_empty() {
        return Err(StoreError::invalid("an agent has a home team"));
    }
    DurablePath::parse(&format!(
        "{}/agents/{employee}/execution.md",
        team_folder(chain)
    ))
}

pub fn encode_agent_execution(record: &AgentExecutionConfig) -> Vec<u8> {
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("employee_id".into(), json!(record.employee_id.as_str()));
    fields.insert(
        "model_profile_id".into(),
        json!(record.model_profile_id.as_str()),
    );
    fields.insert("model_id".into(), json!(record.model_id));
    fields.insert("tools_config".into(), json!(record.tools_config));
    fields.insert(
        "permission_preset".into(),
        json!(record.permission_preset.as_str()),
    );
    if let Some(effort) = record.effort {
        fields.insert("effort".into(), json!(effort.as_str()));
    }
    fields.insert("enabled".into(), json!(record.enabled));
    yaml::encode_document(&fields, &record.system_prompt_markdown)
}

pub fn decode_agent_execution(bytes: &[u8]) -> Result<AgentExecutionConfig, RecordError> {
    let (mut reader, body) = yaml::decode_document(bytes)?;
    let preset_text = reader.text("permission_preset")?;
    let permission_preset = PermissionPreset::parse(&preset_text)
        .ok_or_else(|| reader.error("permission_preset", "not a known permission preset"))?;
    // Optional: a file may omit it (or write `null`) for the runtime's default.
    let effort_text = match reader.has("effort") {
        true => reader.optional_text("effort")?,
        false => None,
    };
    let effort = effort_text
        .map(|text| {
            ReasoningEffort::parse(&text)
                .ok_or_else(|| reader.error("effort", "not a known reasoning effort"))
        })
        .transpose()?;
    let record = AgentExecutionConfig {
        meta: Meta::read(&mut reader)?,
        employee_id: id(&mut reader, "employee_id")?,
        model_profile_id: id(&mut reader, "model_profile_id")?,
        model_id: line(&mut reader, "model_id")?,
        tools_config: reader.strings("tools_config")?,
        permission_preset,
        effort,
        enabled: reader.flag("enabled")?,
        system_prompt_markdown: body,
    };
    if record.system_prompt_markdown.is_empty() {
        return Err(reader.error("body", "system_prompt_markdown must not be empty"));
    }
    reader.finish()?;
    Ok(record)
}

/// Provider connection at `state/provider-connections/<id>.yaml` (`doc-88` §4, `schema.sql:1528`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProviderConnection {
    pub id: Id,
    pub key: String,
    pub name: String,
    pub provider_type: String,
    pub auth_mode: ProviderAuthMode,
    pub subscription_identity: Option<String>,
    pub runtime_driver: String,
    pub endpoint_url: Option<String>,
    pub enabled: bool,
    pub archived: bool,
    pub max_concurrent_turns: u64,
    pub meta: Meta,
}

pub fn provider_connection_path(connection: &Id) -> DurablePath {
    path(&format!("state/provider-connections/{connection}.yaml"))
}

pub fn encode_provider_connection(record: &ProviderConnection) -> Vec<u8> {
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("key".into(), json!(record.key));
    fields.insert("name".into(), json!(record.name));
    fields.insert("provider_type".into(), json!(record.provider_type));
    fields.insert("auth_mode".into(), json!(record.auth_mode.as_str()));
    fields.insert(
        "subscription_identity".into(),
        json!(record.subscription_identity),
    );
    fields.insert("runtime_driver".into(), json!(record.runtime_driver));
    fields.insert("endpoint_url".into(), json!(record.endpoint_url));
    fields.insert("enabled".into(), json!(record.enabled));
    fields.insert("archived".into(), json!(record.archived));
    fields.insert(
        "max_concurrent_turns".into(),
        json!(record.max_concurrent_turns),
    );
    yaml::encode(&fields)
}

pub fn decode_provider_connection(bytes: &[u8]) -> Result<ProviderConnection, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let auth_text = reader.text("auth_mode")?;
    let auth_mode = ProviderAuthMode::parse(&auth_text)
        .ok_or_else(|| reader.error("auth_mode", "not a known auth mode"))?;
    let record = ProviderConnection {
        meta: Meta::read(&mut reader)?,
        id: id(&mut reader, "id")?,
        key: line(&mut reader, "key")?,
        name: line(&mut reader, "name")?,
        provider_type: line(&mut reader, "provider_type")?,
        auth_mode,
        subscription_identity: optional_line(&mut reader, "subscription_identity")?,
        runtime_driver: line(&mut reader, "runtime_driver")?,
        endpoint_url: optional_line(&mut reader, "endpoint_url")?,
        enabled: reader.flag("enabled")?,
        archived: reader.flag("archived")?,
        max_concurrent_turns: reader.number("max_concurrent_turns")?,
    };
    if record.max_concurrent_turns == 0 {
        return Err(reader.error("max_concurrent_turns", "below 1"));
    }
    if record.auth_mode == ProviderAuthMode::SubscriptionCli
        && record.subscription_identity.is_none()
    {
        return Err(reader.error(
            "subscription_identity",
            "required when auth_mode is subscription_cli",
        ));
    }
    reader.finish()?;
    Ok(record)
}

/// Model profile at `state/model-profiles/<id>.yaml` (`doc-88` §4, `schema.sql:1584`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModelProfile {
    pub id: Id,
    pub key: String,
    pub name: String,
    pub description: String,
    pub is_auto_generated: bool,
    pub generated_for_connection_id: Option<Id>,
    pub archived: bool,
    pub connection_ids: Vec<Id>,
    pub meta: Meta,
}

pub fn model_profile_path(profile: &Id) -> DurablePath {
    path(&format!("state/model-profiles/{profile}.yaml"))
}

pub fn encode_model_profile(record: &ModelProfile) -> Vec<u8> {
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("key".into(), json!(record.key));
    fields.insert("name".into(), json!(record.name));
    fields.insert("description".into(), json!(record.description));
    fields.insert("is_auto_generated".into(), json!(record.is_auto_generated));
    fields.insert(
        "generated_for_connection_id".into(),
        json!(record.generated_for_connection_id.as_ref().map(Id::as_str)),
    );
    fields.insert("archived".into(), json!(record.archived));
    let conn_ids: Vec<&str> = record.connection_ids.iter().map(Id::as_str).collect();
    fields.insert("connection_ids".into(), json!(conn_ids));
    yaml::encode(&fields)
}

pub fn decode_model_profile(bytes: &[u8]) -> Result<ModelProfile, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let meta = Meta::read(&mut reader)?;
    let pid = id(&mut reader, "id")?;
    let key = line(&mut reader, "key")?;
    let name = line(&mut reader, "name")?;
    let description = reader.text("description")?;
    let is_auto_generated = reader.flag("is_auto_generated")?;
    let generated_for_connection_id = optional_id(&mut reader, "generated_for_connection_id")?;
    let archived = reader.flag("archived")?;
    let raw_conns = reader.strings("connection_ids")?;
    if raw_conns.is_empty() || raw_conns.len() > 64 {
        return Err(reader.error("connection_ids", "must hold 1..=64 connections"));
    }
    let mut connection_ids = Vec::with_capacity(raw_conns.len());
    for c in raw_conns {
        let cid = super::stable_id(&c)
            .ok_or_else(|| reader.error("connection_ids", "not a UUIDv7 in lowercase text"))?;
        if connection_ids.contains(&cid) {
            return Err(reader.error("connection_ids", "duplicate connection_id"));
        }
        connection_ids.push(cid);
    }
    if is_auto_generated != generated_for_connection_id.is_some() {
        return Err(reader.error(
            "generated_for_connection_id",
            "must be set iff is_auto_generated is true",
        ));
    }
    reader.finish()?;
    Ok(ModelProfile {
        id: pid,
        key,
        name,
        description,
        is_auto_generated,
        generated_for_connection_id,
        archived,
        connection_ids,
        meta,
    })
}

/// Organization-wide channel watching model profile setting at
/// `state/execution/watching-model.yaml` (`ADR-0024` d13, `WATCH-5`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WatchingModelSetting {
    pub model_profile_id: Option<Id>,
    pub meta: Meta,
}

pub fn watching_model_path() -> DurablePath {
    path("state/execution/watching-model.yaml")
}

pub fn encode_watching_model(record: &WatchingModelSetting) -> Vec<u8> {
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert(
        "model_profile_id".into(),
        json!(record.model_profile_id.as_ref().map(Id::as_str)),
    );
    yaml::encode(&fields)
}

pub fn decode_watching_model(bytes: &[u8]) -> Result<WatchingModelSetting, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let meta = Meta::read(&mut reader)?;
    let model_profile_id = optional_id(&mut reader, "model_profile_id")?;
    reader.finish()?;
    Ok(WatchingModelSetting {
        model_profile_id,
        meta,
    })
}
