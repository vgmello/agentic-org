//! Read and write ports for the projects family, extending [`OrganizationRead`] and
//! [`OrganizationWrite`] so atomic project+team creation runs in one unit of work.

use std::collections::BTreeMap;

use org_store::{Engine, EngineError, Id};

use crate::organization::ports::{
    OrganizationRead, OrganizationWrite, TeamDependents, TeamHomedMove,
};
use crate::processes::ports::{KnownProjects, ProcessesRead};
use crate::processes::records::{encode_structure_document, structure_document_path};

use super::records::{Project, ProjectAllocation, ProjectCounter};

pub trait ProjectsRead: OrganizationRead + ProcessesRead {
    fn project(&self, id: &Id) -> Result<Option<Project>, EngineError>;
    fn project_by_number(&self, number: u64) -> Result<Option<Project>, EngineError>;
    fn project_by_slug(&self, slug: &str) -> Result<Option<Project>, EngineError>;
    /// Projects in ascending `number` order (then ID).
    fn projects(&self) -> Result<Vec<Project>, EngineError>;
    fn project_counter(&self) -> Result<Option<ProjectCounter>, EngineError>;
    /// Allocations in ascending `number` order.
    fn project_allocations(&self) -> Result<Vec<ProjectAllocation>, EngineError>;

    /// Resolves a project by UUIDv7 ID, `P-<number>` reference (case-insensitive), or slug.
    fn resolve_project(&self, reference: &str) -> Result<Option<Project>, EngineError> {
        let trimmed = reference.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if let Ok(id) = Id::parse(trimmed) {
            if let Some(found) = self.project(&id)? {
                return Ok(Some(found));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("P-")
            .or_else(|| trimmed.strip_prefix("p-"))
        {
            if !rest.is_empty()
                && !rest.starts_with('0')
                && rest.bytes().all(|b| b.is_ascii_digit())
            {
                if let Ok(number) = rest.parse::<u64>() {
                    if let Some(found) = self.project_by_number(number)? {
                        return Ok(Some(found));
                    }
                }
            }
        }
        self.project_by_slug(&trimmed.to_ascii_lowercase())
    }
}

/// Every [`ProjectsRead`] answers the processes family's [`KnownProjects`].
impl<T: ProjectsRead + ?Sized> KnownProjects for T {
    fn project_ids(&self) -> Result<Vec<Id>, EngineError> {
        Ok(self.projects()?.into_iter().map(|p| p.id).collect())
    }
}

/// Every [`ProjectsRead`] answers the organization's [`TeamDependents`]: its projects, and the
/// team process and standard documents that move with their team.
impl<T: ProjectsRead + ?Sized> TeamDependents for T {
    fn project_exists(&self, id: &Id) -> Result<bool, EngineError> {
        Ok(self.project(id)?.is_some())
    }

    fn team_in_active_project(&self, team: &Id) -> Result<bool, EngineError> {
        Ok(self.projects()?.iter().any(|p| {
            !p.archived && (p.owner_team_id == *team || p.contributing_team_ids.contains(team))
        }))
    }

    fn team_homed_moves(
        &self,
        chains: &BTreeMap<Id, (Vec<Id>, Vec<Id>)>,
    ) -> Result<Vec<TeamHomedMove>, EngineError> {
        let mut moves = Vec::new();
        for doc in self.structure_documents()? {
            let Some(ref tid) = doc.team_id else {
                continue;
            };
            let Some((from, to)) = chains.get(tid) else {
                continue;
            };
            moves.push(TeamHomedMove {
                from: structure_document_path(doc.kind, &doc.id, Some(from), None),
                to: structure_document_path(doc.kind, &doc.id, Some(to), None).ok(),
                bytes: encode_structure_document(&doc),
            });
        }
        Ok(moves)
    }
}

pub trait ProjectsWrite: ProjectsRead + OrganizationWrite {
    fn put_project(&mut self, record: &Project) -> Result<(), EngineError>;
    fn remove_project(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_project_counter(&mut self, record: &ProjectCounter) -> Result<(), EngineError>;
    fn remove_project_counter(&mut self) -> Result<(), EngineError>;
    fn put_project_allocation(&mut self, record: &ProjectAllocation) -> Result<(), EngineError>;
    fn remove_project_allocation(&mut self, number: u64) -> Result<(), EngineError>;
}

pub trait ProjectsEngine: Engine<Tx: ProjectsWrite, Snapshot: ProjectsRead> {}

impl<E> ProjectsEngine for E where E: Engine<Tx: ProjectsWrite, Snapshot: ProjectsRead> {}
