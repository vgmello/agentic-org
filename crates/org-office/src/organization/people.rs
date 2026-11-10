//! Member records, people, reporting and ending organization relations.
use super::policy::{
    check_handle_free, check_member_id, check_text, check_version, fail, found, is_offboarded,
    owner_protected, parse_handle, parse_name, refuse_offboarded, reports_up_to,
};
use super::{Decision, OrgView};
use org_proto::org::{
    Member, MemberAddPersonParams, MemberKind, MemberRemoveParams, MemberSetReportsToParams,
    MemberUpdatePersonParams, OrgChange, Team,
};
use org_proto::{ErrorCode, OrganizationName};

/// The first entry of a new organization's log: its owner, a person who is in no team.
/// `member_id` is made by the office; the app sets the real name later with
/// `member.update_person`.
pub fn owner_bootstrap(member_id: &str, name: &str, now: &str) -> Decision {
    check_member_id(member_id)?;
    let name = parse_name(name)?;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            id: member_id.to_owned(),
            kind: MemberKind::Person,
            name: name.into_string(),
            contact: None,
            owner: true,
            office_id: None,
            agent: None,
            reports_to_member_id: None,
            created_at: Some(now.to_owned()),
            version: 1,
        },
    }])
}

pub(crate) fn member_add_person(
    view: &dyn OrgView,
    now: &str,
    p: &MemberAddPersonParams,
) -> Decision {
    check_member_id(&p.member_id)?;
    let contact = check_text(p.contact.as_deref(), "contact")?;
    let name = parse_handle(&p.name)?;
    if view.member(&p.member_id).is_some() {
        return Err(fail(ErrorCode::AlreadyExists, "that member id is taken"));
    }
    check_handle_free(view, name.key(), None)?;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            id: p.member_id.clone(),
            kind: MemberKind::Person,
            name: name.into_string(),
            contact,
            owner: false,
            office_id: None,
            agent: None,
            reports_to_member_id: None,
            created_at: Some(now.to_owned()),
            version: 1,
        },
    }])
}

pub(crate) fn member_update_person(view: &dyn OrgView, p: &MemberUpdatePersonParams) -> Decision {
    let member = found(view.member(&p.member_id), "no such member")?;
    if member.kind != MemberKind::Person {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "only people are edited here; agents are edited with member.update_agent",
        ));
    }
    if p.contact.is_some() && p.clear_contact {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "give a contact or clear it, not both",
        ));
    }
    let contact = check_text(p.contact.as_deref(), "contact")?;
    // The owner's display name is not a handle: it never clashes and has no address shape to avoid.
    let name = match (p.name.as_deref(), member.owner) {
        (Some(raw), true) => Some(parse_name(raw)?),
        (Some(raw), false) => Some(parse_handle(raw)?),
        (None, _) => None,
    };
    check_version(member.version, p.expected_version)?;
    if let Some(name) = name.as_ref().filter(|_| !member.owner) {
        check_handle_free(view, name.key(), Some(&member.id))?;
    }
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            name: name.map_or(member.name.clone(), OrganizationName::into_string),
            contact: if p.clear_contact {
                None
            } else {
                contact.or(member.contact.clone())
            },
            version: member.version + 1,
            ..member
        },
    }])
}

pub(crate) fn member_set_reports_to(view: &dyn OrgView, p: &MemberSetReportsToParams) -> Decision {
    let member = found(view.member(&p.member_id), "no such member")?;
    let manager = match &p.reports_to_member_id {
        Some(id) => Some(found(view.member(id), "no such manager")?),
        None => None,
    };
    refuse_offboarded(&member)?;
    if let Some(manager) = &manager {
        refuse_offboarded(manager)?;
    }
    if member.owner {
        return Err(owner_protected("the owner reports to no one"));
    }
    // Reporting to the owner is stored as reporting to no one, so each line has one form.
    let manager = manager.filter(|m| !m.owner);
    if let Some(manager) = &manager {
        if reports_up_to(view, &manager.id, &member.id) {
            return Err(fail(
                ErrorCode::Cycle,
                "someone would end up reporting to themselves",
            ));
        }
    }
    check_version(member.version, p.expected_version)?;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            reports_to_member_id: manager.map(|m| m.id),
            version: member.version + 1,
            ..member
        },
    }])
}

pub(crate) fn member_remove(view: &dyn OrgView, p: &MemberRemoveParams) -> Decision {
    let member = found(view.member(&p.member_id), "no such member")?;
    refuse_offboarded(&member)?;
    if member.owner {
        return Err(owner_protected("the owner cannot be removed"));
    }
    check_version(member.version, p.expected_version)?;
    let mut changes = ending_relations(view, &member, true);
    changes.push(OrgChange::MemberRemoved {
        member_id: member.id,
    });
    Ok(changes)
}

/// What ends when a member leaves (removed, or later offboarded): its memberships, the teams
/// it led (they lose the lead), and its direct reports (they now report to its own manager, or
/// to the owner if it had none). Each affected team and member gets a new version.
pub(crate) fn ending_relations(
    view: &dyn OrgView,
    member: &Member,
    repoint_offboarded: bool,
) -> Vec<OrgChange> {
    let mut changes: Vec<OrgChange> = view
        .memberships_of_member(&member.id)
        .into_iter()
        .map(|m| OrgChange::MembershipRemoved {
            membership_id: m.id,
        })
        .collect();
    changes.extend(
        view.teams_led_by(&member.id)
            .into_iter()
            .map(|team| OrgChange::TeamUpserted {
                team: Team {
                    lead_member_id: None,
                    version: team.version + 1,
                    ..team
                },
            }),
    );
    changes.extend(
        view.reports_of(&member.id)
            .into_iter()
            // A record that is history (an offboarded agent) is only re-pointed when its
            // manager is deleted outright, never when the manager is offboarded.
            .filter(|report| repoint_offboarded || !is_offboarded(report))
            .map(|report| OrgChange::MemberUpserted {
                member: Member {
                    reports_to_member_id: member.reports_to_member_id.clone(),
                    version: report.version + 1,
                    ..report
                },
            }),
    );
    changes
}
