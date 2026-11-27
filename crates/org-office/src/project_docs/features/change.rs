//! The one place a write of the tree becomes a [`Change`]: [`TreeEdit`] starts from the
//! project's current documents and media, takes the files the write puts or moves, and
//! [`TreeEdit::finish`] always appends `index.yaml` rendered from the tree as it will be. So the
//! index is regenerated in the very journal entry that changes the tree, and no write path can
//! forget it.

use org_proto::ErrorCode;
use org_store::{Ack, Change, Id};

use crate::project_docs::access::{refused, Denied};
use crate::project_docs::index;
use crate::project_docs::paths::{self, DocPath};
use crate::project_docs::ports::ProjectDocsRead;
use crate::project_docs::records::{encode_doc, encode_media, DocEntry, DocMedia, ProjectDoc};

/// The most documents one project's tree holds: its index stays a small file.
pub const MAX_DOCS_PER_PROJECT: usize = 1_000;
/// The most media assets one project's tree holds.
pub const MAX_MEDIA_PER_PROJECT: usize = 500;

/// One write's edit of one project's tree.
pub struct TreeEdit {
    project: Id,
    docs: Vec<DocEntry>,
    media: Vec<DocMedia>,
    change: Change,
}

impl TreeEdit {
    /// The tree of `project` as `view` holds it.
    pub fn load<R: ProjectDocsRead>(view: &R, project: &Id) -> Result<Self, Denied> {
        Ok(Self {
            project: project.clone(),
            docs: view.project_doc_entries(project)?,
            media: view.project_doc_media(project)?,
            change: Change::new(Ack::Durable),
        })
    }

    /// The document at `path`, if any.
    pub fn doc_at(&self, path: &DocPath) -> Option<&DocEntry> {
        self.docs.iter().find(|d| d.path == *path)
    }

    /// The asset `file` names, if any.
    pub fn media_at(&self, file: &paths::MediaFile) -> Option<&DocMedia> {
        self.media.iter().find(|m| m.file == *file)
    }

    /// Writes `doc` at its path. A document already in the tree under the same ID is replaced,
    /// and its old file removed when the path changed. Another document at the path is
    /// `already_exists`.
    pub fn put_doc(&mut self, doc: ProjectDoc) -> Result<(), Denied> {
        if self.doc_at(&doc.path).is_some_and(|d| d.id != doc.id) {
            return Err(refused(
                ErrorCode::AlreadyExists,
                "path: another document is at this path",
            ));
        }
        match self.docs.iter().position(|d| d.id == doc.id) {
            Some(at) => {
                let old = std::mem::replace(&mut self.docs[at], doc.entry());
                if old.path != doc.path {
                    self.take_change(|c| c.remove(paths::doc_path(&old.project_id, &old.path)));
                }
            }
            None if self.docs.len() >= MAX_DOCS_PER_PROJECT => {
                return Err(refused(
                    ErrorCode::TooLarge,
                    "the project's documentation holds the most documents it may",
                ));
            }
            None => self.docs.push(doc.entry()),
        }
        let bytes = encode_doc(&doc);
        self.take_change(|c| c.put(paths::doc_path(&doc.project_id, &doc.path), bytes));
        Ok(())
    }

    /// Removes a document from the tree and schedules file removal + `index.yaml` regeneration.
    pub fn remove_doc(&mut self, doc: &ProjectDoc) -> Result<(), Denied> {
        let Some(at) = self.docs.iter().position(|d| d.id == doc.id) else {
            return Err(refused(ErrorCode::NotFound, "document not found"));
        };
        self.docs.remove(at);
        self.take_change(|c| c.remove(paths::doc_path(&doc.project_id, &doc.path)));
        Ok(())
    }

    /// Writes `media` and its `bytes`, replacing the asset of the same file.
    pub fn put_media(&mut self, media: DocMedia, bytes: Vec<u8>) -> Result<(), Denied> {
        match self.media.iter().position(|m| m.file == media.file) {
            Some(at) => self.media[at] = media.clone(),
            None if self.media.len() >= MAX_MEDIA_PER_PROJECT => {
                return Err(refused(
                    ErrorCode::TooLarge,
                    "the project's documentation holds the most media assets it may",
                ));
            }
            None => self.media.push(media.clone()),
        }
        let blob = paths::media_blob_path(&media.project_id, &media.file);
        let meta = paths::media_meta_path(&media.project_id, &media.file);
        let sidecar = encode_media(&media);
        self.take_change(|c| c.put(blob, bytes).put(meta, sidecar));
        Ok(())
    }

    fn take_change(&mut self, edit: impl FnOnce(Change) -> Change) {
        let change = std::mem::replace(&mut self.change, Change::new(Ack::Durable));
        self.change = edit(change);
    }

    /// The change: every file put or removed, then the regenerated index, then `reply`. A
    /// write whose index would be over [`index::INDEX_MAX_BYTES`] is `too_large`, so the index
    /// always replicates as one file.
    pub fn finish(self, reply: Vec<u8>) -> Result<Change, Denied> {
        let rendered = index::render(&self.project, &self.docs, &self.media);
        if rendered.len() > index::INDEX_MAX_BYTES {
            return Err(refused(
                ErrorCode::TooLarge,
                "the project's documentation index would be over 256 KiB",
            ));
        }
        Ok(self
            .change
            .put(paths::index_path(&self.project), rendered)
            .reply(reply))
    }
}
