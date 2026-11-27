//! Read and write ports of the documentation tree family, above the projects family: a tree
//! belongs to a project, and who may read it follows the project's teams.

use org_store::{Engine, EngineError, Id};

use super::records::{DocEntry, DocMedia, ProjectDoc};
use crate::projects::ports::ProjectsRead;

/// Queries over projected documents and media.
pub trait ProjectDocsRead: ProjectsRead {
    /// Every document of `project` without its body, in path order.
    fn project_doc_entries(&self, project: &Id) -> Result<Vec<DocEntry>, EngineError>;
    /// The document with this ID and its body, in whichever project it is.
    fn project_doc(&self, id: &Id) -> Result<Option<ProjectDoc>, EngineError>;
    /// Every media asset of `project`, in path order.
    fn project_doc_media(&self, project: &Id) -> Result<Vec<DocMedia>, EngineError>;
    /// Every project with at least one document or media asset, in ID order.
    fn projects_with_docs(&self) -> Result<Vec<Id>, EngineError>;
}

/// Changes to projected documents and media inside a store transaction. Rows are keyed by
/// their file: removing a file removes the row at that path, never a row that moved away.
pub trait ProjectDocsWrite: ProjectDocsRead {
    fn put_project_doc(&mut self, doc: &ProjectDoc) -> Result<(), EngineError>;
    fn remove_project_doc_at(&mut self, project: &Id, path: &str) -> Result<(), EngineError>;
    fn put_doc_media(&mut self, media: &DocMedia) -> Result<(), EngineError>;
    fn remove_doc_media_at(&mut self, project: &Id, path: &str) -> Result<(), EngineError>;
}

/// An engine holding the documentation tree family.
pub trait ProjectDocsEngine: Engine<Tx: ProjectDocsWrite, Snapshot: ProjectDocsRead> {}

impl<E> ProjectDocsEngine for E where E: Engine<Tx: ProjectDocsWrite, Snapshot: ProjectDocsRead> {}
