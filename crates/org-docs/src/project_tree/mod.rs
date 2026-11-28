//! Pure project documentation tree rules, codecs, `index.yaml` rendering, and
//! engine-independent operations (`TASK-55`, `FC-180`, `PKG-11/12/14`).

/// Deterministic Markdown/YAML codecs for project documents and media sidecars.
pub mod codecs;
/// Deterministic JSON/YAML encoding and canonical `docs/index.yaml` rendering.
pub mod index;
/// Engine-independent read and write decisions for `docs.*`.
pub mod ops;

use std::fmt;

use org_proto::docs::{
    DOC_BODY_MAX_BYTES, DOC_KIND_MAX_BYTES, DOC_PATH_MAX_BYTES, DOC_SECTION_MAX_BYTES,
    DOC_SUMMARY_MAX_BYTES,
};

use crate::is_invisible_or_break;

pub use index::{
    canonical_json, render_project_index, yaml_encode, yaml_encode_document, IndexDocRow,
    IndexMediaRow,
};

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
/// Largest `index.yaml` (256 KiB): fits inside one replication payload and frame.
pub const INDEX_MAX_BYTES: usize = 256 * 1024;

const RESERVED_KINDS: [&str; 2] = ["brief", "changelog"];

/// Why a documentation tree path was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathError(pub &'static str);

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PathError {}

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
    /// Parses a caller's document path relative to `docs/`.
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

    /// Joins a validated section and a file name into a validated document path.
    pub fn join(section: &Section, file: &str) -> Result<Self, PathError> {
        if section.0.is_empty() {
            Self::parse(file)
        } else {
            Self::parse(&format!("{}/{file}", section.0))
        }
    }

    /// The relative path string (`guides/setup.md`).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The folder this document sits in (`""` at top level).
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
    /// Parses a caller's section (`""` is the top of the tree).
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

    /// The section path string (`""` at top level).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `other` is this section or nested beneath it.
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
    /// Parses a caller's media asset file name (`architecture.png`).
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

    /// The media file name (`architecture.png`).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The lowercase extension without dot.
    pub fn extension(&self) -> &str {
        self.0.rsplit_once('.').map_or("", |(_, ext)| ext)
    }

    /// The asset path relative to `docs/` (`media/architecture.png`).
    pub fn rel_path(&self) -> String {
        format!("{MEDIA_FOLDER}/{}", self.0)
    }

    /// Parses `media/<file>` relative to `docs/`.
    pub fn from_rel_path(raw: &str) -> Option<Self> {
        let file = raw.strip_prefix(MEDIA_FOLDER)?.strip_prefix('/')?;
        Self::parse(file).ok()
    }
}

/// Converts a document title to a lowercase ASCII file stem (`<= 64` bytes, `"document"` fallback).
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

/// Lifecycle state of a document (`documents.status`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DocStatus {
    /// Draft document.
    Draft,
    /// Current canonical document (requires an owner).
    Current,
    /// Deprecated / archived document.
    Deprecated,
}

impl DocStatus {
    /// Every status value.
    pub const ALL: [Self; 3] = [Self::Draft, Self::Current, Self::Deprecated];

    /// Wire and schema string representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Current => "current",
            Self::Deprecated => "deprecated",
        }
    }

    /// Parses a status string.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }
}

/// What a media asset depicts (`documentation_media.type`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum MediaKind {
    /// Photograph, screenshot, or illustration.
    Image,
    /// Architecture or technical diagram.
    Diagram,
}

impl MediaKind {
    /// Every media kind.
    pub const ALL: [Self; 2] = [Self::Image, Self::Diagram];

    /// Wire and schema string representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Diagram => "diagram",
        }
    }

    /// Parses a media kind string.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }
}

/// Checks one visible trimmed single-line title (`1..=200` Unicode scalar values).
pub fn is_title_line(text: &str) -> bool {
    !text.is_empty()
        && text.trim() == text
        && text.chars().count() <= 200
        && !text
            .chars()
            .any(|c| c.is_control() || is_invisible_or_break(c))
}

/// Checks whether `text` is a valid tree document kind (`1..=64` slug outside `brief`/`changelog`).
pub fn is_doc_kind(text: &str) -> bool {
    let first_ok = text
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    first_ok
        && text.len() <= DOC_KIND_MAX_BYTES
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !RESERVED_KINDS.contains(&text)
}

/// Checks whether `text` is a valid document summary (`<= 2 KiB`, trimmed, no control/invisible chars).
pub fn is_summary(text: &str) -> bool {
    text.len() <= DOC_SUMMARY_MAX_BYTES
        && text.trim() == text
        && !text
            .chars()
            .any(|c| (c.is_control() && c != '\n') || (c != '\n' && is_invisible_or_break(c)))
}

/// Checks whether `text` is a valid Markdown body (`<= 256 KiB`, no NUL).
pub fn is_body(text: &str) -> bool {
    text.len() <= DOC_BODY_MAX_BYTES && !text.contains('\0')
}

/// Checks whether `text` is a 64-character lowercase hex SHA-256 digest.
pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Returns the MIME type for a validated media file's extension.
pub fn media_type_of(file: &MediaFile) -> &'static str {
    match file.extension() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/gif",
    }
}

/// Pure classification of a durable path under `state/projects/<id>/docs/**`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocsPath {
    /// `docs/index.yaml`.
    Index(String),
    /// `docs/<path>.md`.
    Doc(String, DocPath),
    /// `docs/media/<file>`.
    MediaBlob(String, MediaFile),
    /// `docs/media/<file>.yaml`.
    MediaMeta(String, MediaFile),
    /// Unrecognized path under `docs/`.
    Foreign(String),
}

/// Parses a durable path under `state/projects/<uuidv7>/docs/**`; `None` outside it.
pub fn parse_docs_path(raw: &str) -> Option<DocsPath> {
    let parts: Vec<&str> = raw.split('/').collect();
    let ["state", "projects", project, "docs", rest @ ..] = parts.as_slice() else {
        return None;
    };
    if !org_proto::is_uuid_v7(project) {
        return None;
    }
    let pid = (*project).to_owned();
    Some(match rest {
        [] => DocsPath::Foreign(pid),
        [INDEX_FILE] => DocsPath::Index(pid),
        [MEDIA_FOLDER, name] => match name.strip_suffix(".yaml") {
            Some(file) => MediaFile::parse(file).map_or_else(
                |_| DocsPath::Foreign(pid.clone()),
                |f| DocsPath::MediaMeta(pid.clone(), f),
            ),
            None => MediaFile::parse(name).map_or_else(
                |_| DocsPath::Foreign(pid.clone()),
                |f| DocsPath::MediaBlob(pid.clone(), f),
            ),
        },
        _ => DocPath::parse(&rest.join("/")).map_or_else(
            |_| DocsPath::Foreign(pid.clone()),
            |p| DocsPath::Doc(pid.clone(), p),
        ),
    })
}
