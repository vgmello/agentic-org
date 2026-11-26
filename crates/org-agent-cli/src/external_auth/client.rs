//! Office-socket calls of external sign-in (`auth.*`) and the workspace key (`TASK-137`).

use std::path::PathBuf;

use base64::Engine as _;
use org_client::Client;
use org_command::Refusal;
use org_proto::external_auth::{
    challenge_message, AuthChallenge, AuthChallengeParams, AuthExchange, AuthExchangeResult,
    ChallengePurpose, SignedChallengeParams,
};
use org_proto::location::{OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::msg::{MsgBind, MsgBindParams};
use org_proto::{Call, ErrorCode};
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use zeroize::Zeroizing;

use crate::office::{map_client_error, RUNTIME_DIR_ENV, SOCKET_ENV};

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
    /// `--socket`, else `--office` (with `AGENT_OS_RUNTIME_DIR`, `ORG_OFFICE_RUNTIME_DIR`,
    /// `ORG_OFFICE_HOME/run` or this account's office home), else `AGENT_OS_SOCKET`.
    pub fn resolve(socket: Option<PathBuf>, office: Option<String>) -> Result<Self, Refusal> {
        if let Some(socket) = socket {
            return Ok(Self {
                socket: Some(socket),
                office_id: office,
                runtime_dir: None,
            });
        }
        if let Some(office_id) = office {
            let runtime_dir = env_path(RUNTIME_DIR_ENV)
                .or_else(|| env_path(OFFICE_RUNTIME_DIR_ENV))
                .or_else(|| env_path(OFFICE_HOME_ENV).map(|home| home.join("run")))
                .or_else(|| org_client::default_office_home().map(|home| home.join("run")));
            return Ok(Self {
                socket: None,
                office_id: Some(office_id),
                runtime_dir,
            });
        }
        if let Some(socket) = env_path(SOCKET_ENV) {
            return Ok(Self {
                socket: Some(socket),
                office_id: None,
                runtime_dir: None,
            });
        }
        Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "No office to sign in to.",
            "Pass --socket <path> or --office <office-id>.",
        ))
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

/// Asks for a `purpose` nonce on lease `session_id` and signs it with the workspace key.
pub fn signed_challenge(
    client: &Client,
    pkcs8: &[u8],
    member_id: &str,
    session_id: &str,
    purpose: ChallengePurpose,
) -> Result<SignedChallengeParams, Refusal> {
    let challenge = call::<AuthChallenge>(
        client,
        &org_proto::external_auth::AuthChallengeParams {
            member: member_id.to_owned(),
            session_id: session_id.to_owned(),
            purpose,
        },
    )?;
    let message = challenge_message(
        purpose,
        &challenge.office_id,
        &challenge.member_id,
        session_id,
        &challenge.nonce,
    );
    let signature = key_pair(pkcs8)?.sign(&message);
    Ok(SignedChallengeParams {
        member: challenge.member_id,
        session_id: session_id.to_owned(),
        nonce: challenge.nonce,
        signature_ed25519_b64: base64::engine::general_purpose::STANDARD.encode(signature.as_ref()),
    })
}

/// The 1-RTT refresh: challenge, sign, exchange for a fresh fast token.
pub fn exchange(
    client: &Client,
    pkcs8: &[u8],
    member_id: &str,
    session_id: &str,
) -> Result<AuthExchangeResult, Refusal> {
    let signed = signed_challenge(
        client,
        pkcs8,
        member_id,
        session_id,
        ChallengePurpose::Exchange,
    )
    .map_err(ended)?;
    call::<AuthExchange>(client, &signed).map_err(ended)
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

/// Parses `--ttl`: `90`, `90s`, `45m`, `24h` or `7d`, in seconds.
pub fn parse_ttl(text: &str) -> Result<u64, Refusal> {
    let text = text.trim();
    let (digits, unit) = match text.char_indices().last() {
        Some((at, c)) if c.is_ascii_alphabetic() => (&text[..at], c),
        _ => (text, 's'),
    };
    let scale = match unit.to_ascii_lowercase() {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => 0,
    };
    digits
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && scale > 0)
        .and_then(|n| n.checked_mul(scale))
        .ok_or_else(|| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                format!("`{text}` is not a duration."),
                "Pass --ttl like 90m, 24h or 7d.",
            )
        })
}

/// The challenge params type, re-exported for callers building their own requests.
pub type ChallengeParams = AuthChallengeParams;
