//! Durable path wrappers of the documentation tree binding `org_docs::project_tree` to `DurablePath`.

pub use org_docs::project_tree::{
    file_stem_from_title, DocPath, MediaFile, PathError, Section, INDEX_FILE, MAX_DEPTH,
    MEDIA_EXTENSIONS, MEDIA_FILE_MAX_BYTES, MEDIA_FOLDER,
};
use org_docs::project_tree::{parse_docs_path, DocsPath};
use org_store::{DurablePath, Id};

/// Which file of the family a durable path is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocsFile {
    Index(Id),
    Doc(Id, DocPath),
    MediaBlob(Id, MediaFile),
    MediaMeta(Id, MediaFile),
    Foreign(Id),
}

fn durable(text: &str) -> DurablePath {
    DurablePath::parse(text).expect("validated documentation path is a durable path")
}

pub fn docs_root(project: &Id) -> DurablePath {
    durable(&format!("state/projects/{}/docs", project.as_str()))
}

pub fn index_path(project: &Id) -> DurablePath {
    durable(&format!(
        "state/projects/{}/docs/{INDEX_FILE}",
        project.as_str()
    ))
}

pub fn doc_path(project: &Id, path: &DocPath) -> DurablePath {
    durable(&format!(
        "state/projects/{}/docs/{}",
        project.as_str(),
        path.as_str()
    ))
}

pub fn media_blob_path(project: &Id, file: &MediaFile) -> DurablePath {
    durable(&format!(
        "state/projects/{}/docs/{MEDIA_FOLDER}/{}",
        project.as_str(),
        file.as_str()
    ))
}

pub fn media_meta_path(project: &Id, file: &MediaFile) -> DurablePath {
    durable(&format!(
        "state/projects/{}/docs/{MEDIA_FOLDER}/{}.yaml",
        project.as_str(),
        file.as_str()
    ))
}

/// The family's file at `path`; `None` when `path` is not under a project's `docs/`.
pub fn parse_durable(path: &DurablePath) -> Option<DocsFile> {
    Some(match parse_docs_path(path.as_str())? {
        DocsPath::Index(p) => DocsFile::Index(Id::parse(&p).ok()?),
        DocsPath::Doc(p, d) => DocsFile::Doc(Id::parse(&p).ok()?, d),
        DocsPath::MediaBlob(p, f) => DocsFile::MediaBlob(Id::parse(&p).ok()?, f),
        DocsPath::MediaMeta(p, f) => DocsFile::MediaMeta(Id::parse(&p).ok()?, f),
        DocsPath::Foreign(p) => DocsFile::Foreign(Id::parse(&p).ok()?),
    })
}
