//! Durable path wrappers of the documentation tree: pure path parsing and validation live in
//! `org_docs::project_tree`, and this module binds them to `org_store::DurablePath`.

pub use org_docs::project_tree::{
    file_stem_from_title, DocPath, MediaFile, PathError, Section, INDEX_FILE, MAX_DEPTH,
    MEDIA_EXTENSIONS, MEDIA_FILE_MAX_BYTES, MEDIA_FOLDER,
};
use org_store::{DurablePath, Id};

use crate::organization::records::is_stable_id;

const SIDECAR_SUFFIX: &str = ".yaml";

/// Which file of the family a durable path is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocsFile {
    /// `docs/index.yaml`.
    Index(Id),
    /// `docs/<path>.md`.
    Doc(Id, DocPath),
    /// `docs/media/<file>`: the asset's bytes.
    MediaBlob(Id, MediaFile),
    /// `docs/media/<file>.yaml`: the asset's metadata.
    MediaMeta(Id, MediaFile),
    /// Anything else under a project's `docs/`: owned, so it is refused, never projected.
    Foreign(Id),
}

fn project_root(project: &Id) -> String {
    format!("state/projects/{}/docs", project.as_str())
}

fn durable(text: &str) -> DurablePath {
    DurablePath::parse(text).expect("validated documentation path is a durable path")
}

/// `state/projects/<id>/docs`: the family's prefix for one project.
pub fn docs_root(project: &Id) -> DurablePath {
    durable(&project_root(project))
}

pub fn index_path(project: &Id) -> DurablePath {
    durable(&format!("{}/{INDEX_FILE}", project_root(project)))
}

pub fn doc_path(project: &Id, path: &DocPath) -> DurablePath {
    durable(&format!("{}/{}", project_root(project), path.as_str()))
}

pub fn media_blob_path(project: &Id, file: &MediaFile) -> DurablePath {
    durable(&format!(
        "{}/{MEDIA_FOLDER}/{}",
        project_root(project),
        file.as_str()
    ))
}

pub fn media_meta_path(project: &Id, file: &MediaFile) -> DurablePath {
    durable(&format!(
        "{}/{MEDIA_FOLDER}/{}{SIDECAR_SUFFIX}",
        project_root(project),
        file.as_str()
    ))
}

/// The family's file at `path`; `None` when `path` is not under a project's `docs/`.
pub fn parse_durable(path: &DurablePath) -> Option<DocsFile> {
    let parts: Vec<&str> = path.components().collect();
    let ["state", "projects", project, "docs", rest @ ..] = parts.as_slice() else {
        return None;
    };
    if !is_stable_id(project) {
        return None;
    }
    let project = Id::parse(project).ok()?;
    Some(match rest {
        [] => DocsFile::Foreign(project),
        [INDEX_FILE] => DocsFile::Index(project),
        [MEDIA_FOLDER, name] => match name.strip_suffix(SIDECAR_SUFFIX) {
            Some(file) => MediaFile::parse(file).map_or(DocsFile::Foreign(project.clone()), |f| {
                DocsFile::MediaMeta(project.clone(), f)
            }),
            None => MediaFile::parse(name).map_or(DocsFile::Foreign(project.clone()), |f| {
                DocsFile::MediaBlob(project.clone(), f)
            }),
        },
        _ => DocPath::parse(&rest.join("/")).map_or(DocsFile::Foreign(project.clone()), |p| {
            DocsFile::Doc(project.clone(), p)
        }),
    })
}
