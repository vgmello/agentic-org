//! Office-socket calls of external sign-in (`auth.*`) and the workspace key (`TASK-137`).

use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use org_client::locator::{self, DiscoveredOffice, ResolveOfficeError};
use org_client::Client;
use org_command::Refusal;
use org_proto::external_auth::{
    challenge_message, AuthChallenge, AuthChallengeParams, AuthChallengeResult, AuthExchange,
    AuthExchangeResult, ChallengePurpose, SignedChallengeParams, MAX_HOSTNAME_BYTES,
    MAX_WORKSPACE_PATH_BYTES,
};
use org_proto::location::{OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::msg::{MsgBind, MsgBindParams};
use org_proto::{Call, ErrorCode};
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use zeroize::Zeroizing;

use super::store::WorkspaceState;
use crate::runtime::binding::{map_client_error, OFFICE_ENV, RUNTIME_DIR_ENV, SOCKET_ENV};

/// Which office socket to talk to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    pub socket: Option<PathBuf>,
    pub office_id: Option<String>,
    pub runtime_dir: Option<PathBuf>,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl Target {
    /// `--socket`, else `--office` (explicit runtime dir env or `locator/instances.json`), else
    /// `AGENT_OS_SOCKET` / `AGENT_OS_OFFICE`, else auto-discovery of the active local office from
    /// `locator/instances.json` + saved CLI context fallback.
    pub fn resolve(socket: Option<PathBuf>, office: Option<String>) -> Result<Self, Refusal> {
        if let Some(socket) = socket {
            return Ok(Self {
                socket: Some(socket),
                office_id: office,
                runtime_dir: None,
            });
        }
        if let Some(office_selector) = office {
            return Self::resolve_named_office(office_selector);
        }
        if let Some(socket) = env_path(SOCKET_ENV) {
            return Ok(Self {
                socket: Some(socket),
                office_id: None,
                runtime_dir: None,
            });
        }
        if let Some(env_office) = std::env::var(OFFICE_ENV)
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
        {
            return Self::resolve_named_office(env_office);
        }
        match locator::resolve_local_office(None) {
            Ok(found) => Ok(Self::from_discovered(found, None)),
            Err(ResolveOfficeError::Ambiguous { candidates, .. }) => Err(ambiguous_refusal(None, &candidates)),
            Err(ResolveOfficeError::NotFound { .. }) => Err(Refusal::new(
                ErrorCode::NotFound,
                "No active local office was found.",
                "Start your office (`agentic-org service start`) or pass --office <office-id> / --socket <path>.",
            )),
        }
    }

    fn resolve_named_office(selector: String) -> Result<Self, Refusal> {
        let explicit_rt = env_path(RUNTIME_DIR_ENV).or_else(|| env_path(OFFICE_RUNTIME_DIR_ENV));
        if let Some(runtime_dir) = explicit_rt.filter(|_| org_proto::is_uuid_v7(&selector)) {
            return Ok(Self {
                socket: None,
                office_id: Some(selector),
                runtime_dir: Some(runtime_dir),
            });
        }
        match locator::resolve_local_office(Some(&selector)) {
            Ok(found) => Ok(Self::from_discovered(found, Some(selector))),
            Err(ResolveOfficeError::Ambiguous { candidates, .. }) => {
                Err(ambiguous_refusal(Some(&selector), &candidates))
            }
            Err(ResolveOfficeError::NotFound { .. }) => {
                if org_proto::is_uuid_v7(&selector) {
                    let runtime_dir = env_path(OFFICE_HOME_ENV)
                        .map(|home| home.join("run"))
                        .or_else(|| org_client::default_office_home().map(|home| home.join("run")));
                    return Ok(Self {
                        socket: None,
                        office_id: Some(selector),
                        runtime_dir,
                    });
                }
                Err(Refusal::new(
                    ErrorCode::NotFound,
                    format!("No active local office matched `{selector}`."),
                    "Check `agentic-org office list` and pass --office <office-id> or --socket <path>.",
                ))
            }
        }
    }

    fn from_discovered(found: DiscoveredOffice, selector: Option<String>) -> Self {
        let office_id = found
            .office_id
            .or_else(|| selector.filter(|s| org_proto::is_uuid_v7(s)));
        Self {
            socket: Some(found.socket),
            office_id,
            runtime_dir: Some(found.runtime_dir),
        }
    }

    /// A fresh connection to the office socket.
    pub fn connect(&self) -> Result<Client, Refusal> {
        let connected = match (&self.socket, &self.office_id, &self.runtime_dir) {
            (Some(socket), _, _) => Client::connect(socket),
            (None, Some(office), Some(dir)) => Client::connect_selected_office(office, dir),
            _ => {
                return Err(Refusal::new(
                    ErrorCode::NotFound,
                    "The office socket cannot be found.",
                    "Pass --socket <path>, or set AGENT_OS_RUNTIME_DIR for --office.",
                ))
            }
        };
        connected.map_err(|e| match e {
            org_client::ClientError::NotRunning(_)
            | org_client::ClientError::OfficeNotReachable => Refusal::new(
                ErrorCode::NotFound,
                "The office is not running or its socket was not found.",
                "Check that the office is active and --socket / --office is correct.",
            ),
            other => map_client_error(other),
        })
    }
}

/// One typed call, refusals mapped like every other `agent-os` call.
pub fn call<C: Call>(client: &Client, params: &C::Params) -> Result<C::Result, Refusal> {
    client.call::<C>(params).map_err(map_client_error)
}

/// A new workspace key: PKCS#8 DER (kept secret) and the base64 public key.
pub fn generate_key() -> Result<(Zeroizing<Vec<u8>>, String), Refusal> {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "System entropy is unavailable.",
            "Retry the sign-in.",
        )
    })?;
    let pkcs8 = Zeroizing::new(pkcs8.as_ref().to_vec());
    let public = public_key_b64(&pkcs8)?;
    Ok((pkcs8, public))
}

/// Reuses this workspace's existing Ed25519 keypair (`key.ed25519.pkcs8`) when present and valid,
/// or generates a new one otherwise (`TASK-248`). Preflights the state tree first so a symlinked
/// or group-writable state directory fails closed before any RPC.
pub fn load_or_generate_key(
    state: &WorkspaceState,
) -> Result<(Zeroizing<Vec<u8>>, String), Refusal> {
    state.preflight()?;
    if let Some(existing) = std::fs::symlink_metadata(state.dir())
        .is_ok()
        .then(|| state.read_key())
        .transpose()?
        .flatten()
    {
        if let Ok(pubkey) = public_key_b64(&existing) {
            return Ok((existing, pubkey));
        }
    }
    generate_key()
}

/// Rejects control characters, Unicode line/paragraph separators, and bidirectional
/// overrides/isolates that can spoof terminal or web approval surfaces.
pub(crate) fn is_disallowed_display_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{2028}'
                | '\u{2029}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
        )
}

/// Formats `path` for the optional `workspace_path` wire field, omitting it (`None`) when it is
/// non-UTF-8, exceeds `MAX_WORKSPACE_PATH_BYTES`, contains control/line-separator/bidi
/// characters, or is not a clean absolute path.
pub(crate) fn optional_workspace_path(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if text.is_empty()
        || text.len() > MAX_WORKSPACE_PATH_BYTES
        || text.chars().any(is_disallowed_display_char)
        || !path.is_absolute()
        || !path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
    {
        return None;
    }
    if text != "/" {
        if text.ends_with('/') {
            return None;
        }
        let rest = text.strip_prefix('/')?;
        if rest
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
        {
            return None;
        }
    }
    Some(text.to_owned())
}

/// Best-effort local host name (safe, zero `unsafe`, zero platform `#[cfg]`), sanitized to the
/// wire budget (`MAX_HOSTNAME_BYTES`).
pub fn local_hostname() -> Option<String> {
    for var in ["HOSTNAME", "HOST"] {
        if let Some(name) = std::env::var(var).ok().and_then(|v| sanitize_hostname(&v)) {
            return Some(name);
        }
    }
    if let Some(name) = read_hostname_file(Path::new("/etc/hostname")) {
        return Some(name);
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| sanitize_hostname(&text))
}

fn read_hostname_file(path: &Path) -> Option<String> {
    let file = org_platform::fs::open_nofollow_ro(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_HOSTNAME_BYTES as u64 + 16)
        .read_to_string(&mut text)
        .ok()?;
    sanitize_hostname(&text)
}

pub(crate) fn sanitize_hostname(raw: &str) -> Option<String> {
    let trimmed = raw.trim_matches([' ', '\r', '\n']);
    if trimmed.is_empty()
        || trimmed.len() > MAX_HOSTNAME_BYTES
        || trimmed.chars().any(is_disallowed_display_char)
    {
        return None;
    }
    Some(trimmed.to_owned())
}

fn ambiguous_refusal(selector: Option<&str>, candidates: &[DiscoveredOffice]) -> Refusal {
    let err = ResolveOfficeError::Ambiguous {
        selector: selector.map(str::to_owned),
        candidates: candidates.to_vec(),
    };
    Refusal::new(
        ErrorCode::InvalidRequest,
        err.to_string(),
        "Pass --office <office-id> (or --socket <path>) to choose one office.",
    )
}

fn key_pair(pkcs8: &[u8]) -> Result<Ed25519KeyPair, Refusal> {
    Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "The workspace key is not a valid Ed25519 key.",
            "Run `agent-os logout`, then `agent-os login` again.",
        )
    })
}

/// The base64 public key of a PKCS#8 workspace key.
pub fn public_key_b64(pkcs8: &[u8]) -> Result<String, Refusal> {
    Ok(base64::engine::general_purpose::STANDARD.encode(key_pair(pkcs8)?.public_key().as_ref()))
}

/// What a signed challenge is for: the office and agent the workspace signed in to, and its
/// lease session. The client signs only a nonce the office issued for exactly these.
#[derive(Debug, Clone, Copy)]
pub struct Signer<'a> {
    pub pkcs8: &'a [u8],
    /// The office the workspace signed in to; `None` only before the first answer names it.
    pub office_id: Option<&'a str>,
    pub member_id: &'a str,
    pub session_id: &'a str,
}

fn foreign_challenge() -> Refusal {
    Refusal::new(
        ErrorCode::NotAuthorized,
        "The office socket answered for another office or agent; nothing was signed.",
        "Check --socket / --office, then run `agent-os login --as @<handle>` again.",
    )
}

/// Asks the office for a `purpose` nonce on the signer's lease session.
fn request_challenge(
    client: &Client,
    signer: &Signer<'_>,
    purpose: ChallengePurpose,
) -> Result<AuthChallengeResult, Refusal> {
    call::<AuthChallenge>(
        client,
        &org_proto::external_auth::AuthChallengeParams {
            member: signer.member_id.to_owned(),
            session_id: signer.session_id.to_owned(),
            purpose,
        },
    )
}

/// Signs `challenge` with the workspace key, after checking it was issued for the signer's own
/// office and agent: a challenge for anyone else is refused before anything is signed, so a
/// socket that is not the signed-in office cannot use the workspace key as a signing oracle.
fn sign(
    signer: &Signer<'_>,
    purpose: ChallengePurpose,
    challenge: AuthChallengeResult,
) -> Result<SignedChallengeParams, Refusal> {
    if challenge.member_id != signer.member_id
        || signer
            .office_id
            .is_some_and(|office| office != challenge.office_id)
    {
        return Err(foreign_challenge());
    }
    let message = challenge_message(
        purpose,
        &challenge.office_id,
        signer.member_id,
        signer.session_id,
        &challenge.nonce,
    );
    let signature = key_pair(signer.pkcs8)?.sign(&message);
    Ok(SignedChallengeParams {
        member: signer.member_id.to_owned(),
        session_id: signer.session_id.to_owned(),
        nonce: challenge.nonce,
        signature_ed25519_b64: base64::engine::general_purpose::STANDARD.encode(signature.as_ref()),
    })
}

/// Asks for a `purpose` nonce on the signer's lease session and signs it ([`sign`]).
pub fn signed_challenge(
    client: &Client,
    signer: &Signer<'_>,
    purpose: ChallengePurpose,
) -> Result<SignedChallengeParams, Refusal> {
    sign(signer, purpose, request_challenge(client, signer, purpose)?)
}

/// The 1-RTT refresh: challenge, sign, exchange for a fresh fast token. A token issued for
/// another agent or lease session than the signer's is refused.
pub fn exchange(client: &Client, signer: &Signer<'_>) -> Result<AuthExchangeResult, Refusal> {
    let challenge = request_challenge(client, signer, ChallengePurpose::Exchange).map_err(ended)?;
    let signed = sign(signer, ChallengePurpose::Exchange, challenge)?;
    let fresh = call::<AuthExchange>(client, &signed).map_err(ended)?;
    if fresh.member_id != signer.member_id || fresh.session_id != signer.session_id {
        return Err(foreign_challenge());
    }
    Ok(fresh)
}

/// A refused challenge or exchange means the lease is gone.
fn ended(refusal: Refusal) -> Refusal {
    if refusal.code == ErrorCode::NotAuthorized {
        Refusal::new(
            ErrorCode::NotAuthorized,
            "This workspace's external session has ended (revoked, superseded or expired).",
            "Run `agent-os login --as @<handle>` again.",
        )
    } else {
        refusal
    }
}

/// Binds `client` as `member_id` with a fast token.
pub fn bind(client: &Client, member_id: &str, token: &str) -> Result<(), Refusal> {
    call::<MsgBind>(
        client,
        &MsgBindParams::member_with_token(member_id.to_owned(), token.to_owned()),
    )
    .map(|_| ())
}

/// The challenge params type, re-exported for callers building their own requests.
pub type ChallengeParams = AuthChallengeParams;
