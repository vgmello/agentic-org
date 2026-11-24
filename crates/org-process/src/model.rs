//! Core v1 value types for organization processes and standards (`doc-34`, `doc-75`).

use org_docs::is_invisible_or_break;

/// What a structure document is (`doc-34` §2.1): how the organization handles something
/// (`process`), or what technologies/patterns it uses (`standard`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DocumentKind {
    /// How the organization handles a workflow or obligation.
    Process,
    /// Technologies, tools, and conventions the organization uses.
    Standard,
}

impl DocumentKind {
    /// Wire and front-matter spelling (`"process"` or `"standard"`).
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentKind::Process => "process",
            DocumentKind::Standard => "standard",
        }
    }

    /// Parses `"process"` or `"standard"`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "process" => Some(DocumentKind::Process),
            "standard" => Some(DocumentKind::Standard),
            _ => None,
        }
    }
}

/// Lifecycle state of a structure document (`doc-34` §2.1, `doc-75`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DocumentStatus {
    /// Not yet in force; excluded silently from effective sets.
    Draft,
    /// In force; requires an active accountable owner employee.
    Current,
    /// No longer in force; excluded silently from effective sets.
    Deprecated,
}

impl DocumentStatus {
    /// Wire and front-matter spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentStatus::Draft => "draft",
            DocumentStatus::Current => "current",
            DocumentStatus::Deprecated => "deprecated",
        }
    }

    /// Parses `"draft"`, `"current"`, or `"deprecated"`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "draft" => Some(DocumentStatus::Draft),
            "current" => Some(DocumentStatus::Current),
            "deprecated" => Some(DocumentStatus::Deprecated),
            _ => None,
        }
    }
}

/// Where a process or standard document lives in the single inheritance chain (`doc-34` §2.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    /// Organization-wide root (`team_id: null`, `project_id: null`).
    Organization,
    /// One team in the team forest (`team_id`, `project_id: null`).
    Team(String),
    /// One project (`team_id: null`, `project_id`).
    Project(String),
}

impl Scope {
    /// Level rank in inheritance order: Organization (0) < Team (1) < Project (2).
    pub fn level_rank(&self) -> u8 {
        match self {
            Scope::Organization => 0,
            Scope::Team(_) => 1,
            Scope::Project(_) => 2,
        }
    }

    /// Level name (`"organization"`, `"team"`, or `"project"`).
    pub fn level_str(&self) -> &'static str {
        match self {
            Scope::Organization => "organization",
            Scope::Team(_) => "team",
            Scope::Project(_) => "project",
        }
    }
}

/// Stable override key (`doc-34` §2.1, `doc-75`): 1..=64 lowercase ASCII letters, digits, or
/// single hyphens, starting and ending with a letter or digit.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessKey(String);

impl ProcessKey {
    /// Validates and constructs a [`ProcessKey`].
    pub fn new(raw: &str) -> Option<Self> {
        let bytes = raw.as_bytes();
        if bytes.is_empty() || bytes.len() > 64 {
            return None;
        }
        let is_alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
        if !is_alnum(bytes[0]) || !is_alnum(bytes[bytes.len() - 1]) {
            return None;
        }
        if !bytes.iter().all(|&b| is_alnum(b) || b == b'-') {
            return None;
        }
        if raw.contains("--") {
            return None;
        }
        Some(Self(raw.to_owned()))
    }

    /// Returns the validated key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One authored structure document fed into resolution, write checks, or rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureDocInput {
    /// Immutable UUIDv7 document identifier.
    pub id: String,
    /// Optional override key matched across levels.
    pub key: Option<ProcessKey>,
    /// `process` or `standard`.
    pub kind: DocumentKind,
    /// Human-readable title.
    pub title: String,
    /// Lifecycle state (`draft`, `current`, `deprecated`).
    pub status: DocumentStatus,
    /// Accountable owner employee ID (required for `current`).
    pub owner_employee_id: Option<String>,
    /// Owning scope (organization, team, or project).
    pub scope: Scope,
    /// When `false`, no lower level may replace this key (`doc-34` decision 4).
    pub overridable: bool,
    /// Monotonic document revision (`>= 1`).
    pub revision: u64,
    /// Markdown body text.
    pub body_markdown: String,
    /// Canonical relative path in organization storage.
    pub path: String,
    /// Hex SHA-256 of the document content for read tracking.
    pub blob_sha256: String,
    /// Set when front matter or path/scope validation failed (excluded with visible problem).
    pub invalid_field: Option<String>,
}

impl StructureDocInput {
    /// Whether this document is exempt from sealing and clashes (valid `draft` or `deprecated`).
    pub(crate) fn is_inactive(&self) -> bool {
        self.invalid_field.is_none()
            && matches!(
                self.status,
                DocumentStatus::Draft | DocumentStatus::Deprecated
            )
    }

    /// Whether this document seals its key against lower overrides (fails closed on invalid docs
    /// whose `overridable` flag is not `true`).
    pub(crate) fn seals_key(&self) -> bool {
        !self.is_inactive() && !self.overridable
    }
}

/// Produces a clean one-line summary (up to 160 chars) from a document's Markdown body.
pub fn summarize_body(body: &str) -> String {
    let mut fallback: Option<String> = None;
    for raw_line in body.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("```") {
            continue;
        }
        let cleaned = strip_control_and_invisible(trimmed);
        if cleaned.is_empty() {
            continue;
        }
        if let Some(heading) = cleaned.strip_prefix('#') {
            let text = heading.trim_start_matches('#').trim();
            if fallback.is_none() && !text.is_empty() {
                fallback = Some(truncate_chars(text, 160));
            }
            continue;
        }
        return truncate_chars(&cleaned, 160);
    }
    fallback.unwrap_or_default()
}

pub(crate) fn strip_control_and_invisible(s: &str) -> String {
    s.chars()
        .filter(|&c| !c.is_control() && !is_invisible_or_break(c))
        .collect::<String>()
        .trim()
        .to_owned()
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}
