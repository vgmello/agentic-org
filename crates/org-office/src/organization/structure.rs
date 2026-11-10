//! Teams, membership placement and project/team links.
use super::policy::{
    check_id, check_text, check_version, fail, found, name_key, owner_protected, owns_or_is,
    parse_name, refuse_offboarded, sibling_clash,
};
use super::{Decision, OrgView};
use org_proto::org::{
    MemberMoveParams, Membership, OrgChange, ProjectAddTeamParams, ProjectRemoveTeamParams,
    ProjectTeam, Team, TeamAddMemberParams, TeamCreateParams, TeamDeleteParams, TeamMoveParams,
    TeamRemoveMemberParams, TeamRenameParams, TeamSetLeadParams, TeamSetTitleParams,
};
use org_proto::ErrorCode;

pub(crate) fn team_create(view: &dyn OrgView, p: &TeamCreateParams) -> Decision {
    if let Some(owner) = &p.owner_team_id {
        found(view.team(owner), "no such owner team")?;
    }
    check_id(&p.team_id)?;
    let name = parse_name(&p.name)?;
    if view.team(&p.team_id).is_some() {
        return Err(fail(ErrorCode::AlreadyExists, "that team id is taken"));
    }
    if view
        .team_named(p.owner_team_id.as_deref(), name.key())
        .is_some()
    {
        return Err(sibling_clash());
    }
    Ok(vec![OrgChange::TeamUpserted {
        team: Team {
            id: p.team_id.clone(),
            name: name.into_string(),
            owner_team_id: p.owner_team_id.clone(),
            lead_member_id: None,
            version: 1,
        },
    }])
}

pub(crate) fn team_rename(view: &dyn OrgView, p: &TeamRenameParams) -> Decision {
    let team = found(view.team(&p.team_id), "no such team")?;
    let name = parse_name(&p.name)?;
    check_version(team.version, p.expected_version)?;
    if view
        .team_named(team.owner_team_id.as_deref(), name.key())
        .is_some_and(|other| other.id != team.id)
    {
        return Err(sibling_clash());
    }
    Ok(vec![OrgChange::TeamUpserted {
        team: Team {
            name: name.into_string(),
            version: team.version + 1,
            ..team
        },
    }])
}

pub(crate) fn team_move(view: &dyn OrgView, p: &TeamMoveParams) -> Decision {
    let team = found(view.team(&p.team_id), "no such team")?;
    if let Some(owner) = &p.owner_team_id {
        found(view.team(owner), "no such owner team")?;
        if owns_or_is(view, &team.id, owner) {
            return Err(fail(
                ErrorCode::Cycle,
                "a team cannot be moved under itself or a team it owns",
            ));
        }
    }
    check_version(team.version, p.expected_version)?;
    if view
        .team_named(p.owner_team_id.as_deref(), &name_key(&team.name))
        .is_some_and(|other| other.id != team.id)
    {
        return Err(sibling_clash());
    }
    Ok(vec![OrgChange::TeamUpserted {
        team: Team {
            owner_team_id: p.owner_team_id.clone(),
            version: team.version + 1,
            ..team
        },
    }])
}

pub(crate) fn team_set_lead(view: &dyn OrgView, p: &TeamSetLeadParams) -> Decision {
    let team = found(view.team(&p.team_id), "no such team")?;
    if let Some(lead_id) = &p.lead_member_id {
        let lead = found(view.member(lead_id), "no such member")?;
        refuse_offboarded(&lead)?;
        if lead.owner {
            return Err(owner_protected("the owner cannot lead a team"));
        }
        if view.membership_of(&team.id, &lead.id).is_none() {
            return Err(fail(
                ErrorCode::NotAMember,
                "a team's lead must be a member of the team",
            ));
        }
    }
    check_version(team.version, p.expected_version)?;
    Ok(vec![OrgChange::TeamUpserted {
        team: Team {
            lead_member_id: p.lead_member_id.clone(),
            version: team.version + 1,
            ..team
        },
    }])
}

pub(crate) fn team_delete(view: &dyn OrgView, p: &TeamDeleteParams) -> Decision {
    let team = found(view.team(&p.team_id), "no such team")?;
    if !view.teams_owned_by(&team.id).is_empty() {
        return Err(fail(
            ErrorCode::NotEmpty,
            "the team owns other teams; move or delete them first",
        ));
    }
    check_version(team.version, p.expected_version)?;
    let mut changes: Vec<OrgChange> = view
        .memberships_of_team(&team.id)
        .into_iter()
        .map(|m| OrgChange::MembershipRemoved {
            membership_id: m.id,
        })
        .collect();
    changes.extend(
        view.project_teams_of_team(&team.id)
            .into_iter()
            .map(|link| OrgChange::ProjectTeamRemoved {
                project_team_id: link.id,
            }),
    );
    changes.push(OrgChange::TeamDeleted { team_id: team.id });
    Ok(changes)
}

pub(crate) fn team_add_member(view: &dyn OrgView, p: &TeamAddMemberParams) -> Decision {
    found(view.team(&p.team_id), "no such team")?;
    let member = found(view.member(&p.member_id), "no such member")?;
    check_id(&p.membership_id)?;
    refuse_offboarded(&member)?;
    let title = check_text(p.title_in_team.as_deref(), "title_in_team")?;
    if member.owner {
        return Err(owner_protected("the owner is not a member of any team"));
    }
    if view.membership(&p.membership_id).is_some() {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "that membership id is taken",
        ));
    }
    if view.membership_of(&p.team_id, &p.member_id).is_some() {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "the member is already in that team",
        ));
    }
    Ok(vec![OrgChange::MembershipUpserted {
        membership: Membership {
            id: p.membership_id.clone(),
            team_id: p.team_id.clone(),
            member_id: p.member_id.clone(),
            title_in_team: title,
            version: 1,
        },
    }])
}

pub(crate) fn team_remove_member(view: &dyn OrgView, p: &TeamRemoveMemberParams) -> Decision {
    let membership = found(view.membership(&p.membership_id), "no such membership")?;
    check_version(membership.version, p.expected_version)?;
    Ok(leaving(view, &membership))
}

/// Ending `membership`; leaving a team ends leading it, in the same event.
pub(crate) fn leaving(view: &dyn OrgView, membership: &Membership) -> Vec<OrgChange> {
    let mut changes = vec![OrgChange::MembershipRemoved {
        membership_id: membership.id.clone(),
    }];
    if let Some(team) = view.team(&membership.team_id) {
        if team.lead_member_id.as_deref() == Some(membership.member_id.as_str()) {
            changes.push(OrgChange::TeamUpserted {
                team: Team {
                    lead_member_id: None,
                    version: team.version + 1,
                    ..team
                },
            });
        }
    }
    changes
}

pub(crate) fn member_move(view: &dyn OrgView, p: &MemberMoveParams) -> Decision {
    let membership = found(view.membership(&p.membership_id), "no such membership")?;
    found(view.team(&p.to_team_id), "no such team")?;
    check_id(&p.new_membership_id)?;
    let title = check_text(p.title_in_team.as_deref(), "title_in_team")?;
    check_version(membership.version, p.expected_version)?;
    if view.membership(&p.new_membership_id).is_some() {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "that membership id is taken",
        ));
    }
    // This also refuses a move to the team the member is already in.
    if view
        .membership_of(&p.to_team_id, &membership.member_id)
        .is_some()
    {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "the member is already in that team",
        ));
    }
    let mut changes = leaving(view, &membership);
    changes.insert(
        1,
        OrgChange::MembershipUpserted {
            membership: Membership {
                id: p.new_membership_id.clone(),
                team_id: p.to_team_id.clone(),
                member_id: membership.member_id,
                title_in_team: title,
                version: 1,
            },
        },
    );
    Ok(changes)
}

pub(crate) fn team_set_title(view: &dyn OrgView, p: &TeamSetTitleParams) -> Decision {
    let membership = found(view.membership(&p.membership_id), "no such membership")?;
    let title = check_text(p.title_in_team.as_deref(), "title_in_team")?;
    check_version(membership.version, p.expected_version)?;
    Ok(vec![OrgChange::MembershipUpserted {
        membership: Membership {
            title_in_team: title,
            version: membership.version + 1,
            ..membership
        },
    }])
}

pub(crate) fn project_add_team(view: &dyn OrgView, p: &ProjectAddTeamParams) -> Decision {
    if !view.project_exists(&p.project_id) {
        return Err(fail(ErrorCode::NotFound, "no such project"));
    }
    found(view.team(&p.team_id), "no such team")?;
    check_id(&p.project_team_id)?;
    if view.project_team(&p.project_team_id).is_some() {
        return Err(fail(ErrorCode::AlreadyExists, "that link id is taken"));
    }
    if view.project_team_of(&p.project_id, &p.team_id).is_some() {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "that team already works on the project",
        ));
    }
    Ok(vec![OrgChange::ProjectTeamAdded {
        project_team: ProjectTeam {
            id: p.project_team_id.clone(),
            project_id: p.project_id.clone(),
            team_id: p.team_id.clone(),
            version: 1,
        },
    }])
}

pub(crate) fn project_remove_team(view: &dyn OrgView, p: &ProjectRemoveTeamParams) -> Decision {
    let link = found(view.project_team(&p.project_team_id), "no such link")?;
    check_version(link.version, p.expected_version)?;
    Ok(vec![OrgChange::ProjectTeamRemoved {
        project_team_id: link.id,
    }])
}
