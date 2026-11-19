//! The organization family's two ports. An engine implements both once: the read port for
//! its transaction and its snapshot, the write port for its transaction. Decisions read
//! through the first; the projector writes through the second. Neither names a database.
//!
//! The ports hold no rule: an engine without constraints (the memory engine) is as valid
//! as one with them. The rules are in the decisions and in the projector's `validate`.

use org_store::{CoreRead, Engine, EngineError, Id};

use super::records::{
    Employee, Office, OfficeCode, Organization, Repository, Team, TeamAllocation, TeamCounter,
};

/// The deepest team tree a chain is followed through. A path cannot hold more.
pub const MAX_TEAM_DEPTH: usize = 16;

/// A row of `record_refs`: a record other families may refer to by kind and ID.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct RecordRef {
    pub kind: String,
    pub id: Id,
    pub is_deleted: bool,
}

pub trait OrganizationRead: CoreRead {
    /// The directory generation of this view: the organization stream's high-water as of
    /// the last committed batch. A snapshot returns the generation of what it shows.
    fn generation(&self) -> Result<u64, EngineError> {
        let cursor = self.cursor()?;
        Ok(cursor.map_or(0, |cursor| cursor.high_waters.organization))
    }

    fn organization(&self) -> Result<Option<Organization>, EngineError>;
    /// In the order of `repositories.yaml`.
    fn repositories(&self) -> Result<Vec<Repository>, EngineError>;
    fn office(&self, id: &Id) -> Result<Option<Office>, EngineError>;
    /// In order of ID.
    fn offices(&self) -> Result<Vec<Office>, EngineError>;
    /// Every code HQ has issued, current and retired, in the order of the file.
    fn office_codes(&self) -> Result<Vec<OfficeCode>, EngineError>;
    fn team(&self, id: &Id) -> Result<Option<Team>, EngineError>;
    fn team_by_number(&self, number: u64) -> Result<Option<Team>, EngineError>;
    /// In order of ID.
    fn teams(&self) -> Result<Vec<Team>, EngineError>;
    fn employee(&self, id: &Id) -> Result<Option<Employee>, EngineError>;
    /// In order of ID.
    fn employees(&self) -> Result<Vec<Employee>, EngineError>;
    /// Whether any employee, of any status, has this handle.
    fn handle_in_use(&self, handle: &str) -> Result<bool, EngineError>;
    fn team_counter(&self) -> Result<Option<TeamCounter>, EngineError>;
    /// In order of number.
    fn team_allocations(&self) -> Result<Vec<TeamAllocation>, EngineError>;
    /// In order of kind, then ID.
    fn record_refs(&self) -> Result<Vec<RecordRef>, EngineError>;

    /// The code `office` creates task references with: its one code that is not retired.
    fn office_code(&self, office: &Id) -> Result<Option<OfficeCode>, EngineError> {
        let mut codes = self.office_codes()?.into_iter();
        Ok(codes.find(|code| code.retired_at.is_none() && code.office_id == *office))
    }

    /// The teams from the top-level one down to `id`, which is the last. `None` when the
    /// team or one above it is missing, or the chain is deeper than [`MAX_TEAM_DEPTH`]
    /// (which a cycle is).
    fn team_chain(&self, id: &Id) -> Result<Option<Vec<Id>>, EngineError> {
        let mut chain = Vec::new();
        let mut next = Some(id.clone());
        while let Some(id) = next {
            let Some(team) = self.team(&id)? else {
                return Ok(None);
            };
            if chain.len() == MAX_TEAM_DEPTH {
                return Ok(None);
            }
            chain.push(id);
            next = team.parent_team_id;
        }
        chain.reverse();
        Ok(Some(chain))
    }
}

/// Row upserts and removals, one pair per record kind. A put replaces the row of the same
/// key whole. `put_organization` and `put_team` also keep the record's `record_refs` row,
/// and their removals drop it.
pub trait OrganizationWrite: OrganizationRead {
    fn put_organization(&mut self, record: &Organization) -> Result<(), EngineError>;
    fn remove_organization(&mut self) -> Result<(), EngineError>;
    /// Replaces every repository row: the file is the list. An empty list removes them.
    fn put_repositories(&mut self, records: &[Repository]) -> Result<(), EngineError>;
    fn put_office(&mut self, record: &Office) -> Result<(), EngineError>;
    fn remove_office(&mut self, id: &Id) -> Result<(), EngineError>;
    /// Replaces every office-code row: the file is the list. An empty list removes them.
    fn put_office_codes(&mut self, records: &[OfficeCode]) -> Result<(), EngineError>;
    fn put_team(&mut self, record: &Team) -> Result<(), EngineError>;
    fn remove_team(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_employee(&mut self, record: &Employee) -> Result<(), EngineError>;
    fn remove_employee(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_team_counter(&mut self, record: &TeamCounter) -> Result<(), EngineError>;
    fn remove_team_counter(&mut self) -> Result<(), EngineError>;
    fn put_team_allocation(&mut self, record: &TeamAllocation) -> Result<(), EngineError>;
    fn remove_team_allocation(&mut self, number: u64) -> Result<(), EngineError>;
}

/// An engine the organization family runs on: both ports, where the store needs them.
pub trait OrganizationEngine: Engine<Tx: OrganizationWrite, Snapshot: OrganizationRead> {}

impl<E> OrganizationEngine for E where E: Engine<Tx: OrganizationWrite, Snapshot: OrganizationRead> {}
