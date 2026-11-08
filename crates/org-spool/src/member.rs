//! Member names with a meaning (ADR-0003 §2.6): `owner` names the organization's owner in
//! `--to`, `--from`, `--as` and `$ORG_AGENT`, and `owner` / `office` are reserved, so no
//! member can be registered under them.

use crate::envelope::validate_agent_id;
use crate::meta::OWNER_ID;
use crate::spool::SpoolError;

/// Names no member can have (any case): `owner` is the alias of the owner, `office` is kept
/// for the office itself.
const RESERVED_NAMES: [&str; 2] = ["owner", "office"];

/// Whether `raw` (trimmed, any case) is a reserved name.
pub fn is_reserved_name(raw: &str) -> bool {
    let raw = raw.trim();
    RESERVED_NAMES.iter().any(|r| raw.eq_ignore_ascii_case(r))
}

/// Refuse a reserved name where a member is named or created (`msg register`, `profile set`).
pub fn check_not_reserved(raw: &str, what: &str) -> Result<(), SpoolError> {
    if is_reserved_name(raw) {
        return Err(SpoolError::InvalidInput(format!(
            "{what} '{}' is reserved (owner, office)",
            raw.trim()
        )));
    }
    Ok(())
}

/// The owner's id in canonical case (`VGMELLO` is `vgmello`: a case-insensitive file system
/// would give both the same inbox); any other id as is.
pub fn canonical_member_id(raw: &str) -> String {
    let raw = raw.trim();
    if raw.eq_ignore_ascii_case(OWNER_ID) {
        OWNER_ID.to_string()
    } else {
        raw.to_string()
    }
}

/// A member named in `--to`, `--from`, `--as` or `$ORG_AGENT`: `owner` (any case) is the
/// owner; the owner's id in any case is canonical; anything else as is.
pub fn resolve_member(raw: &str) -> String {
    if raw.trim().eq_ignore_ascii_case("owner") {
        OWNER_ID.to_string()
    } else {
        canonical_member_id(raw)
    }
}

/// Whether `id` is the owner, ignoring ASCII case.
pub fn is_owner(id: &str) -> bool {
    id.trim().eq_ignore_ascii_case(OWNER_ID)
}

/// Who is calling: `--as` if given, else `$ORG_AGENT`, each resolved like any member name
/// (`owner` is the owner). Acting as the owner with `--as` from another member's session
/// (`$ORG_AGENT` set to someone else) is refused: Phase A has no authentication, so this only
/// stops mistakes. A reserved name is never an identity.
pub fn resolve_caller(as_flag: Option<&str>, env_agent: Option<&str>) -> Result<String, String> {
    let given = |v: Option<&str>| {
        v.map(str::trim)
            .filter(|s| !s.is_empty())
            .map(resolve_member)
    };
    let session = given(env_agent);
    let caller = match given(as_flag) {
        Some(as_member) => {
            if let Some(session) = session.as_deref().filter(|s| !is_owner(s)) {
                if is_owner(&as_member) {
                    return Err(format!("can't act as the owner from {session}'s session"));
                }
            }
            as_member
        }
        None => session.ok_or_else(|| {
            "identity not set: pass --as <agent-id> or set $ORG_AGENT".to_string()
        })?,
    };
    validate_agent_id(&caller, "identity")?;
    if is_reserved_name(&caller) {
        return Err(format!("identity '{caller}' is reserved (owner, office)"));
    }
    Ok(caller)
}
