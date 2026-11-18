//! The organization family on the memory engine: both ports over plain maps. Never part
//! of a production build.

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
};

use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::{
    ports::{OrganizationRead, OrganizationWrite, RecordRef},
    records::{
        Employee, Office, OfficeCode, Organization, Repository, Team, TeamAllocation, TeamCounter,
    },
};

/// The family's tables in the memory engine. It has no constraint of any kind.
#[derive(Clone, Default, Debug)]
pub struct OrganizationTables {
    organization: Option<Organization>,
    repositories: Vec<Repository>,
    offices: BTreeMap<Id, Office>,
    office_codes: Vec<OfficeCode>,
    teams: BTreeMap<Id, Team>,
    employees: BTreeMap<Id, Employee>,
    team_counter: Option<TeamCounter>,
    team_allocations: BTreeMap<u64, TeamAllocation>,
    record_refs: BTreeSet<RecordRef>,
}

fn record_ref(kind: &str, id: &Id) -> RecordRef {
    RecordRef {
        kind: kind.to_owned(),
        id: id.clone(),
        is_deleted: false,
    }
}

impl FamilyTable for OrganizationTables {
    fn name(&self) -> &'static str {
        "organization"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        fn rows<'a, T: std::fmt::Debug + 'a>(
            table: &'a str,
            rows: impl IntoIterator<Item = &'a T> + 'a,
        ) -> impl Iterator<Item = String> + 'a {
            let rows = rows.into_iter().enumerate();
            rows.map(move |(i, row)| format!("{table} {i:06} {row:?}"))
        }
        let mut lines: Vec<String> = Vec::new();
        lines.extend(rows("organizations", &self.organization));
        lines.extend(rows("repositories", &self.repositories));
        lines.extend(rows("offices", self.offices.values()));
        lines.extend(rows("office_codes", &self.office_codes));
        lines.extend(rows("teams", self.teams.values()));
        lines.extend(rows("employees", self.employees.values()));
        lines.extend(rows("reference_counters", &self.team_counter));
        let allocations = self.team_allocations.values();
        lines.extend(rows("reference_allocations", allocations));
        lines.extend(rows("record_refs", &self.record_refs));
        lines.sort();
        lines
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A read of the tables; a family that has written nothing reads as empty.
fn read<R: Default>(
    tables: Option<&OrganizationTables>,
    query: impl FnOnce(&OrganizationTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

macro_rules! memory_read {
    ($($view:ty),+) => {$(
        impl OrganizationRead for $view {
            fn organization(&self) -> Result<Option<Organization>, EngineError> {
                read(self.table(), |t| t.organization.clone())
            }

            fn repositories(&self) -> Result<Vec<Repository>, EngineError> {
                read(self.table(), |t| t.repositories.clone())
            }

            fn office(&self, id: &Id) -> Result<Option<Office>, EngineError> {
                read(self.table(), |t| t.offices.get(id).cloned())
            }

            fn offices(&self) -> Result<Vec<Office>, EngineError> {
                read(self.table(), |t| t.offices.values().cloned().collect())
            }

            fn office_codes(&self) -> Result<Vec<OfficeCode>, EngineError> {
                read(self.table(), |t| t.office_codes.clone())
            }

            fn team(&self, id: &Id) -> Result<Option<Team>, EngineError> {
                read(self.table(), |t| t.teams.get(id).cloned())
            }

            fn team_by_number(&self, number: u64) -> Result<Option<Team>, EngineError> {
                read(self.table(), |t| {
                    t.teams.values().find(|team| team.number == number).cloned()
                })
            }

            fn teams(&self) -> Result<Vec<Team>, EngineError> {
                read(self.table(), |t| t.teams.values().cloned().collect())
            }

            fn employee(&self, id: &Id) -> Result<Option<Employee>, EngineError> {
                read(self.table(), |t| t.employees.get(id).cloned())
            }

            fn employees(&self) -> Result<Vec<Employee>, EngineError> {
                read(self.table(), |t| t.employees.values().cloned().collect())
            }

            fn handle_in_use(&self, handle: &str) -> Result<bool, EngineError> {
                read(self.table(), |t| {
                    t.employees.values().any(|employee| employee.handle == handle)
                })
            }

            fn team_counter(&self) -> Result<Option<TeamCounter>, EngineError> {
                read(self.table(), |t| t.team_counter.clone())
            }

            fn team_allocations(&self) -> Result<Vec<TeamAllocation>, EngineError> {
                read(self.table(), |t| t.team_allocations.values().cloned().collect())
            }

            fn record_refs(&self) -> Result<Vec<RecordRef>, EngineError> {
                read(self.table(), |t| t.record_refs.iter().cloned().collect())
            }
        }
    )+};
}

memory_read!(MemoryTx, MemorySnapshot);

impl OrganizationWrite for MemoryTx {
    fn put_organization(&mut self, record: &Organization) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.record_refs.retain(|r| r.kind != "organization");
        tables
            .record_refs
            .insert(record_ref("organization", &record.id));
        tables.organization = Some(record.clone());
        Ok(())
    }

    fn remove_organization(&mut self) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.record_refs.retain(|r| r.kind != "organization");
        tables.organization = None;
        Ok(())
    }

    fn put_repositories(&mut self, records: &[Repository]) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.repositories = records.to_vec();
        Ok(())
    }

    fn put_office(&mut self, record: &Office) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.offices.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_office(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.offices.remove(id);
        Ok(())
    }

    fn put_office_codes(&mut self, records: &[OfficeCode]) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.office_codes = records.to_vec();
        Ok(())
    }

    fn put_team(&mut self, record: &Team) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.record_refs.insert(record_ref("team", &record.id));
        tables.teams.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_team(&mut self, id: &Id) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.record_refs.remove(&record_ref("team", id));
        tables.teams.remove(id);
        Ok(())
    }

    fn put_employee(&mut self, record: &Employee) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.employees.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_employee(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.employees.remove(id);
        Ok(())
    }

    fn put_team_counter(&mut self, record: &TeamCounter) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.team_counter = Some(record.clone());
        Ok(())
    }

    fn remove_team_counter(&mut self) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.team_counter = None;
        Ok(())
    }

    fn put_team_allocation(&mut self, record: &TeamAllocation) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables
            .team_allocations
            .insert(record.number, record.clone());
        Ok(())
    }

    fn remove_team_allocation(&mut self, number: u64) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.team_allocations.remove(&number);
        Ok(())
    }
}
