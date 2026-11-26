//! External (BYOA) agent sign-in on the office socket and its operator calls (`TASK-137`).
//!
//! - `auth.login_request|login_poll|challenge|exchange|logout`: answered only on the office
//!   socket and only before a connection binds. The socket's accept gate admits same-UID peers
//!   only (`peer_allowed`); each connection has a call budget; the port's books are bounded.
//! - `auth.whoami`: a bound connection's member, mode (`managed`|`external`) and expiries.
//! - `member.external_approve|sessions|revoke`: operator calls, only through a verified grant
//!   (a person or the owner, on a host channel). An agent on the office socket, managed or
//!   external, can never approve, list or revoke a lease.
//! - An externally bound connection may only make the organization's collaboration calls
//!   ([`external_may_call`]): never host administration, provider credentials or raw data.

use org_proto::external_auth::{
    AuthChallenge, AuthChallengeResult, AuthExchange, AuthExchangeResult, AuthLoginPoll,
    AuthLoginPollResult, AuthLoginRequest, AuthLoginRequestParams, AuthLoginRequestResult,
    AuthLogout, AuthLogoutResult, AuthWhoami, AuthWhoamiResult, ChallengePurpose,
    ExternalSessionView, MemberExternalApprove, MemberExternalApproveParams, MemberExternalRevoke,
    MemberExternalRevokeResult, MemberExternalSessions, MemberExternalSessionsResult,
    SignedChallengeParams,
};
use org_proto::msg::{json_string_len, MAX_HANDLE_BYTES};
use org_proto::{ApiError, Call, ErrorCode, MemberKind, Request, Response};

use super::auth::Binding;
use super::binding::{lookup, Lookup};
use super::org::revoke_bound_member;
use super::shared::Shared;
use super::wire::internal;

/// What the office socket asks of external (BYOA) agent auth. Implemented by the office's
/// session manager; every check on the bound hot path ([`Self::verify`], [`Self::is_active`])
/// is RAM-only.
pub trait ExternalAuthPort: Send + Sync {
    fn login_request(
        &self,
        member_id: &str,
        params: &AuthLoginRequestParams,
    ) -> Result<AuthLoginRequestResult, ApiError>;
    fn login_poll(&self, request_id: &str) -> Result<AuthLoginPollResult, ApiError>;
    fn challenge(
        &self,
        member_id: &str,
        session_id: &str,
        purpose: ChallengePurpose,
    ) -> Result<AuthChallengeResult, ApiError>;
    fn exchange(
        &self,
        member_id: &str,
        params: &SignedChallengeParams,
    ) -> Result<AuthExchangeResult, ApiError>;
    fn logout(
        &self,
        member_id: &str,
        params: &SignedChallengeParams,
    ) -> Result<AuthLogoutResult, ApiError>;
    fn whoami(&self, member_id: &str, external_session: Option<&str>) -> AuthWhoamiResult;
    fn approve(
        &self,
        actor: &str,
        params: &MemberExternalApproveParams,
    ) -> Result<ExternalSessionView, ApiError>;
    fn sessions(
        &self,
        actor: &str,
        employee_id: &str,
        limit: Option<u32>,
    ) -> Result<MemberExternalSessionsResult, ApiError>;
    fn revoke(
        &self,
        actor: &str,
        employee_id: &str,
        session_id: Option<&str>,
    ) -> Result<MemberExternalRevokeResult, ApiError>;
    /// The lease session `token` is the live fast token of, for `member_id`.
    fn verify(&self, member_id: &str, token: Option<&str>) -> Option<String>;
    /// Whether an externally bound connection of lease `session_id` may keep acting.
    fn is_active(&self, member_id: &str, session_id: &str) -> bool;
}

/// Most `auth.*` calls one office-socket connection may make; the next one closes it.
pub(super) const MAX_AUTH_CALLS_PER_CONNECTION: u32 = 64;

/// Calls an externally bound connection may make by exact name.
const EXTERNAL_CALLS: &[&str] = &[
    "who",
    "status.set",
    "org.snapshot",
    "project.list",
    "project.inspect",
    "skill.list",
    "skill.read",
    "process.show",
    "process.effective",
    "budget.agent_own",
    "subscribe",
];

/// Call families an externally bound connection may make: messages, chat and tasks.
const EXTERNAL_FAMILIES: &[&str] = &["msg.", "chat.", "dm.", "tasks."];

/// Whether an externally bound connection may call `method`: the organization's collaboration
/// primitives only.
pub(super) fn external_may_call(method: &str) -> bool {
    EXTERNAL_CALLS.contains(&method)
        || EXTERNAL_FAMILIES
            .iter()
            .any(|family| method.starts_with(family))
}

/// The refusal an externally bound connection gets for anything else.
pub(super) fn external_refusal(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::NotAuthorized,
            "an external agent session may only use collaboration commands (messages, chat, tasks, directory, status)",
        ),
    )
}

/// One `auth.*` answer, and whether the connection must close after it.
pub(super) struct AuthAnswer {
    pub(super) response: Response,
    pub(super) close: bool,
}

fn error(id: u64, code: ErrorCode, message: &str) -> Response {
    Response::error(id, ApiError::new(code, message))
}

fn reply<C: Call>(id: u64, outcome: Result<C::Result, ApiError>) -> Response {
    match outcome {
        Ok(result) => Response::ok::<C>(id, &result).unwrap_or_else(|_| internal(id)),
        Err(refusal) => Response::error(id, refusal),
    }
}

fn params<C: Call>(request: &Request) -> Result<C::Params, Response> {
    request.params::<C>().map_err(|_| {
        error(
            request.id,
            ErrorCode::InvalidRequest,
            &format!("invalid `{}` parameters", C::METHOD),
        )
    })
}

/// `@handle`, handle or id, without whitespace or the `@`.
fn member_reference(text: &str) -> &str {
    let trimmed = org_office::directory::trim_address(text);
    trimmed.strip_prefix('@').unwrap_or(trimmed)
}

/// The id of the active agent of this office `member` names. Anything else is the same opaque
/// refusal a bind gets.
fn signin_agent(shared: &Shared, member: &str) -> Result<String, ApiError> {
    let refused = || {
        ApiError::new(
            ErrorCode::NotAuthorized,
            "this office cannot sign in this member",
        )
    };
    if json_string_len(member) > MAX_HANDLE_BYTES {
        return Err(refused());
    }
    match lookup(shared, member_reference(member)) {
        Lookup::Bindable(found) if found.kind == MemberKind::Agent => Ok(found.id),
        Lookup::Failed => Err(ApiError::new(
            ErrorCode::Unavailable,
            "the directory could not answer",
        )),
        _ => Err(refused()),
    }
}

/// Answers an `auth.*` call on the office socket; `None` for any other method.
pub(super) fn answer_auth(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    calls: &mut u32,
) -> Option<AuthAnswer> {
    if !request.method.starts_with("auth.") {
        return None;
    }
    let id = request.id;
    *calls = calls.saturating_add(1);
    if *calls > MAX_AUTH_CALLS_PER_CONNECTION {
        return Some(AuthAnswer {
            response: error(
                id,
                ErrorCode::Unavailable,
                "too many sign-in calls on this connection; reconnect",
            ),
            close: true,
        });
    }
    let response = match shared.ports.external_auth.as_deref() {
        None => error(
            id,
            ErrorCode::Unsupported,
            "this office does not serve external agents",
        ),
        Some(port) if request.is::<AuthWhoami>() => match binding.member() {
            Some(member) => {
                reply::<AuthWhoami>(id, Ok(port.whoami(member, binding.external_session())))
            }
            None => error(id, ErrorCode::NotBound, "bind before asking who you are"),
        },
        Some(_) if binding.is_bound() => error(
            id,
            ErrorCode::InvalidRequest,
            "sign-in calls are made on a connection that is not bound",
        ),
        Some(port) => answer_signin(request, shared, port),
    };
    Some(AuthAnswer {
        response,
        close: false,
    })
}

fn answer_signin(request: &Request, shared: &Shared, port: &dyn ExternalAuthPort) -> Response {
    let id = request.id;
    if request.is::<AuthLoginRequest>() {
        let p = match params::<AuthLoginRequest>(request) {
            Ok(p) => p,
            Err(refusal) => return refusal,
        };
        let outcome = signin_agent(shared, &p.member).and_then(|m| port.login_request(&m, &p));
        reply::<AuthLoginRequest>(id, outcome)
    } else if request.is::<AuthLoginPoll>() {
        match params::<AuthLoginPoll>(request) {
            Ok(p) => reply::<AuthLoginPoll>(id, port.login_poll(&p.request_id)),
            Err(refusal) => refusal,
        }
    } else if request.is::<AuthChallenge>() {
        let p = match params::<AuthChallenge>(request) {
            Ok(p) => p,
            Err(refusal) => return refusal,
        };
        let outcome = signin_agent(shared, &p.member)
            .and_then(|m| port.challenge(&m, &p.session_id, p.purpose));
        reply::<AuthChallenge>(id, outcome)
    } else if request.is::<AuthExchange>() {
        let p = match params::<AuthExchange>(request) {
            Ok(p) => p,
            Err(refusal) => return refusal,
        };
        let outcome = signin_agent(shared, &p.member).and_then(|m| port.exchange(&m, &p));
        reply::<AuthExchange>(id, outcome)
    } else if request.is::<AuthLogout>() {
        let p = match params::<AuthLogout>(request) {
            Ok(p) => p,
            Err(refusal) => return refusal,
        };
        let outcome = signin_agent(shared, &p.member).and_then(|m| port.logout(&m, &p));
        if let Ok(done) = &outcome {
            // Every connection still bound with the ended lease closes now.
            revoke_bound_member(shared, &done.member_id);
        }
        reply::<AuthLogout>(id, outcome)
    } else {
        error(
            id,
            ErrorCode::UnknownMethod,
            &format!("this office does not know `{}`", request.method),
        )
    }
}

/// The id an operator call names: `@handle`, handle or id of any member the directory knows (a
/// paused or offboarded agent included, so its lease can still be listed or revoked).
fn operator_target(shared: &Shared, reference: &str) -> Result<String, ApiError> {
    let reference = member_reference(reference);
    match shared.ports.directory.resolve_member(reference) {
        Ok(Some((member, _))) => Ok(member.id),
        Ok(None) if org_proto::is_uuid_v7(reference) => Ok(reference.to_owned()),
        Ok(None) => Err(ApiError::new(ErrorCode::NotFound, "no such agent")),
        Err(_) => Err(ApiError::new(
            ErrorCode::StorageError,
            "the directory could not answer",
        )),
    }
}

/// Answers `member.external_approve|sessions|revoke`; `None` for any other method.
pub(super) fn answer_operator(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    let approve = request.is::<MemberExternalApprove>();
    let sessions = request.is::<MemberExternalSessions>();
    if !(approve || sessions || request.is::<MemberExternalRevoke>()) {
        return None;
    }
    let id = request.id;
    if !binding.via_grant() {
        return Some(error(
            id,
            ErrorCode::NotAuthorized,
            "only a signed-in person may approve, list or revoke external agent sessions",
        ));
    }
    let Some(actor) = binding.member() else {
        return Some(internal(id));
    };
    let Some(port) = shared.ports.external_auth.as_deref() else {
        return Some(error(
            id,
            ErrorCode::Unsupported,
            "this office does not serve external agents",
        ));
    };
    Some(if approve {
        match params::<MemberExternalApprove>(request) {
            Ok(p) => {
                let outcome = port.approve(actor, &p);
                if let Ok(view) = &outcome {
                    // A superseded lease's or a parked run's connections close now.
                    revoke_bound_member(shared, &view.employee_id);
                }
                reply::<MemberExternalApprove>(id, outcome)
            }
            Err(refusal) => refusal,
        }
    } else if sessions {
        match params::<MemberExternalSessions>(request) {
            Ok(p) => {
                let outcome = operator_target(shared, &p.employee_id)
                    .and_then(|target| port.sessions(actor, &target, p.limit));
                reply::<MemberExternalSessions>(id, outcome)
            }
            Err(refusal) => refusal,
        }
    } else {
        match params::<MemberExternalRevoke>(request) {
            Ok(p) => {
                let outcome = operator_target(shared, &p.employee_id)
                    .and_then(|target| port.revoke(actor, &target, p.session_id.as_deref()));
                if let Ok(done) = &outcome {
                    if done.revoked {
                        revoke_bound_member(shared, &done.employee_id);
                    }
                }
                reply::<MemberExternalRevoke>(id, outcome)
            }
            Err(refusal) => refusal,
        }
    })
}
