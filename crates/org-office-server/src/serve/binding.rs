//! Member binding: binding a connection, the live directory lookup, and its outcomes.

use std::sync::atomic::Ordering;

use org_office_sessions::{PeerIdentity, VerifiedRunBinding};
use org_proto::msg::{json_string_len, MsgBind, MsgBindResult, MAX_HANDLE_BYTES, OFFICE_SENDER};
use org_proto::{AgentStatus, ErrorCode, MemberKind, Request, Response};

use super::auth::{Binding, ExternalAuthority};
use super::held::Control;
use super::shared::Shared;
use super::wire::{bind_error, internal};

pub(super) fn lookup_live_binding(
    shared: &Shared,
    binding: &Binding,
    control: &Control,
) -> Option<Lookup> {
    let run_revoked = match binding {
        Binding::External {
            id,
            session_id,
            authority: ExternalAuthority::Verifier,
        } => shared
            .ports
            .run_binding_verifier
            .as_ref()
            .is_none_or(|verifier| !verifier.has_active_external(id, session_id)),
        // An external connection lives exactly as long as its lease's fast token (RAM-only).
        Binding::External {
            id,
            session_id,
            authority: ExternalAuthority::ExternalAuth,
        } => shared
            .ports
            .external_auth
            .as_ref()
            .is_none_or(|port| !port.is_active(id, session_id)),
        _ => {
            !binding.via_grant()
                && binding.member().is_some_and(|member| {
                    shared
                        .ports
                        .run_binding_verifier
                        .as_ref()
                        .is_some_and(|verifier| !verifier.has_active(member))
                })
        }
    };
    if control.revoked.load(Ordering::SeqCst) || run_revoked {
        Some(Lookup::Not)
    } else {
        binding
            .member()
            .map(|member| lookup_binding(shared, member))
    }
}

/// `msg.bind`, in the contract's order (`org_proto::msg::MsgBind`): the input alone, then the
/// member; it fails closed. Each call replaces the previous binding, and a
/// failed one leaves the connection unbound.
pub(super) fn bind_connection(
    request: &Request,
    shared: &Shared,
    binding: &mut Binding,
    peer: &PeerIdentity,
) -> Response {
    *binding = Binding::Anonymous;
    let params = match request.params::<MsgBind>() {
        Ok(params) => params,
        Err(_) => return bind_error(request.id, ErrorCode::InvalidRequest, "bad msg.bind params"),
    };
    // The retired `app_token` is refused while the params decode. An app-wide bind is gone:
    // persons and the owner act through grants on host channels.
    let Some(member) = params.member else {
        return bind_error(request.id, ErrorCode::InvalidRequest, "bind as a member");
    };
    let bound = bind_member(
        request.id,
        shared,
        &member,
        params.session_token.as_deref(),
        peer,
    );
    match bound {
        Ok((now, member_id)) => {
            *binding = now;
            let result = MsgBindResult {
                member_id,
                office_id: shared.office_id.clone(),
            };
            Response::ok::<MsgBind>(request.id, &result).unwrap_or_else(|_| internal(request.id))
        }
        Err(refusal) => refusal,
    }
}

/// As a member: an agent of this office that may message. Office-run agents require a live
/// `AGENT_OS_SESSION_TOKEN` and peer verification via [`org_office_sessions::RunBindingVerifier`]
/// (`CLI-12`, `SUB-3`, `B2 Q9`, `TASK-233`); an external (BYOA) workspace presents its lease's
/// fast token instead (`TASK-137`), which binds the connection as [`Binding::External`].
fn bind_member(
    id: u64,
    shared: &Shared,
    member: &str,
    session_token: Option<&str>,
    peer: &PeerIdentity,
) -> Result<(Binding, Option<String>), Response> {
    // Step 1, which needs no lookup.
    if json_string_len(member) > MAX_HANDLE_BYTES {
        return Err(bind_error(
            id,
            ErrorCode::TooLarge,
            "the member is too long",
        ));
    }
    let handle = org_office::directory::trim_address(member);
    if handle.eq_ignore_ascii_case(OFFICE_SENDER) {
        return Err(bind_error(
            id,
            ErrorCode::InvalidRequest,
            "a connection cannot bind as the office",
        ));
    }
    // Step 2: the member. A lookup that fails, an unknown member, a person (binds only via
    // grants), and a member that cannot message are all `not_authorized`, and say no more.
    let refused = || {
        bind_error(
            id,
            ErrorCode::NotAuthorized,
            "this office cannot bind this member",
        )
    };
    let Some(found) = bindable(shared, handle).filter(|m| m.kind != MemberKind::Person) else {
        return Err(refused());
    };
    let Some(binding) = verify_member_binding(shared, &found, session_token, peer) else {
        return Err(refused());
    };
    Ok((binding, Some(found.id)))
}

fn verify_member_binding(
    shared: &Shared,
    found: &org_proto::Member,
    session_token: Option<&str>,
    peer: &PeerIdentity,
) -> Option<Binding> {
    let Some(verifier) = &shared.ports.run_binding_verifier else {
        let disallowed = session_token.is_some()
            || found
                .agent
                .as_ref()
                .is_some_and(|profile| profile.run_mode.is_some());
        return (!disallowed).then(|| Binding::Member {
            id: found.id.clone(),
            via_grant: false,
        });
    };
    match verifier.verify_binding(&found.id, session_token, peer) {
        Ok(VerifiedRunBinding::Managed(active))
            if active.employee_id == found.id && !active.is_external() =>
        {
            Some(Binding::Member {
                id: found.id.clone(),
                via_grant: false,
            })
        }
        Ok(VerifiedRunBinding::External {
            employee_id,
            session_id,
        }) if employee_id == found.id => Some(Binding::External {
            id: employee_id,
            session_id,
            authority: ExternalAuthority::Verifier,
        }),
        Ok(VerifiedRunBinding::Managed(_) | VerifiedRunBinding::External { .. }) => None,
        Err(_) => {
            let session_id = shared
                .ports
                .external_auth
                .as_ref()
                .and_then(|port| port.verify(&found.id, session_token))?;
            Some(Binding::External {
                id: found.id.clone(),
                session_id,
                authority: ExternalAuthority::ExternalAuth,
            })
        }
    }
}

/// The member `handle` names if a connection may act as it: a person or the owner, or an agent
/// of this office that is active or paused and not office-run (an office-run agent needs a session
/// token, which this office does not issue yet). `None` for anything else and for a lookup that
/// fails: it fails closed. Used at bind and again on every later call of the connection.
fn bindable(shared: &Shared, handle: &str) -> Option<org_proto::Member> {
    match lookup(shared, handle) {
        Lookup::Bindable(member) => Some(*member),
        Lookup::Not | Lookup::Failed => None,
    }
}

/// What a look at a member found.
pub(super) enum Lookup {
    Bindable(Box<org_proto::Member>),
    /// The member is there to see and may not be bound (or is not there).
    Not,
    /// The directory could not say. A bind or a call fails closed on it; the watcher does not
    /// end a connection on it (the next look, or the connection's next call, decides).
    Failed,
}

pub(super) fn lookup(shared: &Shared, handle: &str) -> Lookup {
    let found = match shared.ports.directory.resolve_member(handle) {
        Ok(Some((found, _))) => found,
        Ok(None) => return Lookup::Not,
        Err(_) => return Lookup::Failed,
    };
    let may = match found.kind {
        MemberKind::Person => true,
        MemberKind::Agent => {
            found.office_id.as_deref() == Some(shared.office_id.as_str())
                && found.agent.as_ref().is_some_and(|profile| {
                    if shared.ports.run_binding_verifier.is_some() {
                        profile.status == AgentStatus::Active
                    } else {
                        matches!(profile.status, AgentStatus::Active | AgentStatus::Paused)
                            && profile.run_mode.is_none()
                    }
                })
        }
        _ => false,
    };
    if may {
        Lookup::Bindable(Box::new(found))
    } else {
        Lookup::Not
    }
}

/// Resolves an existing binding by identity: another member's matching handle cannot keep it.
pub(super) fn lookup_binding(shared: &Shared, id: &str) -> Lookup {
    match lookup(shared, id) {
        Lookup::Bindable(member) if member.id != id => Lookup::Not,
        found => found,
    }
}

/// Whether the member `id` is certainly no longer bindable. A directory that could not answer is
/// not certain.
pub(super) fn is_gone(shared: &Shared, id: &str) -> bool {
    matches!(lookup_binding(shared, id), Lookup::Not)
}
