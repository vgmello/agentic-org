//! Projects and areas in the mock HQ (design `tasks-projects-docs.md` §1.9, §2.5, §4.8): `project.create`, `project.update`,
//! `project.archive`, `area.create`, `area.rename`, `area.set_team`, `area.set_lead`, and the invariant that an area's team is
//! one of its project's teams (and its lead a member of that team), which `reconcile_areas` restores after any command.
//! `area.delete` waits for work items (it moves open bugs).

use super::{checked_name, err, is_offboarded, member_ix, same, team_ix, Ctx, Outcome};
use org_proto::org::{
    Area, AreaCreateParams, AreaRenameParams, AreaSetLeadParams, AreaSetTeamParams, OrgChange,
    OrgSnapshot, Project, ProjectArchiveParams, ProjectCreateParams, ProjectRepo, ProjectTeam,
    ProjectUpdateParams, RepoHost, RepoSpec, AREA_NAME_MAX_CHARS, PROJECT_DESCRIPTION_MAX_CHARS,
    PROJECT_KEY_MAX_CHARS, RESERVED_PROJECT_KEYS,
};
use org_proto::ErrorCode;

fn project_ix(s: &OrgSnapshot, id: &str) -> Outcome<usize> {
    s.projects.iter().position(|p| p.id == id).ok_or_else(|| {
        org_proto::ApiError::new(ErrorCode::NotFound, "That project isn't there any more.")
    })
}

fn area_ix(s: &OrgSnapshot, id: &str) -> Outcome<usize> {
    s.areas.iter().position(|a| a.id == id).ok_or_else(|| {
        org_proto::ApiError::new(ErrorCode::NotFound, "That area isn't there any more.")
    })
}

fn key_ok(key: &str) -> bool {
    let n = key.chars().count();
    (2..=PROJECT_KEY_MAX_CHARS).contains(&n)
        && key.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn key_taken(s: &OrgSnapshot, key: &str) -> bool {
    RESERVED_PROJECT_KEYS.contains(&key) || s.projects.iter().any(|p| p.key == key)
}

/// A key from the name (design §4.8): its ASCII letters and digits, lowercased, the first 3 (prefixed with `p` if that
/// doesn't start with a letter, padded with `x` to 2); a digit 2, 3, … is appended while it is taken or reserved.
pub fn derive_key(s: &OrgSnapshot, name: &str) -> String {
    let mut base: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .take(3)
        .collect();
    if !base.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        base.insert(0, 'p');
        base.truncate(3);
    }
    while base.chars().count() < 2 {
        base.push('x');
    }
    let mut key = base.clone();
    let mut n = 2;
    while key_taken(s, &key) {
        key = format!("{base}{n}");
        n += 1;
    }
    key
}

/// An area's name: the organization-name rules, and at most `AREA_NAME_MAX_CHARS` characters.
fn checked_area_name(raw: &str) -> Outcome<String> {
    let name = checked_name(raw)?;
    if name.chars().count() > AREA_NAME_MAX_CHARS {
        return err(
            ErrorCode::InvalidName,
            format!("An area's name can be at most {AREA_NAME_MAX_CHARS} characters."),
        );
    }
    Ok(name)
}

fn check_description(text: &str) -> Outcome<()> {
    if text.chars().count() > PROJECT_DESCRIPTION_MAX_CHARS
        || text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        return err(
            ErrorCode::InvalidRequest,
            format!("A description can be at most {PROJECT_DESCRIPTION_MAX_CHARS} characters, without control characters."),
        );
    }
    Ok(())
}

fn check_branch(text: &str) -> Outcome<()> {
    if text.trim().is_empty() || text.chars().count() > 255 || text.chars().any(char::is_control) {
        return err(ErrorCode::InvalidRequest, "That isn't a branch name.");
    }
    Ok(())
}

fn project_name_taken(s: &OrgSnapshot, name: &str, except: Option<&str>) -> bool {
    s.projects
        .iter()
        .any(|p| Some(p.id.as_str()) != except && same(&p.name, name))
}

fn live_area_name_taken(
    s: &OrgSnapshot,
    project_id: &str,
    name: &str,
    except: Option<&str>,
) -> bool {
    s.areas.iter().any(|a| {
        a.project_id == project_id
            && !a.deleted
            && Some(a.id.as_str()) != except
            && same(&a.name, name)
    })
}

fn project_team_ids(s: &OrgSnapshot, project_id: &str) -> Vec<String> {
    s.project_teams
        .iter()
        .filter(|l| l.project_id == project_id)
        .map(|l| l.team_id.clone())
        .collect()
}

pub fn project_create(s: &mut OrgSnapshot, p: &ProjectCreateParams, ctx: &Ctx) -> Outcome {
    for t in &p.teams {
        team_ix(s, &t.team_id)?;
    }
    if let Some(key) = &p.key {
        if !key_ok(key) || RESERVED_PROJECT_KEYS.contains(&key.as_str()) {
            return err(ErrorCode::InvalidRequest, "A key is 2 to 10 lowercase letters and digits, starting with a letter, and not reserved.");
        }
    }
    if let Some(d) = &p.description {
        check_description(d)?;
    }
    if let Some(b) = &p.integration_branch {
        check_branch(b)?;
    }
    let mut seen_teams = Vec::new();
    for t in &p.teams {
        if seen_teams.contains(&t.team_id) {
            return err(ErrorCode::InvalidRequest, "A team can be listed once.");
        }
        seen_teams.push(t.team_id.clone());
    }
    let repo = match &p.repo {
        None => None,
        Some(RepoSpec::NewLocal {}) => Some(ProjectRepo {
            host: RepoHost::Local,
            default_branch: "main".into(),
            path: Some(format!("/workspace/repos/{}.git", p.project_id)),
            github: None,
        }),
        Some(RepoSpec::ExistingLocal { path }) => {
            if !path.starts_with('/') || path.chars().any(char::is_control) {
                return err(ErrorCode::InvalidRequest, "A repository path is absolute.");
            }
            Some(ProjectRepo {
                host: RepoHost::Local,
                default_branch: "main".into(),
                path: Some(path.clone()),
                github: None,
            })
        }
        Some(_) => {
            return err(
                ErrorCode::InvalidRequest,
                "That repository kind isn't supported yet.",
            )
        }
    };
    let name = checked_name(&p.name)?;
    let general_name = checked_area_name(p.general_area_name.as_deref().unwrap_or("General"))?;
    let mut link_ids: Vec<&str> = Vec::new();
    for t in &p.teams {
        if link_ids.contains(&t.project_team_id.as_str()) {
            return err(ErrorCode::AlreadyExists, "A link id is used twice.");
        }
        link_ids.push(&t.project_team_id);
    }
    if s.projects.iter().any(|x| x.id == p.project_id)
        || s.areas.iter().any(|a| a.id == p.general_area_id)
        || p.teams
            .iter()
            .any(|t| s.project_teams.iter().any(|l| l.id == t.project_team_id))
    {
        return err(ErrorCode::AlreadyExists, "One of those ids is taken.");
    }
    if let Some(key) = &p.key {
        if key_taken(s, key) {
            return err(ErrorCode::AlreadyExists, "That key is taken.");
        }
    }
    if project_name_taken(s, &name, None) {
        return err(
            ErrorCode::AlreadyExists,
            format!("There's already a project called {name}."),
        );
    }
    let key = p.key.clone().unwrap_or_else(|| derive_key(s, &name));
    let mut project = Project::new(p.project_id.clone(), name);
    project.key = key;
    project.description = p.description.clone();
    project.repo = repo;
    project.integration_branch = p.integration_branch.clone();
    project.created_at = Some(ctx.now.clone());
    project.version = 1;
    let general = Area {
        id: p.general_area_id.clone(),
        project_id: p.project_id.clone(),
        name: general_name,
        team_id: p.teams.first().map(|t| t.team_id.clone()),
        lead_member_id: None,
        general: true,
        deleted: false,
        replaced_by: None,
        version: 1,
    };
    s.projects.push(project.clone());
    s.areas.push(general.clone());
    let mut changes = vec![
        OrgChange::ProjectUpserted { project },
        OrgChange::AreaUpserted { area: general },
    ];
    for t in &p.teams {
        let project_team = ProjectTeam {
            id: t.project_team_id.clone(),
            project_id: p.project_id.clone(),
            team_id: t.team_id.clone(),
            version: 1,
        };
        s.project_teams.push(project_team.clone());
        changes.push(OrgChange::ProjectTeamAdded { project_team });
    }
    Ok(changes)
}

pub fn project_update(s: &mut OrgSnapshot, p: &ProjectUpdateParams) -> Outcome {
    let i = project_ix(s, &p.project_id)?;
    if p.description.is_some() && p.clear_description
        || p.integration_branch.is_some() && p.clear_integration_branch
    {
        return err(
            ErrorCode::InvalidRequest,
            "Give a value or clear it, not both.",
        );
    }
    if p.name.is_none()
        && p.description.is_none()
        && !p.clear_description
        && p.integration_branch.is_none()
        && !p.clear_integration_branch
    {
        return err(ErrorCode::InvalidRequest, "Nothing to change.");
    }
    if let Some(d) = &p.description {
        check_description(d)?;
    }
    if let Some(b) = &p.integration_branch {
        check_branch(b)?;
    }
    let name = p.name.as_deref().map(checked_name).transpose()?;
    if s.projects[i].version != p.expected_version {
        return err(
            ErrorCode::Conflict,
            format!("{} changed since you looked.", s.projects[i].name),
        );
    }
    if let Some(n) = &name {
        if project_name_taken(s, n, Some(&p.project_id)) {
            return err(
                ErrorCode::AlreadyExists,
                format!("There's already a project called {n}."),
            );
        }
    }
    let project = &mut s.projects[i];
    if let Some(n) = name {
        project.name = n;
    }
    if let Some(d) = &p.description {
        project.description = Some(d.clone());
    }
    if p.clear_description {
        project.description = None;
    }
    if let Some(b) = &p.integration_branch {
        project.integration_branch = Some(b.clone());
    }
    if p.clear_integration_branch {
        project.integration_branch = None;
    }
    project.version += 1;
    Ok(vec![OrgChange::ProjectUpserted {
        project: project.clone(),
    }])
}

pub fn project_archive(s: &mut OrgSnapshot, p: &ProjectArchiveParams) -> Outcome {
    let i = project_ix(s, &p.project_id)?;
    if s.projects[i].version != p.expected_version {
        return err(
            ErrorCode::Conflict,
            format!("{} changed since you looked.", s.projects[i].name),
        );
    }
    s.projects[i].archived = p.archived;
    s.projects[i].version += 1;
    Ok(vec![OrgChange::ProjectUpserted {
        project: s.projects[i].clone(),
    }])
}

/// A lead must be a live, non-owner member of the area's team (shared by `area.create` and `area.set_lead`).
fn check_lead(s: &OrgSnapshot, lead_id: &str, team_id: Option<&str>) -> Outcome<()> {
    let m = &s.members[member_ix(s, lead_id)?];
    if is_offboarded(m) {
        return err(
            ErrorCode::InvalidRequest,
            format!("{} has left the organization.", m.name),
        );
    }
    let Some(team_id) = team_id else {
        return err(
            ErrorCode::InvalidRequest,
            "An area needs a team before it has a lead.",
        );
    };
    if m.owner {
        return err(ErrorCode::OwnerProtected, "The owner can't lead an area.");
    }
    if !s
        .memberships
        .iter()
        .any(|x| x.team_id == team_id && x.member_id == lead_id)
    {
        return err(
            ErrorCode::NotAMember,
            format!("{} isn't in that team.", m.name),
        );
    }
    Ok(())
}

pub fn area_create(s: &mut OrgSnapshot, p: &AreaCreateParams) -> Outcome {
    let pi = project_ix(s, &p.project_id)?;
    if let Some(t) = &p.team_id {
        team_ix(s, t)?;
    }
    if let Some(l) = &p.lead_member_id {
        member_ix(s, l)?;
    }
    let teams = project_team_ids(s, &p.project_id);
    match &p.team_id {
        Some(t) if !teams.contains(t) => {
            return err(
                ErrorCode::InvalidRequest,
                "That team doesn't work on this project.",
            )
        }
        None if !teams.is_empty() => {
            return err(
                ErrorCode::InvalidRequest,
                "Choose which of the project's teams owns this area.",
            )
        }
        _ => {}
    }
    if let Some(l) = &p.lead_member_id {
        let m = &s.members[member_ix(s, l)?];
        if is_offboarded(m) || p.team_id.is_none() {
            check_lead(s, l, p.team_id.as_deref())?;
        }
    }
    let name = checked_area_name(&p.name)?;
    if let Some(l) = &p.lead_member_id {
        check_lead(s, l, p.team_id.as_deref())?;
    }
    if s.areas.iter().any(|a| a.id == p.area_id) {
        return err(ErrorCode::AlreadyExists, "That area id is taken.");
    }
    if live_area_name_taken(s, &p.project_id, &name, None) {
        return err(
            ErrorCode::AlreadyExists,
            format!("{} already has an area called {name}.", s.projects[pi].name),
        );
    }
    let area = Area {
        id: p.area_id.clone(),
        project_id: p.project_id.clone(),
        name,
        team_id: p.team_id.clone(),
        lead_member_id: p.lead_member_id.clone(),
        general: false,
        deleted: false,
        replaced_by: None,
        version: 1,
    };
    s.areas.push(area.clone());
    Ok(vec![OrgChange::AreaUpserted { area }])
}

pub fn area_rename(s: &mut OrgSnapshot, p: &AreaRenameParams) -> Outcome {
    let i = area_ix(s, &p.area_id)?;
    if s.areas[i].deleted {
        return err(ErrorCode::InvalidRequest, "That area was deleted.");
    }
    let name = checked_area_name(&p.name)?;
    if s.areas[i].version != p.expected_version {
        return err(
            ErrorCode::Conflict,
            format!("{} changed since you looked.", s.areas[i].name),
        );
    }
    if live_area_name_taken(s, &s.areas[i].project_id.clone(), &name, Some(&p.area_id)) {
        return err(
            ErrorCode::AlreadyExists,
            format!("There's already an area called {name} in this project."),
        );
    }
    s.areas[i].name = name;
    s.areas[i].version += 1;
    Ok(vec![OrgChange::AreaUpserted {
        area: s.areas[i].clone(),
    }])
}

pub fn area_set_team(s: &mut OrgSnapshot, p: &AreaSetTeamParams) -> Outcome {
    let i = area_ix(s, &p.area_id)?;
    if let Some(t) = &p.team_id {
        team_ix(s, t)?;
    }
    if s.areas[i].deleted {
        return err(ErrorCode::InvalidRequest, "That area was deleted.");
    }
    let teams = project_team_ids(s, &s.areas[i].project_id);
    match &p.team_id {
        Some(t) if !teams.contains(t) => {
            return err(
                ErrorCode::InvalidRequest,
                "That team doesn't work on this project.",
            )
        }
        None if !teams.is_empty() => {
            return err(
                ErrorCode::InvalidRequest,
                "Choose which of the project's teams owns this area.",
            )
        }
        _ => {}
    }
    if s.areas[i].version != p.expected_version {
        return err(
            ErrorCode::Conflict,
            format!("{} changed since you looked.", s.areas[i].name),
        );
    }
    s.areas[i].team_id = p.team_id.clone();
    // A lead who isn't in the new team stops leading.
    if let Some(lead) = s.areas[i].lead_member_id.clone() {
        let stays = p.team_id.as_deref().is_some_and(|t| {
            s.memberships
                .iter()
                .any(|m| m.team_id == t && m.member_id == lead)
        });
        if !stays {
            s.areas[i].lead_member_id = None;
        }
    }
    s.areas[i].version += 1;
    Ok(vec![OrgChange::AreaUpserted {
        area: s.areas[i].clone(),
    }])
}

pub fn area_set_lead(s: &mut OrgSnapshot, p: &AreaSetLeadParams) -> Outcome {
    let i = area_ix(s, &p.area_id)?;
    if let Some(l) = &p.lead_member_id {
        member_ix(s, l)?;
    }
    if s.areas[i].deleted {
        return err(ErrorCode::InvalidRequest, "That area was deleted.");
    }
    if let Some(l) = &p.lead_member_id {
        check_lead(s, l, s.areas[i].team_id.as_deref())?;
    }
    if s.areas[i].version != p.expected_version {
        return err(
            ErrorCode::Conflict,
            format!("{} changed since you looked.", s.areas[i].name),
        );
    }
    s.areas[i].lead_member_id = p.lead_member_id.clone();
    s.areas[i].version += 1;
    Ok(vec![OrgChange::AreaUpserted {
        area: s.areas[i].clone(),
    }])
}

/// An organization saved before projects had areas gets each project's General area (id `<project>-general`, owned
/// by the project's first team, if it has one), so nothing filed under it is lost. Idempotent.
pub fn backfill_general_areas(s: &mut OrgSnapshot) {
    for i in 0..s.projects.len() {
        let id = s.projects[i].id.clone();
        if s.areas.iter().any(|a| a.project_id == id && a.general) {
            continue;
        }
        let team_id = s
            .project_teams
            .iter()
            .find(|l| l.project_id == id)
            .map(|l| l.team_id.clone());
        s.areas.push(Area {
            id: format!("{id}-general"),
            project_id: id,
            name: "General".into(),
            team_id,
            lead_member_id: None,
            general: true,
            deleted: false,
            replaced_by: None,
            version: 1,
        });
    }
}

/// Restore the area invariants after any command (design §2.5): an area's team is one of its project's teams (else the
/// area is team-less), and its lead is a live member of that team (else it has no lead). Each area that changes goes up
/// one version and is reported. Idempotent, so it can run after every command.
pub fn reconcile_areas(s: &mut OrgSnapshot, changes: &mut Vec<OrgChange>) {
    for i in 0..s.areas.len() {
        if s.areas[i].deleted {
            continue;
        }
        let mut changed = false;
        if let Some(team) = s.areas[i].team_id.clone() {
            let works = s.teams.iter().any(|t| t.id == team)
                && s.project_teams
                    .iter()
                    .any(|l| l.project_id == s.areas[i].project_id && l.team_id == team);
            if !works {
                s.areas[i].team_id = None;
                changed = true;
            }
        }
        if let Some(lead) = s.areas[i].lead_member_id.clone() {
            let team = s.areas[i].team_id.clone();
            let live = s.members.iter().any(|m| m.id == lead && !is_offboarded(m));
            let in_team = team.is_some_and(|t| {
                s.memberships
                    .iter()
                    .any(|m| m.team_id == t && m.member_id == lead)
            });
            if !live || !in_team {
                s.areas[i].lead_member_id = None;
                changed = true;
            }
        }
        if changed {
            s.areas[i].version += 1;
            changes.push(OrgChange::AreaUpserted {
                area: s.areas[i].clone(),
            });
        }
    }
}
