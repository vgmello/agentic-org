//! The bound office of `agent-os`: an [`OfficeTransport`] over `org-client` that connects lazily
//! and binds once. It knows only its transport: the command table and its dispatch live in the
//! crate's registry, and a signed-in workspace's challenge-response is the [`WorkspaceConnector`]
//! the composition root gives it, so this kernel names no feature.

use org_client::Client;
use org_command::Refusal;
use org_proto::location::selected_office_socket_in;
use org_proto::msg::{MsgBind, MsgBindParams};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::runtime::binding::{map_client_error, map_connect_error, Binding};
use crate::runtime::transport::OfficeTransport;

/// Connects and binds the office of a signed-in external workspace (`TASK-137`), refreshing its
/// token when stale or refused, and records the fresh token in the binding.
pub type WorkspaceConnector = fn(&mut Binding) -> Result<Client, Refusal>;

/// Connects lazily to the bound office socket, binds once, and carries each call's
/// `waiting_messages` hint until the command that made it takes it.
pub struct BoundOffice {
    binding: Binding,
    client: Option<Client>,
    /// Per-command `waiting_messages` hint returned by the most recent office RPC call.
    waiting_messages: Option<u32>,
    /// Whether the caller asked for a binding ([`OfficeTransport::is_bound`]).
    bound: bool,
    /// The employee the office bound the connection to (`msg.bind`), once connected.
    bound_member: Option<String>,
    /// How a signed-in workspace binding connects ([`Self::with_workspace_connector`]).
    workspace: Option<WorkspaceConnector>,
}

impl BoundOffice {
    /// An office for `binding`, bound when it carries a session token or names a signed-in
    /// workspace (`TASK-137`), whose token is refreshed when the connection is made.
    pub fn new(binding: Binding) -> Self {
        let bound = binding.session_token.is_some() || binding.external_state.is_some();
        Self {
            binding,
            client: None,
            waiting_messages: None,
            bound,
            bound_member: None,
            workspace: None,
        }
    }

    /// Connects a signed-in workspace binding (`external_state`) with `connector`. Without one,
    /// such a binding is refused rather than connected unbound.
    pub fn with_workspace_connector(mut self, connector: WorkspaceConnector) -> Self {
        self.workspace = Some(connector);
        self
    }

    /// Marks the office bound because the environment asked for a binding
    /// ([`env_requests_binding`](crate::runtime::binding::env_requests_binding)), even when its
    /// token could not be read: then every office command fails closed instead of running
    /// unbound.
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
                "Check that AGENT_OS_BINDING_FILE names this session's owner-only binding file (a regular file, not a symlink, at most 4 KiB), or start the session again; `agent-os context --offline` works without the office.",
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
        let client = match self.client.take() {
            Some(client) => client,
            None => {
                let (client, member) = if self.binding.external_state.is_some() {
                    let connect = self.workspace.ok_or_else(|| {
                        Refusal::new(
                            ErrorCode::NotAuthorized,
                            "This signed-in workspace binding has no connector to refresh its token.",
                            "Run the command with agent-os itself, or bind with AGENT_OS_SESSION_TOKEN.",
                        )
                    })?;
                    (connect(&mut self.binding)?, self.binding.agent_id.clone())
                } else {
                    self.connect_client()?
                };
                self.bound_member = member;
                client
            }
        };
        Ok(self.client.insert(client))
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

    fn session_mode(&self) -> Option<org_proto::external_auth::SessionMode> {
        self.binding
            .external_state
            .is_some()
            .then_some(org_proto::external_auth::SessionMode::External)
    }

    fn take_waiting_messages(&mut self) -> Option<u32> {
        self.waiting_messages.take()
    }
}

#[cfg(test)]
mod tests;
