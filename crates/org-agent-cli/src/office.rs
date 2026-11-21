//! Bound office transport implementing `org_command::Office` over `org-client`.

use std::env;
use std::fmt;
use std::path::PathBuf;

use org_client::{Client, ClientError};
use org_command::{Answer, Office, Refusal, Request};
use org_proto::directory::{DirectoryCard, Who, WhoParams};
use org_proto::msg::{MsgBind, MsgBindParams};
use org_proto::org::{MemberKind, OrgSnapshot, OrgSnapshotCall, Team};
use org_proto::{Empty, ErrorCode};
use serde_json::{json, Value};

/// Environment variable naming the office socket path (`doc-28` §2.8).
pub const SOCKET_ENV: &str = "AGENT_OS_SOCKET";
/// Environment variable carrying the run's session binding token (`doc-28` §2.8).
pub const SESSION_TOKEN_ENV: &str = "AGENT_OS_SESSION_TOKEN";
/// Environment variable naming the bound agent's employee ID (`doc-28` §2.8).
pub const AGENT_ID_ENV: &str = "AGENT_OS_AGENT_ID";

/// Run binding configuration supplied to `agent-os` by its office.
#[derive(Clone, Default)]
pub struct Binding {
    pub socket: Option<PathBuf>,
    pub session_token: Option<String>,
    pub agent_id: Option<String>,
}

impl fmt::Debug for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Binding")
            .field("socket", &self.socket)
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "<redacted>"),
            )
            .field("agent_id", &self.agent_id)
            .finish()
    }
}

impl Binding {
    /// Reads the process binding from `AGENT_OS_*` environment variables.
    pub fn from_env() -> Self {
        let non_empty = |name: &str| {
            env::var(name)
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        Self {
            socket: non_empty(SOCKET_ENV).map(PathBuf::from),
            session_token: non_empty(SESSION_TOKEN_ENV),
            agent_id: non_empty(AGENT_ID_ENV),
        }
    }
}

/// Connects lazily to the bound office socket and executes `org-command` requests.
pub struct BoundOffice {
    binding: Binding,
    client: Option<Client>,
}

impl BoundOffice {
    pub fn new(binding: Binding) -> Self {
        Self {
            binding,
            client: None,
        }
    }

    fn client(&mut self) -> Result<&mut Client, Refusal> {
        if self.client.is_none() {
            // Fail closed unless both socket path and session token are injected by the office.
            // Note: `MsgBindParams` does not yet carry a wire token field; wire session-token
            // verification lands with the agent session-binding slice (TASK-35 / B6).
            let (Some(socket), Some(_token)) = (&self.binding.socket, &self.binding.session_token)
            else {
                return Err(Refusal::new(
                    ErrorCode::NotAuthorized,
                    "agent-os requires a run binding from its office.",
                    "Run agent-os inside an office-started agent session with AGENT_OS_SOCKET and AGENT_OS_SESSION_TOKEN set.",
                ));
            };
            let client = Client::connect(socket).map_err(map_client_error)?;
            if let Some(agent_id) = &self.binding.agent_id {
                client
                    .call::<MsgBind>(&MsgBindParams::member(agent_id.clone()))
                    .map_err(map_client_error)?;
            }
            self.client = Some(client);
        }
        Ok(self.client.as_mut().expect("client just connected"))
    }
}

impl Office for BoundOffice {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        let client = self.client()?;
        match (request.command.resource, request.command.verb) {
            ("org", "inspect") => {
                let snapshot = client
                    .call::<OrgSnapshotCall>(&Empty {})
                    .map_err(map_client_error)?;
                Ok(Answer::completed(snapshot_value(&snapshot)))
            }
            ("team", "list") => {
                let offset = parse_cursor(request.cursor.as_deref())?;
                let snapshot = client
                    .call::<OrgSnapshotCall>(&Empty {})
                    .map_err(map_client_error)?;
                let mut teams: Vec<&Team> = snapshot.teams.iter().collect();
                teams.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
                let total = teams.len();
                let limit = request.limit.unwrap_or(org_command::DEFAULT_LIMIT) as usize;
                let items: Vec<Value> = teams
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .map(team_value)
                    .collect();
                let next = offset.saturating_add(items.len());
                let mut answer = Answer::completed(Value::Array(items));
                if next < total {
                    answer.next_cursor = Some(next.to_string());
                }
                Ok(answer)
            }
            ("team", "inspect") => {
                let target = request
                    .input
                    .get("team")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Refusal::new(
                            ErrorCode::InvalidRequest,
                            "missing required --team flag.",
                            "Pass --team <team-id-or-name>.",
                        )
                    })?;
                let snapshot = client
                    .call::<OrgSnapshotCall>(&Empty {})
                    .map_err(map_client_error)?;
                let team = find_team(&snapshot, target)?;
                let who = client
                    .call::<Who>(&WhoParams {
                        team: Some(team.id.clone()),
                        ..WhoParams::default()
                    })
                    .map_err(map_client_error)?;
                let members: Vec<Value> = who.members.iter().map(card_value).collect();
                let mut obj = match team_value(team) {
                    Value::Object(map) => map,
                    _ => unreachable!(),
                };
                obj.insert("members".into(), Value::Array(members));
                Ok(Answer::completed(Value::Object(obj)))
            }
            (resource @ ("person" | "agent"), "inspect") => {
                let member = request
                    .input
                    .get("member")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Refusal::new(
                            ErrorCode::InvalidRequest,
                            "missing required --member flag.",
                            "Pass --member <id-or-handle>.",
                        )
                    })?;
                let who = client
                    .call::<Who>(&WhoParams {
                        member: Some(member.to_owned()),
                        ..WhoParams::default()
                    })
                    .map_err(map_client_error)?;
                let expected = expected_kind(resource);
                let Some(card) = who.members.iter().find(|c| c.kind == expected) else {
                    return Err(Refusal::new(
                        ErrorCode::NotFound,
                        format!("No active {resource} matches that reference."),
                        "Run `agent-os org inspect` to see active members.",
                    ));
                };
                Ok(Answer::completed(card_value(card)))
            }
            (resource, verb) => Err(Refusal::new(
                ErrorCode::Unsupported,
                format!("`{resource} {verb}` is not wired to the office yet."),
                "Run `agent-os --help` to see available commands.",
            )),
        }
    }
}

fn find_team<'a>(snapshot: &'a OrgSnapshot, target: &str) -> Result<&'a Team, Refusal> {
    if let Some(team) = snapshot.teams.iter().find(|t| t.id == target) {
        return Ok(team);
    }
    let needle = target.trim().to_lowercase();
    let matches: Vec<&Team> = snapshot
        .teams
        .iter()
        .filter(|t| t.name.trim().to_lowercase() == needle)
        .collect();
    match matches.as_slice() {
        [only] => Ok(only),
        [] => Err(Refusal::new(
            ErrorCode::NotFound,
            "No active team matches that reference.",
            "Run `agent-os team list` to see active teams.",
        )),
        _ => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "More than one team has that name.",
            "Pass the team's UUIDv7 ID with --team.",
        )),
    }
}

fn team_value(team: &Team) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("id".into(), json!(team.id));
    map.insert("name".into(), json!(team.name));
    if let Some(parent) = &team.owner_team_id {
        map.insert("owner_team_id".into(), json!(parent));
    }
    if let Some(lead) = &team.lead_member_id {
        map.insert("lead_member_id".into(), json!(lead));
    }
    map.insert("version".into(), json!(team.version));
    Value::Object(map)
}

fn snapshot_value(snapshot: &OrgSnapshot) -> Value {
    let teams: Vec<Value> = snapshot.teams.iter().map(team_value).collect();
    let members: Vec<Value> = snapshot
        .members
        .iter()
        .map(|m| {
            let mut map = serde_json::Map::new();
            map.insert("id".into(), json!(m.id));
            map.insert("kind".into(), json!(m.kind));
            map.insert("name".into(), json!(m.name));
            if let Some(handle) = &m.handle {
                map.insert("handle".into(), json!(handle));
            }
            if let Some(home) = &m.home_team_id {
                map.insert("home_team_id".into(), json!(home));
            }
            if let Some(title) = &m.title_in_team {
                map.insert("title_in_team".into(), json!(title));
            }
            map.insert("owner".into(), json!(m.owner));
            map.insert("version".into(), json!(m.version));
            Value::Object(map)
        })
        .collect();
    json!({
        "position": {
            "epoch": snapshot.position.epoch,
            "seq": snapshot.position.seq,
        },
        "teams": teams,
        "members": members,
    })
}

fn card_value(card: &DirectoryCard) -> Value {
    let teams: Vec<Value> = card
        .teams
        .iter()
        .map(|t| {
            let mut map = serde_json::Map::new();
            map.insert("id".into(), json!(t.id));
            map.insert("name".into(), json!(t.name));
            if let Some(title) = &t.title_in_team {
                map.insert("title_in_team".into(), json!(title));
            }
            Value::Object(map)
        })
        .collect();
    let mut map = serde_json::Map::new();
    map.insert("id".into(), json!(card.id));
    map.insert("name".into(), json!(card.name));
    if let Some(handle) = &card.handle {
        map.insert("handle".into(), json!(handle));
    }
    map.insert("kind".into(), json!(card.kind));
    map.insert("owner".into(), json!(card.owner));
    if let Some(status) = card.agent_status {
        map.insert("agent_status".into(), json!(status));
    }
    if let Some(title) = &card.role_title {
        map.insert("role_title".into(), json!(title));
    }
    map.insert("presence".into(), json!(card.presence));
    map.insert("teams".into(), Value::Array(teams));
    Value::Object(map)
}

fn map_client_error(error: ClientError) -> Refusal {
    match error {
        ClientError::Api(api) => Refusal::new(api.code, api.message, hint_for(api.code)),
        ClientError::RequestTooLarge => Refusal::new(
            ErrorCode::TooLarge,
            "The request exceeded the office socket frame limit.",
            "Reduce the request size and retry.",
        ),
        ClientError::Timeout => Refusal::new(
            ErrorCode::Timeout,
            "The office did not answer before the timeout.",
            "Retry the command; if it persists, check the office status.",
        ),
        ClientError::Disconnected | ClientError::Io(_) | ClientError::Protocol(_) => Refusal::new(
            ErrorCode::Unavailable,
            "The bound office could not be reached.",
            "Check that the office process is running and AGENT_OS_SOCKET is valid.",
        ),
        other => Refusal::new(
            ErrorCode::Unavailable,
            other.to_string(),
            "Check the office connection and retry.",
        ),
    }
}

fn expected_kind(resource: &str) -> MemberKind {
    match resource {
        "person" => MemberKind::Person,
        "agent" => MemberKind::Agent,
        _ => unreachable!(),
    }
}

fn parse_cursor(cursor: Option<&str>) -> Result<usize, Refusal> {
    let Some(raw) = cursor else {
        return Ok(0);
    };
    raw.parse::<usize>().map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "Invalid pagination cursor.",
            "Pass the next_cursor returned by the previous list call.",
        )
    })
}

fn hint_for(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::NotAuthorized | ErrorCode::NotBound => {
            "Check your session binding and permissions for this action."
        }
        ErrorCode::NotFound => "Check the ID or name and retry.",
        ErrorCode::Conflict => "Read the current record and retry with the latest --if-match.",
        ErrorCode::Unsupported => "This office build does not serve that command yet.",
        ErrorCode::Unavailable | ErrorCode::NotReady => "Wait briefly and retry the command.",
        _ => "Check the flags with --help and retry.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_debug_redacts_session_token() {
        let binding = Binding {
            socket: Some(PathBuf::from("/tmp/office.sock")),
            session_token: Some("super-secret-session-token".into()),
            agent_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into()),
        };
        let rendered = format!("{binding:?}");
        assert!(
            !rendered.contains("super-secret-session-token"),
            "{rendered}"
        );
        assert!(rendered.contains("<redacted>"), "{rendered}");
    }

    #[test]
    fn parse_cursor_accepts_non_negative_offsets_and_rejects_garbage() {
        assert_eq!(parse_cursor(None).unwrap(), 0);
        assert_eq!(parse_cursor(Some("0")).unwrap(), 0);
        assert_eq!(parse_cursor(Some("25")).unwrap(), 25);
        assert_eq!(
            parse_cursor(Some("not-a-number")).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn expected_kind_matches_resource_name() {
        assert_eq!(expected_kind("person"), MemberKind::Person);
        assert_eq!(expected_kind("agent"), MemberKind::Agent);
    }
}
