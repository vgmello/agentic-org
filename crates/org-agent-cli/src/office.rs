//! Bound office transport implementing `org_command::Office` over `org-client` for `agent-os`.
//!
//! Supports both direct socket injection (`AGENT_OS_SOCKET`) and selected UUIDv7 office
//! addressing (`AGENT_OS_OFFICE` + `AGENT_OS_RUNTIME_DIR` / `ORG_OFFICE_HOME`, `doc-5` A5).

use std::env;
use std::fmt;
use std::path::PathBuf;

use org_client::{Client, ClientError};
use org_command::{Answer, Office, Refusal, Request};
use org_proto::location::{selected_office_socket_in, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::msg::{MsgBind, MsgBindParams};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::dispatch::{self, hint_for, OfficeTransport};

/// Environment variable naming the office socket path (`doc-28` §2.8).
pub const SOCKET_ENV: &str = "AGENT_OS_SOCKET";
/// Environment variable naming a selected UUIDv7 office ID (`doc-5` A5).
pub const OFFICE_ENV: &str = "AGENT_OS_OFFICE";
/// Environment variable naming the private runtime socket directory for `AGENT_OS_OFFICE`.
pub const RUNTIME_DIR_ENV: &str = "AGENT_OS_RUNTIME_DIR";
/// Environment variable carrying the run's session binding token (`doc-28` §2.8).
pub const SESSION_TOKEN_ENV: &str = "AGENT_OS_SESSION_TOKEN";
/// Environment variable naming the bound agent's employee ID (`doc-28` §2.8).
pub const AGENT_ID_ENV: &str = "AGENT_OS_AGENT_ID";

/// Run binding configuration supplied to `agent-os` by its office.
#[derive(Clone, Default)]
pub struct Binding {
    pub socket: Option<PathBuf>,
    pub office_id: Option<String>,
    pub runtime_dir: Option<PathBuf>,
    pub session_token: Option<String>,
    pub agent_id: Option<String>,
}

impl fmt::Debug for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Binding")
            .field("socket", &self.socket)
            .field("office_id", &self.office_id)
            .field("runtime_dir", &self.runtime_dir)
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
        let runtime_dir = non_empty(RUNTIME_DIR_ENV)
            .or_else(|| non_empty(OFFICE_RUNTIME_DIR_ENV))
            .map(PathBuf::from)
            .or_else(|| non_empty(OFFICE_HOME_ENV).map(|h| PathBuf::from(h).join("run")));
        Self {
            socket: non_empty(SOCKET_ENV).map(PathBuf::from),
            office_id: non_empty(OFFICE_ENV),
            runtime_dir,
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

    fn connect_selected(&self, office_id: &str) -> Result<Client, Refusal> {
        if !org_proto::is_uuid_v7(office_id) {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                format!("Unknown or malformed office `{office_id}` (expected UUIDv7 office ID)."),
                "Set AGENT_OS_OFFICE to a selected UUIDv7 office ID or pass AGENT_OS_SOCKET.",
            ));
        }
        let Some(runtime_dir) = &self.binding.runtime_dir else {
            return Err(Refusal::new(
                ErrorCode::NotFound,
                format!("No runtime socket directory configured for office `{office_id}`."),
                "Set AGENT_OS_RUNTIME_DIR or ORG_OFFICE_HOME to the office runtime directory.",
            ));
        };
        if !runtime_dir.is_absolute() || selected_office_socket_in(runtime_dir, office_id).is_none()
        {
            return Err(Refusal::new(
                ErrorCode::NotFound,
                format!("Office `{office_id}` cannot be addressed under that runtime directory."),
                "Pass an absolute AGENT_OS_RUNTIME_DIR for a valid UUIDv7 office.",
            ));
        }
        Client::connect_selected_office(office_id, runtime_dir)
            .map_err(|e| map_connect_error(e, Some(office_id)))
    }

    fn bind_agent(&self, client: &Client, agent_id: &str) -> Result<(), Refusal> {
        let params = match &self.binding.session_token {
            Some(token) => MsgBindParams::member_with_token(agent_id.to_owned(), token.clone()),
            None => MsgBindParams::member(agent_id.to_owned()),
        };
        let Err(error) = client.call::<MsgBind>(&params) else {
            return Ok(());
        };
        let office_label = self
            .binding
            .office_id
            .as_deref()
            .or_else(|| self.binding.socket.as_deref().and_then(|p| p.to_str()))
            .unwrap_or("office");
        let mut refusal = map_client_error(error);
        refusal.message = format!(
            "Office `{office_label}` refused agent `{agent_id}`: {}",
            refusal.message
        );
        Err(refusal)
    }

    fn connect_client(&self) -> Result<Client, Refusal> {
        if self.binding.session_token.is_none() {
            return Err(Refusal::new(
                ErrorCode::NotAuthorized,
                "agent-os requires a run binding from its office.",
                "Run agent-os inside an office-started agent session with AGENT_OS_SESSION_TOKEN and AGENT_OS_SOCKET (or AGENT_OS_OFFICE) set.",
            ));
        }
        let client = if let Some(socket) = &self.binding.socket {
            Client::connect(socket)
                .map_err(|e| map_connect_error(e, self.binding.office_id.as_deref()))?
        } else if let Some(office_id) = &self.binding.office_id {
            self.connect_selected(office_id)?
        } else {
            return Err(Refusal::new(
                ErrorCode::NotAuthorized,
                "agent-os requires a run binding from its office.",
                "Set AGENT_OS_SOCKET or AGENT_OS_OFFICE + AGENT_OS_RUNTIME_DIR alongside AGENT_OS_SESSION_TOKEN.",
            ));
        };
        if let Some(agent_id) = &self.binding.agent_id {
            self.bind_agent(&client, agent_id)?;
        }
        Ok(client)
    }

    /// Eagerly connects and binds to the office socket once (used by `session attach`).
    pub fn ensure_connected(&mut self) -> Result<(), Refusal> {
        self.client().map(|_| ())
    }

    fn client(&mut self) -> Result<&mut Client, Refusal> {
        if self.client.is_none() {
            let client = self.connect_client()?;
            self.client = Some(client);
        }
        Ok(self.client.as_mut().expect("client just connected"))
    }
}

impl OfficeTransport for BoundOffice {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        let client = self.client()?;
        client.call_raw(method, params).map_err(map_client_error)
    }
}

impl Office for BoundOffice {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        dispatch::execute(self, request)
    }
}

fn map_connect_error(error: ClientError, office_id: Option<&str>) -> Refusal {
    let target = office_id.unwrap_or("bound office");
    match error {
        ClientError::NotRunning(_) | ClientError::OfficeNotReachable => Refusal::new(
            ErrorCode::NotFound,
            format!("Office `{target}` is not running or its socket was not found."),
            "Check that the office is active and AGENT_OS_OFFICE / AGENT_OS_SOCKET is correct.",
        ),
        other => map_client_error(other),
    }
}

fn map_client_error(error: ClientError) -> Refusal {
    match error {
        ClientError::Api(api) => {
            let hint = if api.code == ErrorCode::NotAuthorized
                && api.message.contains("Ask your team lead.")
            {
                "Ask your team lead."
            } else {
                hint_for(api.code)
            };
            Refusal::new(api.code, api.message, hint)
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_debug_redacts_session_token() {
        let binding = Binding {
            socket: Some(PathBuf::from("/tmp/office.sock")),
            office_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a0".into()),
            runtime_dir: Some(PathBuf::from("/tmp")),
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
    fn unknown_or_malformed_selected_office_refuses_immediately() {
        let office = BoundOffice::new(Binding {
            socket: None,
            office_id: Some("not-a-uuidv7".into()),
            runtime_dir: Some(PathBuf::from("/tmp")),
            session_token: Some("token".into()),
            agent_id: None,
        });
        let err = office.connect_client().err().expect("refused");
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        assert!(err.message.contains("not-a-uuidv7"), "{}", err.message);

        let dir = tempfile::tempdir().unwrap();
        let missing_id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff";
        let missing = BoundOffice::new(Binding {
            socket: None,
            office_id: Some(missing_id.into()),
            runtime_dir: Some(dir.path().to_path_buf()),
            session_token: Some("token".into()),
            agent_id: None,
        });
        let err = missing.connect_client().err().expect("refused");
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(err.message.contains(missing_id), "{}", err.message);
    }
}
