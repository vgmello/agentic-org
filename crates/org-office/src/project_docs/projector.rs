//! Engine-independent projector of the documentation tree (`ProjectDocFiles`): documents into
//! `documents`, media metadata into `documentation_media`. It owns everything under a
//! project's `docs/`, so a file that is not a document, a media asset, its metadata or the
//! index is refused rather than left unowned.
//!
//! After a rebuild, [`validate`] holds the tree to what the office writes: every file is
//! projected, the stored `index.yaml` is byte-for-byte the index rendered from the rows, each
//! asset's bytes hash to its recorded SHA-256, IDs are unique and every employee referenced
//! exists. A tree edited by hand fails it.

use std::collections::BTreeSet;

use org_store::{CoreRead, DurablePath, Engine, Id, ProjectionError, Projector, Sha256};

use super::index;
use super::paths::{self, DocsFile};
use super::ports::{ProjectDocsRead, ProjectDocsWrite};
use super::records::{self, MEDIA_RECORD_MAX_BYTES};
use crate::attachments::media::{sniff, MediaType};
use crate::organization::records::Meta;

/// Projects `state/projects/<id>/docs/**`.
pub struct ProjectDocFiles;

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

fn check_blob(
    path: &DurablePath,
    file: &paths::MediaFile,
    bytes: &[u8],
) -> Result<(), ProjectionError> {
    if bytes.is_empty() || bytes.len() as u64 > MEDIA_RECORD_MAX_BYTES {
        return Err(refuse(path, "a media asset is 1 byte to 5 MiB"));
    }
    let declared = MediaType::from_mime(records::media_type_of(file))
        .ok_or_else(|| refuse(path, "not an allowed media type"))?;
    sniff::check_matches(declared, bytes).map_err(|e| refuse(path, e.message))
}

impl<E: Engine> Projector<E> for ProjectDocFiles
where
    E::Tx: ProjectDocsWrite,
{
    fn owns(&self, path: &DurablePath) -> bool {
        paths::parse_durable(path).is_some()
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        let Some(file) = paths::parse_durable(path) else {
            return Err(refuse(path, "not a documentation file"));
        };
        match (file, bytes) {
            // A stray file is never projected; a rebuild reports it and goes on.
            (DocsFile::Foreign(_), Some(_)) => Err(ProjectionError::skippable(
                path,
                "not a documentation file: documents are lowercase `.md` paths, assets sit in media/",
            )),
            // The index holds no row: `validate` compares it with the rows after a rebuild.
            (DocsFile::Foreign(_) | DocsFile::Index(_) | DocsFile::MediaBlob(..), None)
            | (DocsFile::Index(_), Some(_)) => Ok(()),
            (DocsFile::Doc(project, doc_path), Some(bytes)) => {
                let doc = records::decode_doc(bytes, &project, doc_path)
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_project_doc(&doc)?)
            }
            (DocsFile::Doc(project, doc_path), None) => {
                Ok(tx.remove_project_doc_at(&project, doc_path.as_str())?)
            }
            (DocsFile::MediaMeta(project, file), Some(bytes)) => {
                let media = records::decode_media(bytes, &project, file)
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_doc_media(&media)?)
            }
            (DocsFile::MediaMeta(project, file), None) => {
                Ok(tx.remove_doc_media_at(&project, &file.rel_path())?)
            }
            (DocsFile::MediaBlob(_, file), Some(bytes)) => check_blob(path, &file, bytes),
        }
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        validate(tx)
    }
}

fn check_attribution(
    path: &DurablePath,
    meta: &Meta,
    owner: Option<&Id>,
    employees: &BTreeSet<Id>,
) -> Result<(), ProjectionError> {
    let referenced = [
        meta.created_by.employee(),
        meta.updated_by.employee(),
        owner,
    ];
    if referenced
        .into_iter()
        .flatten()
        .any(|id| !employees.contains(id))
    {
        return Err(refuse(path, "a referenced employee does not exist"));
    }
    Ok(())
}

/// Holds one project's tree to what the office writes.
fn validate_project<T: ProjectDocsRead + CoreRead>(
    tx: &T,
    project: &Id,
    employees: &BTreeSet<Id>,
    ids: &mut BTreeSet<Id>,
) -> Result<(), ProjectionError> {
    let docs = tx.project_doc_entries(project)?;
    let media = tx.project_doc_media(project)?;
    let sources = tx.sources_under(&paths::docs_root(project))?;
    if docs.is_empty() && media.is_empty() && sources.is_empty() {
        return Ok(());
    }
    if tx.project(project)?.is_none() {
        return Err(refuse(
            &paths::docs_root(project),
            "documentation of a project that does not exist",
        ));
    }
    let index_path = paths::index_path(project);
    let mut expected: BTreeSet<DurablePath> = BTreeSet::from([index_path.clone()]);
    for doc in &docs {
        let path = paths::doc_path(project, &doc.path);
        if !ids.insert(doc.id.clone()) {
            return Err(refuse(&path, "another document or asset has this id"));
        }
        check_attribution(&path, &doc.meta, doc.owner_employee_id.as_ref(), employees)?;
        expected.insert(path);
    }
    for asset in &media {
        let meta_path = paths::media_meta_path(project, &asset.file);
        let blob_path = paths::media_blob_path(project, &asset.file);
        if !ids.insert(asset.id.clone()) {
            return Err(refuse(&meta_path, "another document or asset has this id"));
        }
        check_attribution(&meta_path, &asset.meta, None, employees)?;
        let blob_hash = tx.source_hash(&blob_path)?;
        if blob_hash.map(|h| h.to_hex()).as_deref() != Some(asset.sha256.as_str()) {
            return Err(refuse(
                &blob_path,
                "the asset's bytes are missing or do not match its recorded sha256",
            ));
        }
        expected.insert(meta_path);
        expected.insert(blob_path);
    }
    if let Some(stray) = sources.iter().find(|s| !expected.contains(*s)) {
        return Err(refuse(
            stray,
            "a documentation file the office does not account for (refused or duplicated)",
        ));
    }
    let rendered = index::render(project, &docs, &media);
    if rendered.len() > index::INDEX_MAX_BYTES {
        return Err(refuse(&index_path, "index.yaml would be over 256 KiB"));
    }
    if tx.source_hash(&index_path)? != Some(Sha256::of(&rendered)) {
        return Err(refuse(
            &index_path,
            "index.yaml is missing or does not match the documentation tree",
        ));
    }
    Ok(())
}

fn validate<T: ProjectDocsRead + CoreRead>(tx: &T) -> Result<(), ProjectionError> {
    let mut projects: BTreeSet<Id> = tx.projects()?.into_iter().map(|p| p.id).collect();
    projects.extend(tx.projects_with_docs()?);
    let employees: BTreeSet<Id> = tx.employees()?.into_iter().map(|e| e.id).collect();
    let mut ids = BTreeSet::new();
    for project in &projects {
        validate_project(tx, project, &employees, &mut ids)?;
    }
    Ok(())
}
