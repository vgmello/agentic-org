//! Resolving the organization's effective owner employee ID from a consistent organization read.
//!
//! On HQ, an office's configured `owner` is already the owner's employee UUIDv7 (including after
//! ownership handover via `Office::open_as_owner`). On a replicated satellite office, HQ sends its
//! authoritative `owner_member_id` on every mTLS `Hello` handshake (persisted durably in
//! `hq-owner.v1.json` and passed into ops via `Office::effective_owner_for_ops`). When a pre-upgrade
//! satellite has not yet received an explicit `owner_member_id` from HQ, local writer custody binds
//! `configured_owner` to the satellite's own `office_id`; ONLY in that verified `OfficeRole::Satellite`
//! case does [`effective_owner_id`] fall back to HQ-signed audit metadata (`TASK-141`).

use org_store::{EngineError, Id};

use super::{
    ports::OrganizationRead,
    records::{EmployeeKind, EmployeeStatus, Meta, OfficeRole},
};

/// Returns the organization's effective owner employee ID for `read`.
///
/// Resolution priority (deterministic, fail-closed on storage errors and non-satellite inputs):
/// 1. If `configured_owner` matches an employee record in `read` (always true on HQ and on
///    satellites that have received HQ's authoritative `owner_member_id` via `Hello`), return
///    `configured_owner` directly. (If that employee was deactivated, downstream callers skip
///    inactive employees and fail closed instead of resurrecting an ex-owner.)
/// 2. Gate legacy metadata fallback strictly on `configured_owner` naming a replicated
///    `OfficeRole::Satellite` record. Any non-employee, non-satellite ID (such as a corrupted HQ
///    owner binding or pre-snapshot satellite) returns `configured_owner` unchanged (fail closed).
/// 3. Legacy pre-upgrade satellite fallback: check `updated_by` then `created_by` on the satellite
///    office record, the HQ office record, the organization record, and top-level team #1's audit
///    `meta` (never `team.lead_employee_id`, which any non-owner can hold).
/// 4. If the organization has exactly one active person, return that person's ID; otherwise return
///    `configured_owner`.
pub fn effective_owner_id(
    read: &impl OrganizationRead,
    configured_owner: &Id,
) -> Result<Id, EngineError> {
    if read.employee(configured_owner)?.is_some() {
        return Ok(configured_owner.clone());
    }
    let Some(sat_office) = read
        .office(configured_owner)?
        .filter(|office| office.role == OfficeRole::Satellite)
    else {
        return Ok(configured_owner.clone());
    };
    if let Some(id) = active_person_from_meta(read, &sat_office.meta)? {
        return Ok(id);
    }
    if let Some(hq) = read
        .offices()?
        .into_iter()
        .find(|office| office.role == OfficeRole::Hq)
    {
        if let Some(id) = active_person_from_meta(read, &hq.meta)? {
            return Ok(id);
        }
    }
    if let Some(org) = read.organization()? {
        if let Some(id) = active_person_from_meta(read, &org.meta)? {
            return Ok(id);
        }
    }
    if let Some(team) = read.team_by_number(1)? {
        if let Some(id) = active_person_from_meta(read, &team.meta)? {
            return Ok(id);
        }
    }
    let mut active_people = read
        .employees()?
        .into_iter()
        .filter(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active);
    if let (Some(sole), None) = (active_people.next(), active_people.next()) {
        return Ok(sole.id);
    }
    Ok(configured_owner.clone())
}

fn is_active_person(read: &impl OrganizationRead, id: &Id) -> Result<bool, EngineError> {
    Ok(read
        .employee(id)?
        .is_some_and(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active))
}

fn active_person_from_meta(
    read: &impl OrganizationRead,
    meta: &Meta,
) -> Result<Option<Id>, EngineError> {
    for candidate in [meta.updated_by.employee(), meta.created_by.employee()]
        .into_iter()
        .flatten()
    {
        if is_active_person(read, candidate)? {
            return Ok(Some(candidate.clone()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use org_store::{
        testkit::{id, MemoryEngine},
        Engine, Timestamp,
    };

    use super::*;
    use crate::organization::{
        ports::OrganizationWrite,
        records::{By, Employee, Office, Organization, Team, TeamStatus, DEFAULT_MAX_MEMBERS},
    };

    fn meta(created_by: By, updated_by: By) -> Meta {
        Meta {
            created_by,
            updated_by,
            ..Meta::created(&By::System, Timestamp(1_700_000_000_000))
        }
    }

    fn person(emp_id: u64, handle: &str, status: EmployeeStatus, team: u64) -> Employee {
        Employee {
            id: id(emp_id),
            handle: handle.to_owned(),
            kind: EmployeeKind::Person,
            name: format!("Person {emp_id}"),
            home_team_id: id(team),
            role_id: None,
            reports_to_employee_id: None,
            status,
            contact: None,
            title: None,
            focus: None,
            role_text: None,
            title_in_team: None,
            offboarded_at: None,
            may_hire: true,
            builtin_key: None,
            meta: meta(By::System, By::System),
            body: String::new(),
        }
    }

    fn office(oid: u64, slug: &str, role: OfficeRole, m: Meta) -> Office {
        Office {
            id: id(oid),
            slug: slug.to_owned(),
            name: slug.to_owned(),
            role,
            meta: m,
        }
    }

    #[test]
    fn configured_owner_employee_short_circuits_directly() {
        let mut tx = MemoryEngine::new().begin().unwrap();
        tx.put_employee(&person(1, "creator", EmployeeStatus::Active, 10))
            .unwrap();
        tx.put_employee(&person(2, "successor", EmployeeStatus::Active, 10))
            .unwrap();
        // Handover case: configured_owner is employee 2, not creator 1.
        assert_eq!(effective_owner_id(&tx, &id(2)).unwrap(), id(2));

        // Deactivated explicit owner still short-circuits (fails closed downstream rather than
        // resurrecting another active person).
        tx.put_employee(&person(2, "successor", EmployeeStatus::Inactive, 10))
            .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(2)).unwrap(), id(2));
    }

    #[test]
    fn non_satellite_non_employee_configured_owner_fails_closed() {
        let mut tx = MemoryEngine::new().begin().unwrap();
        tx.put_employee(&person(1, "alice", EmployeeStatus::Active, 10))
            .unwrap();
        tx.put_office(&office(
            100,
            "hq",
            OfficeRole::Hq,
            meta(By::Employee(id(1)), By::Employee(id(1))),
        ))
        .unwrap();
        // Neither a random unknown ID nor the HQ office ID triggers satellite metadata fallback.
        assert_eq!(effective_owner_id(&tx, &id(999)).unwrap(), id(999));
        assert_eq!(effective_owner_id(&tx, &id(100)).unwrap(), id(100));
    }

    #[test]
    fn satellite_never_infers_owner_from_team_one_lead() {
        let mut tx = MemoryEngine::new().begin().unwrap();
        tx.put_employee(&person(1, "alice", EmployeeStatus::Inactive, 10))
            .unwrap();
        tx.put_employee(&person(2, "bob", EmployeeStatus::Active, 10))
            .unwrap();
        tx.put_employee(&person(3, "charlie", EmployeeStatus::Active, 10))
            .unwrap();
        tx.put_office(&office(
            200,
            "sat",
            OfficeRole::Satellite,
            meta(By::Employee(id(1)), By::Employee(id(1))),
        ))
        .unwrap();
        tx.put_team(&Team {
            id: id(10),
            name: "Core".into(),
            number: 1,
            parent_team_id: None,
            // Non-owner Charlie appointed Team #1 lead must NEVER be elevated to owner.
            lead_employee_id: Some(id(3)),
            status: TeamStatus::Active,
            max_members: DEFAULT_MAX_MEMBERS,
            meta: meta(By::Employee(id(1)), By::Employee(id(2))),
        })
        .unwrap();
        // Resolves Bob from team.meta.updated_by, NOT Charlie from lead_employee_id.
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(2));

        // Even with system-attributed team meta, lead_employee_id Charlie is never selected.
        tx.put_team(&Team {
            id: id(10),
            name: "Core".into(),
            number: 1,
            parent_team_id: None,
            lead_employee_id: Some(id(3)),
            status: TeamStatus::Active,
            max_members: DEFAULT_MAX_MEMBERS,
            meta: meta(By::System, By::System),
        })
        .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(200));
    }

    #[test]
    fn satellite_legacy_meta_fallback_resolves_sat_hq_org_and_sole_person() {
        let mut tx = MemoryEngine::new().begin().unwrap();
        // Pre-snapshot empty store returns configured_owner unchanged.
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(200));

        tx.put_employee(&person(1, "alice", EmployeeStatus::Active, 10))
            .unwrap();
        tx.put_employee(&person(2, "bob", EmployeeStatus::Active, 10))
            .unwrap();
        tx.put_office(&office(
            200,
            "sat",
            OfficeRole::Satellite,
            meta(By::System, By::System),
        ))
        .unwrap();
        // Ambiguous with 2 active people and system meta -> fails closed to satellite ID.
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(200));

        // Organization metadata resolves Bob.
        tx.put_organization(&Organization {
            id: id(50),
            slug: "acme".into(),
            name: "Acme".into(),
            organization_repository_id: id(51),
            state_branch: "org/state".into(),
            default_dod: None,
            meta: meta(By::Employee(id(2)), By::Employee(id(2))),
        })
        .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(2));

        // HQ metadata takes priority over org metadata.
        tx.put_office(&office(
            100,
            "hq",
            OfficeRole::Hq,
            meta(By::Employee(id(1)), By::Employee(id(1))),
        ))
        .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(1));

        // Satellite office updated_by takes priority over HQ metadata.
        tx.put_office(&office(
            200,
            "sat",
            OfficeRole::Satellite,
            meta(By::Employee(id(1)), By::Employee(id(2))),
        ))
        .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(2));

        // When Bob is deactivated, satellite meta falls back to active creator Alice.
        tx.put_employee(&person(2, "bob", EmployeeStatus::Inactive, 10))
            .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(1));
    }
}
