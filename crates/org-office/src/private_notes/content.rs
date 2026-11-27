//! The private text of a note, plan or decision and its validation (`TASK-111` ADEC-1): the
//! kind, title (explicit or derived from the body), why, alternatives, normalized tags and
//! body. Every engine stores exactly what this module accepts.

use std::collections::BTreeSet;
use std::fmt;

use org_proto::private_notes::{
    PrivateNoteAddParams, PrivateNoteKind, PrivateNoteUpdateParams,
    PRIVATE_NOTE_DERIVED_TITLE_MAX_CHARS, PRIVATE_NOTE_MAX_CHARS, PRIVATE_NOTE_MAX_TAGS,
    PRIVATE_NOTE_RATIONALE_MAX_CHARS, PRIVATE_NOTE_TAG_MAX_LEN, PRIVATE_NOTE_TITLE_MAX_CHARS,
};
use org_proto::{ApiError, ErrorCode};

use super::PrivateNoteRecord;

/// The complete private content of an active note.
#[derive(Clone, PartialEq, Eq)]
pub struct NoteContent {
    pub kind: PrivateNoteKind,
    /// Always set by [`NoteContent::from_add`] and [`NotePatch::apply`]; only a note carried
    /// over from the task-only layout may still lack one.
    pub title: Option<String>,
    pub why: Option<String>,
    pub alternatives: Option<String>,
    /// Normalized, deduplicated and sorted.
    pub tags: Vec<String>,
    pub body_markdown: String,
}

impl fmt::Debug for NoteContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NoteContent")
            .field("kind", &self.kind)
            .field("title", &self.title.as_ref().map(|_| "<redacted>"))
            .field("why", &self.why.as_ref().map(|_| "<redacted>"))
            .field(
                "alternatives",
                &self.alternatives.as_ref().map(|_| "<redacted>"),
            )
            .field("tags", &format!("<{} redacted>", self.tags.len()))
            .field("body_markdown", &"<redacted>")
            .finish()
    }
}

impl NoteContent {
    /// Validates `tasks.notes.add` content; a blank or absent title is derived from the body.
    pub fn from_add(params: &PrivateNoteAddParams) -> Result<Self, ApiError> {
        let body_markdown = validate_note_body(&params.body_markdown)?;
        let title = match optional_title(params.title.as_deref())? {
            Some(title) => title,
            None => derive_title(&body_markdown),
        };
        Ok(Self {
            kind: params.kind,
            title: Some(title),
            why: rationale(params.why.as_deref(), "why")?,
            alternatives: rationale(params.alternatives.as_deref(), "alternatives")?,
            tags: normalize_tags(&params.tags)?,
            body_markdown,
        })
    }
}

/// A validated author edit. `None` keeps the stored value; `Some(None)` clears it (a cleared
/// title is derived again from the body).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct NotePatch {
    pub kind: Option<PrivateNoteKind>,
    pub title: Option<Option<String>>,
    pub why: Option<Option<String>>,
    pub alternatives: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
    pub body_markdown: Option<String>,
}

impl fmt::Debug for NotePatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let changed = |present: bool| present.then_some("<redacted>");
        f.debug_struct("NotePatch")
            .field("kind", &self.kind)
            .field("title", &changed(self.title.is_some()))
            .field("why", &changed(self.why.is_some()))
            .field("alternatives", &changed(self.alternatives.is_some()))
            .field("tags", &changed(self.tags.is_some()))
            .field("body_markdown", &changed(self.body_markdown.is_some()))
            .finish()
    }
}

impl NotePatch {
    /// Validates `tasks.notes.update`; a request that changes nothing is refused.
    pub fn from_update(params: &PrivateNoteUpdateParams) -> Result<Self, ApiError> {
        let patch = Self {
            kind: params.kind,
            title: params
                .title
                .as_deref()
                .map(|raw| optional_title(Some(raw)))
                .transpose()?,
            why: params
                .why
                .as_deref()
                .map(|raw| rationale(Some(raw), "why"))
                .transpose()?,
            alternatives: params
                .alternatives
                .as_deref()
                .map(|raw| rationale(Some(raw), "alternatives"))
                .transpose()?,
            tags: params.tags.as_deref().map(normalize_tags).transpose()?,
            body_markdown: params
                .body_markdown
                .as_deref()
                .map(validate_note_body)
                .transpose()?,
        };
        if patch == Self::default() {
            return Err(invalid(
                "a private note update must change at least one field",
            ));
        }
        Ok(patch)
    }

    /// The content `base` holds after this patch, or `None` when neither holds a body (a
    /// tombstone). A note left without a title takes one derived from its body.
    pub fn apply(&self, base: &PrivateNoteRecord) -> Option<NoteContent> {
        let body_markdown = self
            .body_markdown
            .clone()
            .or_else(|| base.body_markdown.clone())?;
        let title = self
            .title
            .clone()
            .unwrap_or_else(|| base.title.clone())
            .unwrap_or_else(|| derive_title(&body_markdown));
        Some(NoteContent {
            kind: self.kind.unwrap_or(base.kind),
            title: Some(title),
            why: self.why.clone().unwrap_or_else(|| base.why.clone()),
            alternatives: self
                .alternatives
                .clone()
                .unwrap_or_else(|| base.alternatives.clone()),
            tags: self.tags.clone().unwrap_or_else(|| base.tags.clone()),
            body_markdown,
        })
    }
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, message)
}

fn too_large(field: &str, max: usize) -> ApiError {
    ApiError::new(
        ErrorCode::TooLarge,
        format!("private note {field} exceeds {max} characters"),
    )
}

/// Validates and trims a private note body (`PN-01`).
pub fn validate_note_body(raw: &str) -> Result<String, ApiError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid("private note body_markdown cannot be empty"));
    }
    if trimmed.chars().count() > PRIVATE_NOTE_MAX_CHARS {
        return Err(too_large("body_markdown", PRIVATE_NOTE_MAX_CHARS));
    }
    Ok(trimmed.to_owned())
}

/// A trimmed one-line title of at most 200 characters; blank means "derive it".
fn optional_title(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(title) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    if title.chars().any(char::is_control) {
        return Err(invalid("a private note title is one line of text"));
    }
    if title.chars().count() > PRIVATE_NOTE_TITLE_MAX_CHARS {
        return Err(too_large("title", PRIVATE_NOTE_TITLE_MAX_CHARS));
    }
    Ok(Some(title.to_owned()))
}

/// A trimmed `why` or `alternatives` of at most 2,000 characters; blank means "none".
fn rationale(raw: Option<&str>, field: &str) -> Result<Option<String>, ApiError> {
    let Some(text) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    if text.chars().count() > PRIVATE_NOTE_RATIONALE_MAX_CHARS {
        return Err(too_large(field, PRIVATE_NOTE_RATIONALE_MAX_CHARS));
    }
    Ok(Some(text.to_owned()))
}

/// The title of an untitled note: its first line with text (Markdown heading marks dropped),
/// cut to 120 characters. A non-empty body always yields a non-empty title.
pub fn derive_title(body: &str) -> String {
    let lines = || body.lines().map(str::trim).filter(|line| !line.is_empty());
    let line = lines()
        .map(|line| line.trim_start_matches('#').trim())
        .find(|line| !line.is_empty())
        .or_else(|| lines().next())
        .unwrap_or("Untitled");
    let flat: String = line
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= PRIVATE_NOTE_DERIVED_TITLE_MAX_CHARS {
        return flat;
    }
    let mut cut: String = flat
        .chars()
        .take(PRIVATE_NOTE_DERIVED_TITLE_MAX_CHARS - 1)
        .collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// Normalizes one tag in the task-label grammar: trimmed, lowercase ASCII letters and digits
/// with `.` `_` `:` `/` `-` inside, 1..=64 bytes.
pub fn normalize_tag(raw: &str) -> Result<String, ApiError> {
    let tag = raw.trim().to_ascii_lowercase();
    let bytes = tag.as_bytes();
    let inner = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b".:_/-".contains(b);
    let edge = |b: Option<&u8>| b.is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if bytes.len() > PRIVATE_NOTE_TAG_MAX_LEN
        || !edge(bytes.first())
        || !edge(bytes.last())
        || !bytes.iter().all(inner)
    {
        return Err(invalid(
            "a private note tag is 1 to 64 lowercase letters or digits, with . _ : / - inside",
        ));
    }
    Ok(tag)
}

/// Normalizes, deduplicates and sorts at most eight tags.
pub fn normalize_tags(raw: &[String]) -> Result<Vec<String>, ApiError> {
    let tags = raw
        .iter()
        .map(|tag| normalize_tag(tag))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if tags.len() > PRIVATE_NOTE_MAX_TAGS {
        return Err(invalid("a private note carries at most 8 tags"));
    }
    Ok(tags.into_iter().collect())
}

/// Length-prefixed request bytes for a receipt digest: no value can run into the next.
#[derive(Default)]
pub struct DigestInput(Vec<u8>);

impl DigestInput {
    pub fn new(command: &str) -> Self {
        let mut input = Self::default();
        input.text(Some(command));
        input
    }

    /// Adds an optional text value; `None` and `Some("")` differ.
    pub fn text(&mut self, value: Option<&str>) -> &mut Self {
        match value {
            None => self.0.push(0),
            Some(text) => {
                self.0.push(1);
                self.0.extend_from_slice(&(text.len() as u64).to_be_bytes());
                self.0.extend_from_slice(text.as_bytes());
            }
        }
        self
    }

    pub fn number(&mut self, value: u64) -> &mut Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn tags(&mut self, tags: Option<&[String]>) -> &mut Self {
        match tags {
            None => self.0.push(0),
            Some(tags) => {
                self.0.push(1);
                self.number(tags.len() as u64);
                for tag in tags {
                    self.text(Some(tag));
                }
            }
        }
        self
    }

    /// The canonical lowercase hex SHA-256 the receipt stores.
    pub fn finish(&self) -> String {
        super::request_digest(&[&self.0])
    }
}

impl NoteContent {
    /// Adds every content value to a receipt digest.
    pub fn digest_into(&self, input: &mut DigestInput) {
        input
            .text(Some(self.kind.as_str()))
            .text(self.title.as_deref())
            .text(self.why.as_deref())
            .text(self.alternatives.as_deref())
            .tags(Some(&self.tags))
            .text(Some(&self.body_markdown));
    }
}

impl NotePatch {
    /// Adds every patched value (and which ones are absent) to a receipt digest.
    pub fn digest_into(&self, input: &mut DigestInput) {
        fn cleared(value: &Option<Option<String>>) -> Option<&str> {
            value.as_ref().map(|inner| inner.as_deref().unwrap_or(""))
        }
        input
            .text(self.kind.map(PrivateNoteKind::as_str))
            .text(cleared(&self.title))
            .text(cleared(&self.why))
            .text(cleared(&self.alternatives))
            .tags(self.tags.as_deref())
            .text(self.body_markdown.as_deref());
    }
}
