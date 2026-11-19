//! Edit an employee profile through organization-owned identity checks.
use super::policy::{check_home, check_runtime, live_agent, parse_messaging_name, required_text};
use crate::organization::{
    check_handle_free, check_text, check_version, fail, parse_name, Decision, OrgView,
};
use org_proto::org::{Member, MemberUpdateAgentParams, OrgChange};
use org_proto::{ErrorCode, OrganizationName};

pub(crate) fn member_update_agent(view: &dyn OrgView, p: &MemberUpdateAgentParams) -> Decision {
    let (member, mut profile) = live_agent(view, &p.member_id)?;
    let gives_nothing = p.name.is_none()
        && p.role.is_none()
        && p.title.is_none()
        && p.focus.is_none()
        && !p.clear_focus
        && p.runtime.is_none()
        && p.model.is_none()
        && !p.clear_model
        && p.home.is_none()
        && !p.clear_home
        && p.messaging_name.is_none();
    if gives_nothing {
        return Err(fail(ErrorCode::InvalidRequest, "give a field to change"));
    }
    for (given, clear, what) in [
        (p.focus.is_some(), p.clear_focus, "focus"),
        (p.model.is_some(), p.clear_model, "model"),
        (p.home.is_some(), p.clear_home, "home"),
    ] {
        if given && clear {
            return Err(fail(
                ErrorCode::InvalidRequest,
                &format!("give {what} or clear it, not both"),
            ));
        }
    }
    if let Some(role) = &p.role {
        profile.role = required_text(role, "role")?;
    }
    if let Some(title) = &p.title {
        profile.title = required_text(title, "title")?;
    }
    if p.clear_focus {
        profile.focus = None;
    } else if p.focus.is_some() {
        profile.focus = check_text(p.focus.as_deref(), "focus")?;
    }
    if let Some(runtime) = &p.runtime {
        profile.runtime = check_runtime(runtime)?;
    }
    if p.clear_model {
        profile.model = None;
    } else if p.model.is_some() {
        profile.model = check_text(p.model.as_deref(), "model")?;
    }
    if p.clear_home {
        profile.home = None;
    } else if let Some(home) = &p.home {
        profile.home = Some(check_home(home)?);
    }
    let name = p.name.as_deref().map(parse_name).transpose()?;
    let handle = p
        .messaging_name
        .as_deref()
        .map(parse_messaging_name)
        .transpose()?;
    check_version(member.version, p.expected_version)?;
    if let Some(handle) = &handle {
        check_handle_free(view, &handle.to_lowercase(), Some(&member.id))?;
        profile.messaging_name = handle.clone();
    }
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            name: name.map_or(member.name.clone(), OrganizationName::into_string),
            agent: Some(profile),
            version: member.version + 1,
            ..member
        },
    }])
}
