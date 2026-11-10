//! What HQ checks about an agent's profile (`org_proto::org::AgentProfile` and the agent commands): the limits
//! and shapes the contract names. Pure functions; the mock HQ in `org_engine` uses them. All of these refuse with
//! `invalid_request` except a messaging name that breaks its format, which is `invalid_name`.

use org_proto::org::{AgentProfile, AgentRuntime, AgentStatus, RunMode, Schedule};
use org_proto::{
    ApiError, ErrorCode, AGENT_HOME_MAX_BYTES, AGENT_MESSAGING_NAME_MAX_CHARS,
    AGENT_TEXT_MAX_CHARS, AGENT_TIME_ZONE_MAX_CHARS,
};

pub type Check<T = ()> = Result<T, ApiError>;

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, message)
}

/// Text as HQ stores it: trimmed, 1 to 200 characters, no control characters. `what` words the refusal.
pub fn text(raw: &str, what: &str) -> Check<String> {
    let t = raw.trim();
    if t.is_empty() {
        return Err(invalid(format!("{what} can't be empty.")));
    }
    if t.chars().count() > AGENT_TEXT_MAX_CHARS {
        return Err(invalid(format!(
            "{what} can be at most {AGENT_TEXT_MAX_CHARS} characters."
        )));
    }
    if t.chars().any(char::is_control) {
        return Err(invalid(format!("{what} can't contain control characters.")));
    }
    Ok(t.to_string())
}

/// A messaging name: 1 to 64 ASCII letters, digits, `-`, `_` and `.`, starting with a letter or digit.
pub fn messaging_name(raw: &str) -> Check<String> {
    let ok = !raw.is_empty()
        && raw.chars().count() <= AGENT_MESSAGING_NAME_MAX_CHARS
        && raw
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(raw.to_string())
    } else {
        Err(ApiError::new(
            ErrorCode::InvalidName,
            "A messaging name is 1 to 64 letters, digits, - _ or ., starting with a letter or digit.",
        ))
    }
}

/// A runtime HQ accepts: one of the known ones, or a named other (1 to 200 characters) that isn't a known one's name.
pub fn runtime(r: &AgentRuntime) -> Check<AgentRuntime> {
    match r {
        AgentRuntime::Unknown => Err(invalid("Choose what the agent runs on.")),
        AgentRuntime::Other(name) => {
            let name = text(name, "The runtime's name")?;
            if matches!(name.as_str(), "claude_code" | "codex" | "antigravity") {
                return Err(invalid("Use the known runtime, not a copy of its name."));
            }
            Ok(AgentRuntime::Other(name))
        }
        known => Ok(known.clone()),
    }
}

/// An absolute path, at most 1024 bytes, no control characters, no `..` component.
pub fn home(raw: &str) -> Check<String> {
    if raw.is_empty()
        || raw.len() > AGENT_HOME_MAX_BYTES
        || raw.chars().any(char::is_control)
        || !raw.starts_with('/')
        || raw.split('/').any(|part| part == "..")
    {
        return Err(invalid(
            "The home folder must be an absolute path (no .. parts, at most 1024 bytes).",
        ));
    }
    Ok(raw.to_string())
}

/// A time zone name like `Europe/Lisbon`: `/`-separated names of letters, digits, `_`, `+`, `-`, none empty, `.` or `..`.
fn time_zone(raw: &str) -> Check<String> {
    let ok = !raw.is_empty()
        && raw.chars().count() <= AGENT_TIME_ZONE_MAX_CHARS
        && raw.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
        });
    if ok {
        Ok(raw.to_string())
    } else {
        Err(invalid("The time zone must be a name like Europe/Lisbon."))
    }
}

/// A schedule as HQ stores it: the spec trimmed (1 to 200 characters) and a valid time zone.
pub fn schedule(s: &Schedule) -> Check<Schedule> {
    Ok(Schedule {
        spec: text(&s.spec, "The schedule")?,
        time_zone: time_zone(&s.time_zone)?,
    })
}

/// A run mode with its schedule: `scheduled` needs a schedule, any other mode must have none, `Unknown` is refused.
pub fn run_mode(
    mode: Option<RunMode>,
    sched: Option<&Schedule>,
) -> Check<(Option<RunMode>, Option<Schedule>)> {
    match mode {
        Some(RunMode::Unknown) => Err(invalid("Choose how the agent is run.")),
        Some(RunMode::Scheduled) => match sched {
            Some(s) => Ok((mode, Some(schedule(s)?))),
            None => Err(invalid("A scheduled agent needs a schedule.")),
        },
        other => match sched {
            Some(_) => Err(invalid("Only a scheduled agent has a schedule.")),
            None => Ok((other, None)),
        },
    }
}

/// A whole profile as HQ stores it when an agent is added, or an `invalid_request`/`invalid_name` naming the
/// first thing wrong. The status must be `active` or `paused` (an agent is offboarded with its own command),
/// and `offboarded_at` must be absent. The messaging name is checked last: it is the one `invalid_name`.
pub fn new_profile(p: &AgentProfile) -> Check<AgentProfile> {
    let role = text(&p.role, "The role")?;
    let title = text(&p.title, "The title")?;
    let focus = p
        .focus
        .as_deref()
        .map(|f| text(f, "The focus"))
        .transpose()?;
    let model = p
        .model
        .as_deref()
        .map(|m| text(m, "The model"))
        .transpose()?;
    let rt = runtime(&p.runtime)?;
    let home = p.home.as_deref().map(home).transpose()?;
    if !matches!(p.status, AgentStatus::Active | AgentStatus::Paused) {
        return Err(invalid("A new agent is active or paused."));
    }
    if p.offboarded_at.is_some() {
        return Err(invalid("A new agent hasn't been offboarded."));
    }
    let (run_mode, schedule) = run_mode(p.run_mode, p.schedule.as_ref())?;
    let messaging_name = messaging_name(&p.messaging_name)?;
    Ok(AgentProfile {
        role,
        title,
        focus,
        runtime: rt,
        model,
        run_mode,
        schedule,
        home,
        messaging_name,
        status: p.status,
        offboarded_at: None,
    })
}
