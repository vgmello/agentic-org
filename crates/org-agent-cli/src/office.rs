//! Bound office transport implementing `org_command::Office` over `org-client` for `agent-os`.
//!
//! Supports both direct socket injection (`AGENT_OS_SOCKET`) and selected UUIDv7 office
//! addressing (`AGENT_OS_OFFICE` + `AGENT_OS_RUNTIME_DIR` / `ORG_OFFICE_HOME`, `doc-5` A5).

use std::env;
use std::fmt;
use std::path::PathBuf;

use org_client::{Client, ClientError};
use org_command::{Answer, Office, Refusal, RefusalDetails, Request};
use org_proto::location::{selected_office_socket_in, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::msg::{MsgBind, MsgBindParams};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::dispatch;
use crate::transport::{hint_for, OfficeTransport};

/// Environment variable naming the office socket path (`doc-28` §2.8).
pub const SOCKET_ENV: &str = "AGENT_OS_SOCKET";
/// Environment variable naming a selected UUIDv7 office ID (`doc-5` A5).
pub const OFFICE_ENV: &str = "AGENT_OS_OFFICE";
/// Environment variable naming the private runtime socket directory for `AGENT_OS_OFFICE`.
pub const RUNTIME_DIR_ENV: &str = "AGENT_OS_RUNTIME_DIR";
/// Environment variable carrying the run's session binding token (`doc-28` §2.8).
pub const SESSION_TOKEN_ENV: &str = "AGENT_OS_SESSION_TOKEN";
/// Environment variable naming an owner-only (`0600`) file holding the run's session token
/// (fallback when a runtime scrubs `*_TOKEN` variables from tool subprocesses, e.g. Claude
/// Code `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1`).
pub const BINDING_FILE_ENV: &str = "AGENT_OS_BINDING_FILE";
/// Environment variable naming the bound agent's employee ID (`doc-28` §2.8).
pub const AGENT_ID_ENV: &str = "AGENT_OS_AGENT_ID";

const MAX_BINDING_FILE_BYTES: u64 = 4096;

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
        let session_token = non_empty(SESSION_TOKEN_ENV)
            .or_else(|| non_empty(BINDING_FILE_ENV).and_then(|p| read_binding_file_token(&p)));
        Self {
            socket: non_empty(SOCKET_ENV).map(PathBuf::from),
            office_id: non_empty(OFFICE_ENV),
            runtime_dir,
            session_token,
            agent_id: non_empty(AGENT_ID_ENV),
        }
    }
}

fn read_binding_file_token(path: &str) -> Option<String> {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(
            org_platform::fs::O_NOFOLLOW
                | org_platform::fs::O_NONBLOCK
                | org_platform::fs::O_CLOEXEC,
        )
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_BINDING_FILE_BYTES {
        return None;
    }
    let mut buf = String::new();
    let n = file
        .take(MAX_BINDING_FILE_BYTES + 1)
        .read_to_string(&mut buf)
        .ok()?;
    if (n as u64) > MAX_BINDING_FILE_BYTES {
        return None;
    }
    let trimmed = buf.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Whether the process environment asks for an office binding: it names a session token or a
/// binding file, even one that cannot be read. Such a caller is bound: a command that could run
/// without the office fails closed instead (`session boot`).
pub fn env_requests_binding() -> bool {
    [SESSION_TOKEN_ENV, BINDING_FILE_ENV]
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// Connects lazily to the bound office socket and executes `org-command` requests.
pub struct BoundOffice {
    binding: Binding,
    client: Option<Client>,
    /// Per-command `waiting_messages` hint returned by the most recent office RPC call.
    waiting_messages: Option<u32>,
    /// Whether the caller asked for a binding ([`OfficeTransport::is_bound`]).
    bound: bool,
    /// The employee the office bound the connection to (`msg.bind`), once connected.
    bound_member: Option<String>,
}

impl BoundOffice {
    /// An office for `binding`, bound when it carries a session token.
    pub fn new(binding: Binding) -> Self {
        let bound = binding.session_token.is_some();
        Self {
            binding,
            client: None,
            waiting_messages: None,
            bound,
            bound_member: None,
        }
    }

    /// Marks the office bound because the environment asked for a binding
    /// ([`env_requests_binding`]), even when its token could not be read: then every office
    /// command fails closed instead of running unbound.
    pub fn with_binding_requested(mut self, requested: bool) -> Self {
        self.bound |= requested;
        self
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

    fn bind_agent(&self, client: &Client, agent_id: &str) -> Result<Option<String>, Refusal> {
        let params = match &self.binding.session_token {
            Some(token) => MsgBindParams::member_with_token(agent_id.to_owned(), token.clone()),
            None => MsgBindParams::member(agent_id.to_owned()),
        };
        let error = match client.call::<MsgBind>(&params) {
            Ok(bound) => return Ok(bound.member_id),
            Err(error) => error,
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

    fn connect_client(&self) -> Result<(Client, Option<String>), Refusal> {
        if self.binding.session_token.is_none() && self.bound {
            return Err(Refusal::new(
                ErrorCode::NotAuthorized,
                "AGENT_OS_SESSION_TOKEN or AGENT_OS_BINDING_FILE is set, but no session token could be read from it.",
                "Check that AGENT_OS_BINDING_FILE names this session's owner-only binding file (a regular file, not a symlink, at most 4 KiB), or start the session again; `agent-os boot --offline` works without the office.",
            ));
        }
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
        let member = match &self.binding.agent_id {
            Some(agent_id) => self.bind_agent(&client, agent_id)?,
            None => None,
        };
        Ok((client, member))
    }

    /// Eagerly connects and binds to the office socket once (used by `session attach`).
    pub fn ensure_connected(&mut self) -> Result<(), Refusal> {
        self.client().map(|_| ())
    }

    fn client(&mut self) -> Result<&mut Client, Refusal> {
        if self.client.is_none() {
            let (client, member) = self.connect_client()?;
            self.client = Some(client);
            self.bound_member = member;
        }
        Ok(self.client.as_mut().expect("client just connected"))
    }
}

impl OfficeTransport for BoundOffice {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.waiting_messages = None;
        let outcome = self
            .client()?
            .call_raw_with_meta(method, params)
            .map_err(map_client_error)?;
        self.waiting_messages = outcome.meta.waiting_messages;
        Ok(outcome.value)
    }

    fn call_method_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: std::time::Duration,
    ) -> Result<Value, Refusal> {
        self.waiting_messages = None;
        let outcome = self
            .client()?
            .call_raw_with_timeout_and_meta(method, params, timeout)
            .map_err(map_client_error)?;
        self.waiting_messages = outcome.meta.waiting_messages;
        Ok(outcome.value)
    }

    fn is_bound(&self) -> bool {
        self.bound
    }

    fn caller_member_id(&self) -> Option<String> {
        self.bound_member
            .clone()
            .or_else(|| self.binding.agent_id.clone())
    }
}

impl Office for BoundOffice {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        self.waiting_messages = None;
        let mut answer = dispatch::execute(self, request)?;
        answer.waiting_messages = self.waiting_messages.take();
        Ok(answer)
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

/// The refusal of an office error, with everything the office said in its structured fields:
/// `retry_after_ms` for `rate_limited`, and the `field` and `rule` an input failed. The one
/// mapping for `agent-os` and for `agentic-org`'s grant-bound channel alike.
pub fn api_refusal(api: org_proto::ApiError) -> Refusal {
    let hint = match (api.code, api.retry_after_ms, &api.field, &api.rule) {
        (ErrorCode::NotAuthorized, ..) if api.message.contains("Ask your team lead.") => {
            "Ask your team lead.".to_owned()
        }
        (ErrorCode::RateLimited, Some(after), ..) => format!(
            "Wait {after} ms (error.retry_after_ms), then retry the command with the same flags."
        ),
        (_, _, Some(field), Some(rule)) => format!(
            "Fix `{field}`: it does not meet the `{rule}` rule (error.field, error.rule). Then retry."
        ),
        (_, _, Some(field), None) => {
            format!("Fix `{field}` (error.field), then retry.")
        }
        (code, ..) => hint_for(code).to_owned(),
    };
    let details =
        (api.retry_after_ms.is_some() || api.field.is_some() || api.rule.is_some()).then(|| {
            Box::new(RefusalDetails {
                retry_after_ms: api.retry_after_ms,
                field: api.field,
                rule: api.rule,
            })
        });
    Refusal {
        details,
        ..Refusal::new(api.code, api.message, hint)
    }
}

fn map_client_error(error: ClientError) -> Refusal {
    match error {
        ClientError::Api(api) => api_refusal(api),
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

    #[test]
    fn binding_file_reads_trimmed_token() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session-binding.v1");
        std::fs::write(&file, "  tok-from-file-123\n").unwrap();
        assert_eq!(
            read_binding_file_token(file.to_str().unwrap()).as_deref(),
            Some("tok-from-file-123")
        );
        assert_eq!(read_binding_file_token("/nonexistent/binding"), None);

        // Reject symlinks (O_NOFOLLOW) and oversized files.
        let link = dir.path().join("link-binding.v1");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert_eq!(read_binding_file_token(link.to_str().unwrap()), None);

        let huge = dir.path().join("huge-binding.v1");
        std::fs::write(&huge, "x".repeat((MAX_BINDING_FILE_BYTES + 1) as usize)).unwrap();
        assert_eq!(read_binding_file_token(huge.to_str().unwrap()), None);
    }

    #[test]
    fn bound_office_propagates_per_call_waiting_messages_without_stale_leaks() {
        use std::io::BufReader;
        use std::os::unix::net::UnixListener;

        use org_proto::msg::MsgBindResult;
        use org_proto::{frame, Request as WireRequest, Response as WireResponse};
        use serde_json::json;

        let dir = tempfile::Builder::new().prefix("oac-wm").tempdir().unwrap();
        let sock = dir.path().join("office.sock");
        let listener = UnixListener::bind(&sock).unwrap();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());

            // 1. `msg.bind` returns a non-zero waiting_messages hint that must NOT leak.
            let bind_req: WireRequest = frame::read_message(&mut reader).unwrap().unwrap();
            assert_eq!(bind_req.method, "msg.bind");
            let bind_resp = WireResponse::ok::<MsgBind>(
                bind_req.id,
                &MsgBindResult {
                    member_id: Some("0193a5c0-0000-7000-8000-000000000003".into()),
                    office_id: "0193a5c0-0000-7000-8000-000000000001".into(),
                },
            )
            .unwrap()
            .with_waiting_messages(99);
            frame::write_message(&mut stream, &bind_resp).unwrap();

            // 2. First command RPC returns `waiting_messages: Some(4)`.
            let req1: WireRequest = frame::read_message(&mut reader).unwrap().unwrap();
            assert_eq!(req1.method, "budget.agent_own");
            let resp1 = WireResponse {
                id: req1.id,
                outcome: org_proto::Outcome::Result(json!({
                    "agent_id": "0193a5c0-0000-7000-8000-000000000003"
                })),
                waiting_messages: None,
            }
            .with_waiting_messages(4);
            frame::write_message(&mut stream, &resp1).unwrap();

            // 3. Second command RPC returns `waiting_messages: None`.
            let req2: WireRequest = frame::read_message(&mut reader).unwrap().unwrap();
            assert_eq!(req2.method, "budget.agent_own");
            let resp2 = WireResponse {
                id: req2.id,
                outcome: org_proto::Outcome::Result(json!({
                    "agent_id": "0193a5c0-0000-7000-8000-000000000003"
                })),
                waiting_messages: None,
            };
            frame::write_message(&mut stream, &resp2).unwrap();
        });

        let mut office = BoundOffice::new(Binding {
            socket: Some(sock),
            office_id: None,
            runtime_dir: None,
            session_token: Some("tok-123".into()),
            agent_id: Some("0193a5c0-0000-7000-8000-000000000003".into()),
        });
        office.ensure_connected().unwrap();
        assert_eq!(office.waiting_messages, None);

        let args: Vec<String> = ["budget", "status", "--format", "json"]
            .into_iter()
            .map(str::to_owned)
            .collect();

        let first = crate::execute_with_office("agent-os", &args, &mut office);
        assert_eq!(first.exit_code, 0, "{}", first.stdout);
        let first_json: Value = serde_json::from_str(&first.stdout).unwrap();
        assert_eq!(first_json["meta"]["waiting_messages"], 4);

        let second = crate::execute_with_office("agent-os", &args, &mut office);
        assert_eq!(second.exit_code, 0, "{}", second.stdout);
        let second_json: Value = serde_json::from_str(&second.stdout).unwrap();
        assert!(second_json["meta"].get("waiting_messages").is_none());

        drop(office);
        server.join().unwrap();
    }
}
