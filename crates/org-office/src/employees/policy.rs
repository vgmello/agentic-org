//! Employee profile/runtime/schedule validation, using organization-owned member checks.
use crate::organization::{check_text, fail, found, refuse_offboarded, OrgView};
use org_proto::org::{
    AgentProfile, AgentRuntime, Member, MemberKind, RunMode, Schedule, AGENT_HOME_MAX_BYTES,
    AGENT_MESSAGING_NAME_MAX_CHARS, AGENT_TIME_ZONE_MAX_CHARS,
};
use org_proto::{ApiError, ErrorCode};

/// The agent `member_id` names, which must be one that is still a member: a person, or an
/// offboarded agent (whose record is history), is `invalid_request`.
pub(super) fn live_agent(
    view: &dyn OrgView,
    member_id: &str,
) -> Result<(Member, AgentProfile), ApiError> {
    let member = found(view.member(member_id), "no such member")?;
    let Some(profile) = member
        .agent
        .clone()
        .filter(|_| member.kind == MemberKind::Agent)
    else {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "that member is a person; agents only here",
        ));
    };
    refuse_offboarded(&member)?;
    Ok((member, profile))
}

/// A required profile text (`role`, `title`): trimmed, 1 to
/// [`org_proto::org::AGENT_TEXT_MAX_CHARS`] characters,
/// no control characters.
pub(super) fn required_text(raw: &str, what: &str) -> Result<String, ApiError> {
    Ok(check_text(Some(raw), what)?.expect("a given text is returned"))
}

/// A runtime a writer may send: a named one, or `Other` with a name that is not a named runtime
/// in another spelling (ignoring case, with `-`, `_` and spaces alike). `Unknown` is refused.
pub(super) fn check_runtime(runtime: &AgentRuntime) -> Result<AgentRuntime, ApiError> {
    match runtime {
        AgentRuntime::Unknown => Err(fail(
            ErrorCode::InvalidRequest,
            "the runtime must be one this office knows",
        )),
        AgentRuntime::Other(name) => {
            let name = required_text(name, "runtime")?;
            let spelled: String = name
                .chars()
                .map(|c| if c == '-' || c == ' ' { '_' } else { c })
                .collect::<String>()
                .to_lowercase();
            if matches!(spelled.as_str(), "claude_code" | "codex" | "antigravity") {
                Err(fail(
                    ErrorCode::InvalidRequest,
                    "that runtime has a name of its own; use it",
                ))
            } else {
                Ok(AgentRuntime::Other(name))
            }
        }
        known => Ok(known.clone()),
    }
}

/// A home: an absolute path of at most [`AGENT_HOME_MAX_BYTES`] bytes with no control character
/// and no `..` component. It is stored as given and never opened by the rules.
pub(super) fn check_home(home: &str) -> Result<String, ApiError> {
    if !home.starts_with('/')
        || home.len() > AGENT_HOME_MAX_BYTES
        || home.chars().any(char::is_control)
        || home.split('/').any(|part| part == "..")
    {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "home must be an absolute path with no `..`, at most 1024 bytes",
        ));
    }
    Ok(home.to_owned())
}

/// The run mode and schedule together: `Unknown` is refused, `scheduled` needs a schedule and
/// any other mode (or none) must have none.
pub(super) fn check_run_mode(
    run_mode: Option<RunMode>,
    schedule: Option<&Schedule>,
) -> Result<(Option<RunMode>, Option<Schedule>), ApiError> {
    match (run_mode, schedule) {
        (Some(RunMode::Unknown), _) => Err(fail(
            ErrorCode::InvalidRequest,
            "the run mode must be one this office knows",
        )),
        (Some(RunMode::Scheduled), Some(schedule)) => {
            Ok((Some(RunMode::Scheduled), Some(check_schedule(schedule)?)))
        }
        (Some(RunMode::Scheduled), None) => Err(fail(
            ErrorCode::InvalidRequest,
            "a scheduled agent needs a schedule",
        )),
        (mode, None) => Ok((mode, None)),
        (_, Some(_)) => Err(fail(
            ErrorCode::InvalidRequest,
            "only a scheduled agent has a schedule",
        )),
    }
}

/// A schedule: the spec is text (trimmed, 1 to [`org_proto::org::AGENT_TEXT_MAX_CHARS`], no control characters)
/// and the time zone is `/`-separated names of ASCII letters, digits, `_`, `+` and `-`, so it
/// can never name a path.
pub(super) fn check_schedule(schedule: &Schedule) -> Result<Schedule, ApiError> {
    let spec = required_text(&schedule.spec, "schedule.spec")?;
    let zone = schedule.time_zone.as_str();
    let zone_ok = !zone.is_empty()
        && zone.len() <= AGENT_TIME_ZONE_MAX_CHARS
        && zone.split('/').all(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
        });
    if !zone_ok {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "schedule.time_zone must be an IANA name like Europe/Lisbon",
        ));
    }
    Ok(Schedule {
        spec,
        time_zone: zone.to_owned(),
    })
}

/// An agent's `messaging_name`, its handle: 1 to [`AGENT_MESSAGING_NAME_MAX_CHARS`] ASCII
/// letters, digits, `-`, `_` and `.`, starting with a letter or digit, and not shaped like an
/// address (a member id, `run:`). Not trimmed: a name that breaks the format is `invalid_name`.
pub(super) fn parse_messaging_name(raw: &str) -> Result<String, ApiError> {
    let bytes = raw.as_bytes();
    let format_ok = !bytes.is_empty()
        && bytes.len() <= AGENT_MESSAGING_NAME_MAX_CHARS
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !format_ok {
        return Err(ApiError::new(
            ErrorCode::InvalidName,
            "a handle is 1 to 64 letters, digits, `-`, `_` and `.`, starting with a letter or digit",
        ));
    }
    if org_proto::is_shaped_like_an_address(raw) {
        return Err(ApiError::new(
            ErrorCode::InvalidName,
            "a handle can't look like a member id",
        ));
    }
    Ok(raw.to_owned())
}
