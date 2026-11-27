//! Pure project documentation tree rules: validated document/section/media paths,
//! document status and media kind enums, deterministic YAML/JSON encoding, and canonical
//! `docs/index.yaml` rendering (`TASK-55`, `FC-180`, `PKG-11/12/14`).

use std::collections::BTreeMap;
use std::fmt;

use org_proto::docs::{
    DOC_BODY_MAX_BYTES, DOC_KIND_MAX_BYTES, DOC_PATH_MAX_BYTES, DOC_SECTION_MAX_BYTES,
    DOC_SUMMARY_MAX_BYTES,
};
use serde_json::{json, Map, Value};

use crate::is_invisible_or_break;

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

const INDEX_HEADER: &str =
    "# Maintained by the office: regenerated on every documentation change. Do not edit.\n";
const INDEX_SCHEMA_VERSION: u64 = 1;
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

fn write_canonical_value(out: &mut String, value: &Value) {
    match value {
        Value::Object(fields) => {
            out.push('{');
            let mut entries: Vec<_> = fields.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            for (i, (key, val)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push(':');
                write_canonical_value(out, val);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical_value(out, item);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// Deterministic compact JSON with object keys sorted in byte order.
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical_value(&mut out, value);
    out
}

/// Encodes a flat mapping as deterministic YAML (`key: <canonical JSON>\n`, keys in byte order).
pub fn yaml_encode(fields: &Map<String, Value>) -> Vec<u8> {
    let mut entries: Vec<_> = fields.iter().collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut out = String::new();
    for (key, value) in entries {
        out.push_str(key);
        out.push_str(": ");
        write_canonical_value(&mut out, value);
        out.push('\n');
    }
    out.into_bytes()
}

/// Encodes front matter plus Markdown body between `---` fences.
pub fn yaml_encode_document(fields: &Map<String, Value>, body: &str) -> Vec<u8> {
    let mut out = b"---\n".to_vec();
    out.extend(yaml_encode(fields));
    out.extend(b"---\n");
    out.extend(body.as_bytes());
    out
}

/// Borrowed document metadata row for deterministic `docs/index.yaml` rendering.
#[derive(Debug, Clone, Copy)]
pub struct IndexDocRow<'a> {
    /// Document UUIDv7.
    pub id: &'a str,
    /// Validated path relative to `docs/`.
    pub path: &'a DocPath,
    /// Document kind slug.
    pub kind: &'a str,
    /// Document title.
    pub title: &'a str,
    /// Document lifecycle status.
    pub status: DocStatus,
    /// Optional summary.
    pub summary: Option<&'a str>,
    /// Optional owner employee UUIDv7.
    pub owner_employee_id: Option<&'a str>,
    /// Current revision.
    pub revision: u64,
}

/// Borrowed media metadata row for deterministic `docs/index.yaml` rendering.
#[derive(Debug, Clone, Copy)]
pub struct IndexMediaRow<'a> {
    /// Media UUIDv7.
    pub id: &'a str,
    /// Validated media file name.
    pub file: &'a MediaFile,
    /// Media kind (`image` or `diagram`).
    pub kind: MediaKind,
    /// MIME type.
    pub media_type: &'a str,
    /// Byte length.
    pub bytes: u64,
    /// Lowercase SHA-256 hex digest.
    pub sha256: &'a str,
    /// Media title.
    pub title: &'a str,
    /// Current revision.
    pub revision: u64,
}

/// Deterministically renders `docs/index.yaml` for a project's documents and media.
pub fn render_project_index(
    project_id: &str,
    docs: &[IndexDocRow<'_>],
    media: &[IndexMediaRow<'_>],
) -> Vec<u8> {
    let mut by_path: Vec<&IndexDocRow<'_>> = docs.iter().collect();
    by_path.sort_by(|a, b| a.path.cmp(b.path));
    let mut media_by_path: Vec<&IndexMediaRow<'_>> = media.iter().collect();
    media_by_path.sort_by(|a, b| a.file.cmp(b.file));
    let mut sections: BTreeMap<&str, u64> = BTreeMap::new();
    for doc in &by_path {
        let section = doc.path.as_str().rsplit_once('/').map_or("", |(s, _)| s);
        *sections.entry(section).or_default() += 1;
    }
    let mut fields = Map::new();
    fields.insert("schema_version".into(), json!(INDEX_SCHEMA_VERSION));
    fields.insert("project_id".into(), json!(project_id));
    fields.insert(
        "sections".into(),
        Value::Array(
            sections
                .into_iter()
                .map(|(path, count)| json!({"path": path, "document_count": count}))
                .collect(),
        ),
    );
    fields.insert(
        "documents".into(),
        Value::Array(
            by_path
                .into_iter()
                .map(|doc| {
                    json!({
                        "id": doc.id,
                        "path": doc.path.as_str(),
                        "section": doc.path.section().as_str(),
                        "kind": doc.kind,
                        "title": doc.title,
                        "status": doc.status.as_str(),
                        "summary": doc.summary,
                        "owner_employee_id": doc.owner_employee_id,
                        "revision": doc.revision,
                    })
                })
                .collect(),
        ),
    );
    fields.insert(
        "media".into(),
        Value::Array(
            media_by_path
                .into_iter()
                .map(|m| {
                    json!({
                        "id": m.id,
                        "path": m.file.rel_path(),
                        "type": m.kind.as_str(),
                        "media_type": m.media_type,
                        "bytes": m.bytes,
                        "sha256": m.sha256,
                        "title": m.title,
                        "revision": m.revision,
                    })
                })
                .collect(),
        ),
    );
    let mut out = INDEX_HEADER.as_bytes().to_vec();
    out.extend(yaml_encode(&fields));
    out
}
