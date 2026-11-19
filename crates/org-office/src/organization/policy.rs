//! Organization-owned validation, identity and relationship invariants.
use super::OrgView;
use org_proto::org::{
    AgentStatus, Member, MemberKind, AGENT_TEXT_MAX_CHARS, OFFICE_HANDLE, OWNER_HANDLE,
};
use org_proto::{ApiError, ErrorCode, NameError, OrganizationName};
use std::collections::HashSet;

pub(crate) fn fail(code: ErrorCode, message: &str) -> ApiError {
    ApiError::new(code, message)
}

pub(crate) fn valid_owner(member: &Member) -> bool {
    member.kind == MemberKind::Person
        && member.agent.is_none()
        && member.reports_to_member_id.is_none()
}

/// Whether `member` is an agent that was offboarded: a record that is history.
pub(crate) fn is_offboarded(member: &Member) -> bool {
    member
        .agent
        .as_ref()
        .is_some_and(|agent| agent.status == AgentStatus::Offboarded)
}

pub(crate) fn found<T>(value: Option<T>, what: &str) -> Result<T, ApiError> {
    value.ok_or_else(|| fail(ErrorCode::NotFound, what))
}

pub(crate) fn check_version(current: u64, expected: u64) -> Result<(), ApiError> {
    if current == expected {
        Ok(())
    } else {
        Err(fail(
            ErrorCode::Conflict,
            "it changed since you last looked; refresh and try again",
        ))
    }
}

/// The key sibling names are compared by (ignoring case and edges) for a name already stored.
/// A stored name always parses; the fallback only keeps a damaged row from panicking.
pub fn name_key(stored: &str) -> String {
    OrganizationName::parse(stored)
        .map(|n| n.key().to_owned())
        .unwrap_or_else(|_| stored.trim().to_lowercase())
}

/// A team's name, the owner's display name, or any other name that is not a handle. (A person's
/// name is a handle: `parse_handle`.) `NameError::Empty` speaks of "the organization", so the empty
/// case gets its own words here; the other messages are about "the name" in general.
pub(crate) fn parse_name(raw: &str) -> Result<OrganizationName, ApiError> {
    OrganizationName::parse(raw).map_err(|reason| {
        let message = match reason {
            NameError::Empty => "Give it a name.".to_owned(),
            other => other.message(),
        };
        ApiError::new(ErrorCode::InvalidName, message)
    })
}

/// A person's name, which is also the handle messaging addresses them by: a name, and one that
/// does not look like what messaging already addresses by (a member id, a run), so a handle never
/// shadows either.
pub(crate) fn parse_handle(raw: &str) -> Result<OrganizationName, ApiError> {
    let name = parse_name(raw)?;
    if org_proto::is_shaped_like_an_address(name.as_str()) {
        return Err(ApiError::new(
            ErrorCode::InvalidName,
            "a name can't look like a member id or start with `run:`",
        ));
    }
    Ok(name)
}

pub(crate) fn sibling_clash() -> ApiError {
    fail(
        ErrorCode::AlreadyExists,
        "a team with that name already exists here",
    )
}

pub(crate) fn owner_protected(message: &str) -> ApiError {
    fail(ErrorCode::OwnerProtected, message)
}

/// A client-made id: the contract's `org_proto::is_valid_id`.
pub(crate) fn check_id(id: &str) -> Result<(), ApiError> {
    if org_proto::is_valid_id(id) {
        Ok(())
    } else {
        Err(fail(
            ErrorCode::InvalidRequest,
            "an id is 1 to 64 letters, digits, '-' or '_'",
        ))
    }
}

/// A member id: exactly 26 ASCII letters and digits, the shape of a ULID. No handle has this
/// shape (`parse_handle` refuses it), so an id and a handle never name different members.
pub(crate) fn check_member_id(id: &str) -> Result<(), ApiError> {
    if org_proto::is_valid_member_id(id) {
        Ok(())
    } else {
        Err(fail(
            ErrorCode::InvalidRequest,
            "a member id is exactly 26 letters and digits",
        ))
    }
}

/// Optional free text (a contact, a title in a team): trimmed, 1 to [`AGENT_TEXT_MAX_CHARS`]
/// characters, no control characters.
pub(crate) fn check_text(text: Option<&str>, what: &str) -> Result<Option<String>, ApiError> {
    let Some(text) = text else { return Ok(None) };
    let text = text.trim();
    if text.is_empty()
        || text.chars().count() > AGENT_TEXT_MAX_CHARS
        || text.chars().any(char::is_control)
    {
        return Err(fail(
            ErrorCode::InvalidRequest,
            &format!(
                "{what} must be 1 to {AGENT_TEXT_MAX_CHARS} characters with no control characters"
            ),
        ));
    }
    Ok(Some(text.to_owned()))
}

pub(crate) fn refuse_offboarded(member: &Member) -> Result<(), ApiError> {
    if is_offboarded(member) {
        Err(fail(ErrorCode::InvalidRequest, "that agent is offboarded"))
    } else {
        Ok(())
    }
}

/// Handles are compared ignoring case across every agent's `messaging_name`, every person's
/// `name` and the reserved word `owner`; the owner's display name is not a handle. `except` is
/// the member being renamed, which may keep or recapitalize its own.
pub(crate) fn check_handle_free(
    view: &dyn OrgView,
    key: &str,
    except: Option<&str>,
) -> Result<(), ApiError> {
    // The view never returns the owner (its display name is not a handle); the filter keeps a
    // view that forgets that from refusing names the owner merely displays.
    let taken_by_other = view
        .member_with_handle(key)
        .is_some_and(|holder| !holder.owner && Some(holder.id.as_str()) != except);
    if taken_by_other || key == OWNER_HANDLE || key == OFFICE_HANDLE {
        Err(fail(
            ErrorCode::AlreadyExists,
            "that name is already another member's handle",
        ))
    } else {
        Ok(())
    }
}

/// Whether `candidate` is `team_id` itself or a team that `team_id` owns, directly or not.
pub(crate) fn owns_or_is(view: &dyn OrgView, team_id: &str, candidate: &str) -> bool {
    let mut seen = HashSet::new();
    let mut at = Some(candidate.to_owned());
    while let Some(id) = at {
        if id == team_id {
            return true;
        }
        if !seen.insert(id.clone()) {
            return false; // a loop already in the data: do not spin
        }
        at = view.team(&id).and_then(|team| team.owner_team_id);
    }
    false
}

/// Whether following `from`'s reporting line upwards reaches `target`.
pub(crate) fn reports_up_to(view: &dyn OrgView, from: &str, target: &str) -> bool {
    let mut seen = HashSet::new();
    let mut at = Some(from.to_owned());
    while let Some(id) = at {
        if id == target {
            return true;
        }
        if !seen.insert(id.clone()) {
            return false;
        }
        at = view.member(&id).and_then(|m| m.reports_to_member_id);
    }
    false
}
