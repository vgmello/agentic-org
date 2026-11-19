//! Employee status, offboarding and run-mode transitions.
use super::policy::{check_run_mode, live_agent};
use crate::organization::{check_version, ending_relations, fail, Decision, OrgView};
use org_proto::org::{
    AgentStatus, Member, MemberOffboardParams, MemberSetAgentStatusParams, MemberSetRunModeParams,
    OrgChange,
};
use org_proto::ErrorCode;

pub(crate) fn member_set_agent_status(
    view: &dyn OrgView,
    p: &MemberSetAgentStatusParams,
) -> Decision {
    let (member, mut profile) = live_agent(view, &p.member_id)?;
    if !matches!(p.status, AgentStatus::Active | AgentStatus::Paused) {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "an agent is active or paused; offboarding is member.offboard",
        ));
    }
    check_version(member.version, p.expected_version)?;
    profile.status = p.status;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            agent: Some(profile),
            version: member.version + 1,
            ..member
        },
    }])
}

pub(crate) fn member_offboard(view: &dyn OrgView, now: &str, p: &MemberOffboardParams) -> Decision {
    let (member, mut profile) = live_agent(view, &p.member_id)?;
    check_version(member.version, p.expected_version)?;
    // Its relations end as for a removal, except that an already offboarded agent's record is
    // history and keeps who it reported to; the record and its handle stay, as history.
    let mut changes = ending_relations(view, &member, false);
    profile.status = AgentStatus::Offboarded;
    profile.offboarded_at = Some(now.to_owned());
    changes.push(OrgChange::MemberUpserted {
        member: Member {
            agent: Some(profile),
            version: member.version + 1,
            ..member
        },
    });
    Ok(changes)
}

pub(crate) fn member_set_run_mode(view: &dyn OrgView, p: &MemberSetRunModeParams) -> Decision {
    let (member, mut profile) = live_agent(view, &p.member_id)?;
    let (run_mode, schedule) = check_run_mode(p.run_mode, p.schedule.as_ref())?;
    check_version(member.version, p.expected_version)?;
    profile.run_mode = run_mode;
    profile.schedule = schedule;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            agent: Some(profile),
            version: member.version + 1,
            ..member
        },
    }])
}
