//! Hire an agent without changing organization membership ownership.
use super::policy::{
    check_home, check_run_mode, check_runtime, parse_messaging_name, required_text,
};
use crate::organization::{
    check_handle_free, check_id, check_member_id, check_text, fail, parse_name, Decision, OrgView,
};
use org_proto::org::{
    AgentProfile, AgentStatus, Member, MemberAddAgentParams, MemberKind, OrgChange,
};
use org_proto::ErrorCode;

pub(crate) fn hire_employee(view: &dyn OrgView, now: &str, p: &MemberAddAgentParams) -> Decision {
    // The office id is a client-made id like the others: a bad one is invalid_request, before
    // the lookup that would say not_found.
    check_id(&p.office_id)?;
    if !view.office_exists(&p.office_id) {
        return Err(fail(ErrorCode::NotFound, "no such office"));
    }
    check_member_id(&p.member_id)?;
    let profile = &p.profile;
    let role = required_text(&profile.role, "role")?;
    let title = required_text(&profile.title, "title")?;
    let focus = check_text(profile.focus.as_deref(), "focus")?;
    let runtime = check_runtime(&profile.runtime)?;
    let model = check_text(profile.model.as_deref(), "model")?;
    let home = profile.home.as_deref().map(check_home).transpose()?;
    if profile.offboarded_at.is_some() {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "offboarded_at is set by member.offboard only",
        ));
    }
    if !matches!(profile.status, AgentStatus::Active | AgentStatus::Paused) {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "a new agent is active or paused; offboarding is member.offboard",
        ));
    }
    let (run_mode, schedule) = check_run_mode(profile.run_mode, profile.schedule.as_ref())?;
    let name = parse_name(&p.name)?;
    let handle = parse_messaging_name(&profile.messaging_name)?;
    if view.member(&p.member_id).is_some() {
        return Err(fail(ErrorCode::AlreadyExists, "that member id is taken"));
    }
    check_handle_free(view, &handle.to_lowercase(), None)?;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            id: p.member_id.clone(),
            kind: MemberKind::Agent,
            name: name.into_string(),
            contact: None,
            owner: false,
            office_id: Some(p.office_id.clone()),
            agent: Some(AgentProfile {
                role,
                title,
                focus,
                runtime,
                model,
                run_mode,
                schedule,
                home,
                messaging_name: handle,
                status: profile.status,
                offboarded_at: None,
            }),
            reports_to_member_id: None,
            created_at: Some(now.to_owned()),
            version: 1,
        },
    }])
}
