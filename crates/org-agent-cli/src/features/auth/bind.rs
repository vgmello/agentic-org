//! One-step operator-authorized external (BYOA) workspace binding (`TASK-211`):
//! generates a fresh Ed25519 workspace keypair, approves the lease in one operator RPC
//! (`member.external_bind`), and stores the workspace credentials strictly outside Git
//! (`0700` folder, `0600` `key.ed25519.pkcs8` and `binding.v1.json`, never through symlinks).
//! Subsequent `agent-os` commands inside `<workspace>` authenticate immediately via the
//! existing ADR-0036 Ed25519 challenge-exchange flow.

use std::path::{Component, Path, PathBuf};

use org_command::Refusal;
use org_proto::external_auth::{
    ExternalSessionView, MemberExternalBind, MemberExternalBindParams, MemberExternalBindResult,
    MAX_LEASE_TTL_SECS, MAX_WORKSPACE_LABEL_BYTES, MIN_LEASE_TTL_SECS,
};
use org_proto::location::runtime_dir_of_office_socket;
use org_proto::{Call, ErrorCode};
use serde::{Deserialize, Serialize};

use super::client;
use super::store::{self, WorkspaceBinding, WorkspaceState};
use crate::runtime::input::parse_ttl;
use crate::runtime::keys::try_new_uuid_v7;
use crate::runtime::transport::OfficeTransport;

/// Inputs to [`bind_workspace`].
#[derive(Debug, Clone, Copy)]
pub struct WorkspaceBindRequest<'a> {
    /// `@handle`, handle, or agent employee UUIDv7.
    pub member: &'a str,
    /// Existing local workspace directory or Git repository path.
    pub workspace_dir: &'a Path,
    /// Optional human workspace label (defaults to `~/...`).
    pub label: Option<&'a str>,
    /// Optional lease TTL in seconds (`MIN_LEASE_TTL_SECS..=MAX_LEASE_TTL_SECS`).
    pub ttl_secs: Option<u64>,
    /// Park a live managed turn instead of refusing.
    pub takeover: bool,
    /// Optional idempotency/command ID (generated when omitted).
    pub command_id: Option<&'a str>,
    /// Optional explicit state root override (defaults to [`store::state_root`]).
    pub state_root: Option<&'a Path>,
}

/// Non-secret summary of a directly bound workspace (`TASK-211`). Never carries private key
/// bytes or bearer tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundWorkspace {
    pub workspace_root: String,
    pub office_id: String,
    #[serde(flatten)]
    pub session: ExternalSessionView,
}

/// Resolves and validates `dir`: must exist, contain no NUL byte, and be a directory.
pub fn resolve_workspace_dir(dir: &Path) -> Result<PathBuf, Refusal> {
    let raw = dir.as_os_str();
    if raw.is_empty() || raw.as_encoded_bytes().contains(&0) {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "The workspace path must be a non-empty directory path without NUL bytes.",
            "Pass an existing folder to --workspace.",
        ));
    }
    let canonical = dir.canonicalize().map_err(|e| {
        Refusal::new(
            ErrorCode::NotFound,
            format!("The workspace `{}` cannot be resolved: {e}", dir.display()),
            "Pass an existing folder to --workspace.",
        )
    })?;
    let meta = std::fs::metadata(&canonical).map_err(|e| {
        Refusal::new(
            ErrorCode::NotFound,
            format!("The workspace `{}` cannot be inspected: {e}", dir.display()),
            "Pass an existing folder to --workspace.",
        )
    })?;
    if !meta.is_dir() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("The workspace `{}` is not a directory.", dir.display()),
            "Pass an existing directory to --workspace.",
        ));
    }
    Ok(canonical)
}

/// Default readable label (`~/…`) for `root`, keeping the trailing segment within the
/// wire byte budget on a UTF-8 character boundary.
pub fn default_label_for(root: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let label = store::tilde_path(root, home.as_deref());
    if label.len() <= MAX_WORKSPACE_LABEL_BYTES {
        return label;
    }
    let mut start = label.len() - (MAX_WORKSPACE_LABEL_BYTES - '…'.len_utf8());
    while !label.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &label[start..])
}

/// Validates an explicit label or derives the default one for `root`.
pub fn workspace_label(root: &Path, explicit: Option<&str>) -> Result<String, Refusal> {
    let Some(label) = explicit else {
        return Ok(default_label_for(root));
    };
    let trimmed = label.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_WORKSPACE_LABEL_BYTES
        || label.chars().any(client::is_disallowed_display_char)
    {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!(
                "The workspace label must be 1 to {MAX_WORKSPACE_LABEL_BYTES} bytes without control, line-separator, or bidi characters."
            ),
            "Pass a short readable label to --label.",
        ));
    }
    Ok(trimmed.to_owned())
}

/// Parses a `--ttl` duration string and checks the office's external-lease bounds.
pub fn parse_lease_ttl(text: &str) -> Result<u64, Refusal> {
    let secs = parse_ttl(text)?;
    validate_ttl_secs(secs)?;
    Ok(secs)
}

fn validate_ttl_secs(ttl: u64) -> Result<(), Refusal> {
    if !(MIN_LEASE_TTL_SECS..=MAX_LEASE_TTL_SECS).contains(&ttl) {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("--ttl must be between {MIN_LEASE_TTL_SECS}s and {MAX_LEASE_TTL_SECS}s."),
            "Pass --ttl between 5m and 7d (e.g. 8h, 24h, 7d).",
        ));
    }
    Ok(())
}

/// Validates the workspace folder and local state directory before any office call is sent.
pub fn preflight_workspace(
    dir: &Path,
    state_root_override: Option<&Path>,
) -> Result<(PathBuf, WorkspaceState), Refusal> {
    let canonical = resolve_workspace_dir(dir)?;
    let root = match state_root_override {
        Some(path) => path.to_path_buf(),
        None => store::state_root()?,
    };
    let state = WorkspaceState::for_root(&root, &canonical);
    state.preflight()?;
    Ok((canonical, state))
}

fn validate_office_socket(endpoint: Option<&str>) -> Result<Option<String>, Refusal> {
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };
    if endpoint.is_empty() || endpoint.contains('\0') {
        return Err(Refusal::new(
            ErrorCode::Unavailable,
            "The office returned an invalid socket path.",
            "Check that the office is healthy and retry.",
        ));
    }
    let path = Path::new(endpoint);
    let plain = path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
    if !plain {
        return Err(Refusal::new(
            ErrorCode::Unavailable,
            "The office returned an unsafe socket path.",
            "Check that the office is healthy and retry.",
        ));
    }
    Ok(Some(endpoint.to_owned()))
}

/// Validated and preflighted inputs for one-step workspace binding (`TASK-211`), avoiding
/// duplicate workspace resolution, state-directory preflight, or TTL/label checks between
/// dry-run previews and execution.
#[derive(Debug, Clone)]
pub struct PreparedWorkspaceBind {
    pub member: String,
    pub canonical_root: PathBuf,
    pub label: String,
    pub ttl_secs: Option<u64>,
    pub takeover: bool,
    pub command_id: Option<String>,
    state: WorkspaceState,
}

impl PreparedWorkspaceBind {
    /// Validates `req`, resolves the workspace folder, and preflights `[state_root,
    /// state_root/workspaces, self.dir]` without sending any office RPC or creating directories.
    pub fn prepare(req: &WorkspaceBindRequest<'_>) -> Result<Self, Refusal> {
        let member = req.member.trim();
        if member.is_empty() || req.member.chars().any(char::is_control) {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "Missing or invalid agent reference for --member.",
                "Pass an agent @handle or employee UUIDv7 to --member.",
            ));
        }
        if let Some(ttl) = req.ttl_secs {
            validate_ttl_secs(ttl)?;
        }
        let (canonical_root, state) = preflight_workspace(req.workspace_dir, req.state_root)?;
        let label = workspace_label(&canonical_root, req.label)?;
        Ok(Self {
            member: member.to_owned(),
            canonical_root,
            label,
            ttl_secs: req.ttl_secs,
            takeover: req.takeover,
            command_id: req.command_id.map(str::to_owned),
            state,
        })
    }

    /// Reuses an existing valid Ed25519 workspace keypair (or generates a fresh one), records an
    /// approved external lease via `member.external_bind`, and writes the workspace credentials
    /// outside Git.
    pub fn execute<T: OfficeTransport + ?Sized>(
        self,
        transport: &mut T,
    ) -> Result<BoundWorkspace, Refusal> {
        let (pkcs8, pubkey) = client::load_or_generate_key(&self.state)?;
        let command_id = self.command_id.or_else(try_new_uuid_v7);
        let params = MemberExternalBindParams {
            command_id,
            employee_id: self.member,
            pubkey_ed25519_b64: pubkey,
            workspace_label: self.label,
            hostname: client::local_hostname(),
            workspace_path: client::optional_workspace_path(&self.canonical_root),
            ttl_secs: self.ttl_secs,
            takeover: self.takeover,
        };
        let raw_params = serde_json::to_value(&params).map_err(|_| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                "Could not encode member.external_bind parameters.",
                "Check the command flags and retry.",
            )
        })?;
        let raw = transport.call_method(MemberExternalBind::METHOD, raw_params)?;
        let bound: MemberExternalBindResult = serde_json::from_value(raw).map_err(|_| {
            Refusal::new(
                ErrorCode::Unavailable,
                "The office returned an unreadable `member.external_bind` reply.",
                "Retry the command or check the office version.",
            )
        })?;
        let socket = validate_office_socket(bound.office_socket.as_deref())?;
        let runtime_dir = socket
            .as_deref()
            .and_then(|s| runtime_dir_of_office_socket(Path::new(s)))
            .map(|p| p.display().to_string());
        let workspace_root = self.canonical_root.display().to_string();
        self.state.ensure()?;
        // Remove any prior cached fast token before writing the new key/binding so a mid-write
        // failure can never leave a stale token beside newly written state.
        self.state.remove_token()?;
        self.state.write_key(&pkcs8)?;
        self.state.write_binding(&WorkspaceBinding {
            version: 1,
            workspace_root: workspace_root.clone(),
            office_id: bound.office_id.clone(),
            socket,
            runtime_dir,
            member_id: bound.session.employee_id.clone(),
            handle: bound.session.handle.clone(),
            session_id: bound.session.session_id.clone(),
            lease_expires_at: bound.session.expires_at.clone(),
        })?;
        Ok(BoundWorkspace {
            workspace_root,
            office_id: bound.office_id,
            session: bound.session,
        })
    }
}

/// Generates a fresh Ed25519 keypair, records an approved external lease via
/// `member.external_bind`, and writes the workspace credentials outside Git.
pub fn bind_workspace<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    req: &WorkspaceBindRequest<'_>,
) -> Result<BoundWorkspace, Refusal> {
    PreparedWorkspaceBind::prepare(req)?.execute(transport)
}

#[cfg(test)]
#[path = "bind_tests.rs"]
mod tests;
