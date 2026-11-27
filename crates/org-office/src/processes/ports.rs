//! Engine-independent read and write ports for the processes/standards family (`structure_documents`).

use org_store::{Engine, EngineError, Id};

use crate::organization::ports::{OrganizationRead, OrganizationWrite};

use super::records::StructureDocument;

/// Read-only queries over projected `structure_documents`.
pub trait ProcessesRead: OrganizationRead {
    fn structure_document(&self, id: &Id) -> Result<Option<StructureDocument>, EngineError>;
    /// All structure documents in this organization ordered by ID.
    fn structure_documents(&self) -> Result<Vec<StructureDocument>, EngineError>;
}

/// Minimal project metadata needed when validating a project-scoped process write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessProjectInfo {
    pub id: Id,
    pub owner_team_id: Id,
    pub archived: bool,
}

/// The projects a structure document may be scoped to. The processes family declares the one
/// read of projects it needs; the projects family provides it for every [`ProjectsRead`]
/// (`crate::projects::ports`), so this family never depends on that one.
///
/// [`ProjectsRead`]: crate::projects::ports::ProjectsRead
pub trait KnownProjects {
    /// The ID of every project in this organization.
    fn project_ids(&self) -> Result<Vec<Id>, EngineError>;
    /// Resolves a project by ID, `P-<number>` reference, or slug for process write scoping.
    fn resolve_process_project(
        &self,
        reference: &str,
    ) -> Result<Option<ProcessProjectInfo>, EngineError>;
}

/// Write operations on projected `structure_documents` inside a store transaction.
pub trait ProcessesWrite: ProcessesRead + OrganizationWrite {
    fn put_structure_document(&mut self, record: &StructureDocument) -> Result<(), EngineError>;
    fn remove_structure_document(&mut self, id: &Id) -> Result<(), EngineError>;
}

/// An engine supporting organization, projects, messaging, tasks, and processes.
pub trait ProcessesEngine: Engine<Tx: ProcessesWrite, Snapshot: ProcessesRead> {}

impl<E> ProcessesEngine for E where E: Engine<Tx: ProcessesWrite, Snapshot: ProcessesRead> {}
