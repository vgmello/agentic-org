//! The one path validator of the documentation tree (`.agents/rules` §2): every document path,
//! section, media file name and durable path of the family is parsed here, from raw input,
//! before anything is joined, read or written. Callers use only what it returns.
//!
//! - A document path is relative to `docs/`: lowercase ASCII segments of `[a-z0-9._-]` joined
//!   by `/`, each starting with a letter or digit, folders without `.`, the file ending in
//!   `.md`, at most [`MAX_DEPTH`] segments and [`DOC_PATH_MAX_BYTES`] bytes. So no `..`, no
//!   `/abs`, no `\`, no NUL or control character, no empty or dot segment, and never
//!   `index.yaml` or anything under `media/` (the office's own files).
//! - A section is the folder part of a document path (`guides/setup`), the empty string at the
//!   top of the tree.
//! - A media file is one segment (`architecture.png`) whose extension is an allowed image type;
//!   its metadata sits beside it as `<file>.yaml`.
//!
//! Every rule is the data contract's `documents.path` / `documentation_sections.path` /
//! `documentation_media.path` CHECK or stricter, so a row the family writes always fits.

use std::fmt;

use org_proto::docs::{DOC_PATH_MAX_BYTES, DOC_SECTION_MAX_BYTES};
use org_store::{DurablePath, Id};

use crate::organization::records::is_stable_id;

/// The deepest document path, in segments (the file included).
pub const MAX_DEPTH: usize = 8;
/// Longest media file name, in bytes.
pub const MEDIA_FILE_MAX_BYTES: usize = 128;
/// The office-maintained index file, relative to `docs/`.
pub const INDEX_FILE: &str = "index.yaml";
/// The folder of media assets, relative to `docs/`.
pub const MEDIA_FOLDER: &str = "media";
/// The extensions a media asset may have (lowercase, no dot).
pub const MEDIA_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];
const SIDECAR_SUFFIX: &str = ".yaml";

/// Why a path was refused: the caller sees `<field>: <reason>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathError(pub &'static str);

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// A validated document path relative to `docs/` (`guides/setup.md`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocPath(String);

/// A validated section: the folder of a document path, `""` at the top.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Section(String);

/// A validated media file name (`architecture.png`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaFile(String);

fn segment_ok(segment: &str, dots: bool) -> bool {
    let mut bytes = segment.bytes();
    let first_ok = bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    first_ok
        && bytes.all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b == b'-'
                || b == b'_'
                || (dots && b == b'.')
        })
        && !segment.contains("..")
}

impl DocPath {
    /// Parses a caller's document path.
    pub fn parse(raw: &str) -> Result<Self, PathError> {
        if raw.is_empty() || raw.len() > DOC_PATH_MAX_BYTES {
            return Err(PathError("must be 4 to 512 bytes"));
        }
        if raw == INDEX_FILE {
            return Err(PathError("index.yaml is maintained by the office"));
        }
        if raw.starts_with('/') {
            return Err(PathError("must be relative to docs/"));
        }
        let segments: Vec<&str> = raw.split('/').collect();
        if segments.len() > MAX_DEPTH {
            return Err(PathError("is nested too deeply"));
        }
        if segments[0] == MEDIA_FOLDER {
            return Err(PathError("media/ holds media assets, not documents"));
        }
        let (file, folders) = segments.split_last().expect("split yields one segment");
        if !folders.iter().all(|s| segment_ok(s, false)) || !segment_ok(file, true) {
            return Err(PathError(
                "segments are lowercase letters, digits, `-` and `_` (and `.` in the file name), \
                 each starting with a letter or digit; no `..`, empty segment or `\\`",
            ));
        }
        let Some(stem) = file.strip_suffix(".md") else {
            return Err(PathError("must end in `.md`"));
        };
        if stem.is_empty() || stem.ends_with('.') {
            return Err(PathError("needs a file name before `.md`"));
        }
        Ok(Self(raw.to_owned()))
    }

    /// The path joined from a section and a file name.
    pub fn join(section: &Section, file: &str) -> Result<Self, PathError> {
        if section.0.is_empty() {
            Self::parse(file)
        } else {
            Self::parse(&format!("{}/{file}", section.0))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The folder this document sits in.
    pub fn section(&self) -> Section {
        Section(
            self.0
                .rsplit_once('/')
                .map_or_else(String::new, |(folder, _)| folder.to_owned()),
        )
    }

    /// The file name (`setup.md`).
    pub fn file_name(&self) -> &str {
        self.0.rsplit_once('/').map_or(self.0.as_str(), |(_, f)| f)
    }
}

impl Section {
    /// Parses a caller's section: `""` is the top of the tree.
    pub fn parse(raw: &str) -> Result<Self, PathError> {
        if raw.is_empty() {
            return Ok(Self::default());
        }
        if raw.len() > DOC_SECTION_MAX_BYTES {
            return Err(PathError("must be at most 256 bytes"));
        }
        if raw.starts_with('/') || raw.ends_with('/') {
            return Err(PathError("has no leading or trailing `/`"));
        }
        let segments: Vec<&str> = raw.split('/').collect();
        if segments.len() >= MAX_DEPTH {
            return Err(PathError("is nested too deeply"));
        }
        if segments[0] == MEDIA_FOLDER {
            return Err(PathError("media/ holds media assets, not documents"));
        }
        if !segments.iter().all(|s| segment_ok(s, false)) {
            return Err(PathError(
                "segments are lowercase letters, digits, `-` and `_`, each starting with a \
                 letter or digit",
            ));
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `other` is this section or below it.
    pub fn contains(&self, other: &Section) -> bool {
        self.0.is_empty()
            || other.0 == self.0
            || other
                .0
                .strip_prefix(&self.0)
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

impl MediaFile {
    /// Parses a caller's media file name.
    pub fn parse(raw: &str) -> Result<Self, PathError> {
        if raw.is_empty() || raw.len() > MEDIA_FILE_MAX_BYTES {
            return Err(PathError("must be 1 to 128 bytes"));
        }
        if !segment_ok(raw, true) {
            return Err(PathError(
                "is one lowercase file name of letters, digits, `.`, `-` and `_`, starting \
                 with a letter or digit; no folder, `..` or `\\`",
            ));
        }
        match raw.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() && MEDIA_EXTENSIONS.contains(&ext) => {
                Ok(Self(raw.to_owned()))
            }
            _ => Err(PathError("must end in .png, .jpg, .jpeg, .webp or .gif")),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The extension, lowercase, without the dot.
    pub fn extension(&self) -> &str {
        self.0.rsplit_once('.').map_or("", |(_, ext)| ext)
    }

    /// The asset's path relative to `docs/` (`media/architecture.png`).
    pub fn rel_path(&self) -> String {
        format!("{MEDIA_FOLDER}/{}", self.0)
    }

    /// Parses `media/<file>` relative to `docs/`.
    pub fn from_rel_path(raw: &str) -> Option<Self> {
        let file = raw.strip_prefix(MEDIA_FOLDER)?.strip_prefix('/')?;
        Self::parse(file).ok()
    }
}

/// A title turned into a file stem: lowercase ASCII words joined by `-`, at most 64 bytes,
/// `document` when nothing is left.
pub fn file_stem_from_title(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            if out.len() >= 64 {
                break;
            }
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "document".to_owned()
    } else {
        out
    }
}

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
