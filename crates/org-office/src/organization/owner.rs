//! Resolving the organization's effective owner employee ID from a consistent organization read.
//!
//! On HQ, an office's configured `owner` is already the owner's employee UUIDv7 (including after
//! ownership handover via `Office::open_as_owner`). On a replicated satellite office, local writer
//! custody binds `Office::owner` to the satellite's own `office_id`, so directory snapshots,
//! messaging handle resolution (`"owner"`), auto-channel membership, and inbound/outbound relay
//! checks must resolve the replicated organization owner's employee ID from the committed
//! HQ-authoritative organization records (`TASK-141`).

use org_store::{EngineError, Id};

use super::{
    ports::OrganizationRead,
    records::{EmployeeKind, EmployeeStatus, Meta, OfficeRole},
};

/// Returns the organization's effective owner employee ID for `read`.
///
/// Resolution priority (deterministic, fail-closed on storage errors):
/// 1. If `configured_owner` matches an employee record in `read` (always true on HQ, including
///    after ownership handover), return `configured_owner` directly.
/// 2. Check the enrolled satellite office record `read.office(configured_owner)` (`updated_by`,
///    then `created_by`) for an active person — only the organization owner can register a
///    satellite office at HQ.
/// 3. Check the HQ office record (`OfficeRole::Hq`), then the organization record
///    (`read.organization()`), for an active person in `updated_by` then `created_by`.
/// 4. Check the top-level team (`number == 1`) `lead_employee_id` and `meta` for an active person.
/// 5. If the organization has exactly one active person, return that person's ID.
/// 6. Otherwise (e.g. a freshly provisioned satellite before its initial HQ snapshot), return
///    `configured_owner`.
pub fn effective_owner_id(
    read: &impl OrganizationRead,
    configured_owner: &Id,
) -> Result<Id, EngineError> {
    if read.employee(configured_owner)?.is_some() {
        return Ok(configured_owner.clone());
    }
    if let Some(office) = read.office(configured_owner)? {
        if let Some(id) = active_person_from_meta(read, &office.meta)? {
            return Ok(id);
        }
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
        if let Some(lead) = team.lead_employee_id.as_ref() {
            if is_active_person(read, lead)? {
                return Ok(lead.clone());
            }
        }
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
    }

    #[test]
    fn satellite_resolves_owner_from_office_and_org_metadata_with_multiple_people() {
        let mut tx = MemoryEngine::new().begin().unwrap();
        tx.put_employee(&person(1, "alice", EmployeeStatus::Active, 10))
            .unwrap();
        tx.put_employee(&person(2, "bob", EmployeeStatus::Active, 10))
            .unwrap();
        tx.put_office(&office(
            100,
            "hq",
            OfficeRole::Hq,
            meta(By::Employee(id(1)), By::Employee(id(1))),
        ))
        .unwrap();
        // Satellite enrolled by successor Bob (updated_by beats created_by and HQ meta).
        tx.put_office(&office(
            200,
            "sat",
            OfficeRole::Satellite,
            meta(By::Employee(id(1)), By::Employee(id(2))),
        ))
        .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(2));

        // If Bob is deactivated, falls back to active creator Alice.
        tx.put_employee(&person(2, "bob", EmployeeStatus::Inactive, 10))
            .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(1));
    }

    #[test]
    fn satellite_falls_back_through_org_top_team_sole_person_and_empty_store() {
        let mut tx = MemoryEngine::new().begin().unwrap();
        // Pre-snapshot empty store returns configured_owner unchanged.
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(200));

        tx.put_employee(&person(1, "alice", EmployeeStatus::Active, 10))
            .unwrap();
        tx.put_employee(&person(2, "bob", EmployeeStatus::Active, 10))
            .unwrap();
        // Ambiguous with 2 active people and no metadata -> returns configured_owner.
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(200));

        // Top team #1 lead resolves owner when office/org metadata is system-attributed.
        tx.put_team(&Team {
            id: id(10),
            name: "Core".into(),
            number: 1,
            parent_team_id: None,
            lead_employee_id: Some(id(1)),
            status: TeamStatus::Active,
            max_members: DEFAULT_MAX_MEMBERS,
            meta: meta(By::System, By::System),
        })
        .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(1));

        // Organization metadata takes priority over top-team lead.
        tx.put_organization(&Organization {
            id: id(50),
            slug: "acme".into(),
            name: "Acme".into(),
            organization_repository_id: id(51),
            state_branch: "org/state".into(),
            meta: meta(By::Employee(id(2)), By::Employee(id(2))),
        })
        .unwrap();
        assert_eq!(effective_owner_id(&tx, &id(200)).unwrap(), id(2));
    }
}
