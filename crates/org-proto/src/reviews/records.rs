//! Review wire records: the review, its rounds, reviewers, verdicts, findings and replies,
//! finding anchors and drafts, routing, and the bounded diff of a code review round.

use serde::{Deserialize, Serialize};

use super::kinds::{
    AnchorKind, FindingSeverity, FindingStatus, ReviewDiffFileStatus, ReviewSessionMode,
    ReviewStatus, ReviewTargetKind, ReviewVerdict, RoutingMode,
};
use super::limits::{
    REVIEW_ANCHOR_MAX_LINES, REVIEW_DOC_SECTION_MAX_CHARS, REVIEW_MAX_REVIEWERS,
    REVIEW_PATH_MAX_LEN,
};

/// Where a finding points. Which fields a kind takes is checked by [`FindingAnchor::check`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingAnchor {
    pub kind: AnchorKind,
    /// `file_line`: the repository-relative file; `doc_section`: the document, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `file_line`: first anchored line, counted from 1, at the round's head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    /// `file_line`: last anchored line (inclusive); `start_line` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    /// `file_line`: lowercase hex SHA-256 of the anchored lines at the round's head, each
    /// without its line terminator and followed by `\n`. The office sets it when the
    /// finding is recorded, so a later round can tell whether those lines changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_snippet_sha256: Option<String>,
    /// `doc_section`: the section heading or identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_section: Option<String>,
    /// `checklist_item`: the id of the task checklist item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checklist_item_id: Option<String>,
}

impl FindingAnchor {
    /// An anchor on the review as a whole.
    pub fn general() -> Self {
        Self::default()
    }

    /// An anchor on lines `start_line..=end_line` of `path`.
    pub fn file_lines(path: impl Into<String>, start_line: u32, end_line: u32) -> Self {
        Self {
            kind: AnchorKind::FileLine,
            path: Some(path.into()),
            start_line: Some(start_line),
            end_line: Some(end_line),
            ..Self::default()
        }
    }

    /// The last anchored line of a `file_line` anchor (`end_line`, else `start_line`).
    pub fn last_line(&self) -> Option<u32> {
        self.end_line.or(self.start_line)
    }

    /// Checks the anchor's shape for its kind, answering why it is malformed:
    /// - `general`: no other field.
    /// - `file_line`: `path` (1 to [`REVIEW_PATH_MAX_LEN`] bytes, relative, no control
    ///   characters), `start_line >= 1`, `end_line >= start_line`, at most
    ///   [`REVIEW_ANCHOR_MAX_LINES`] lines, an optional 64-hex lowercase snippet hash.
    /// - `doc_section`: `doc_section` (1 to [`REVIEW_DOC_SECTION_MAX_CHARS`]), optional `path`.
    /// - `checklist_item`: `checklist_item_id` only.
    ///
    /// The office still validates a `file_line` path against the commit before reading it.
    pub fn check(&self) -> Result<(), &'static str> {
        let lines = self.start_line.is_some() || self.end_line.is_some();
        let hash = self.anchor_snippet_sha256.is_some();
        match self.kind {
            AnchorKind::General => {
                if self.path.is_some()
                    || lines
                    || hash
                    || self.doc_section.is_some()
                    || self.checklist_item_id.is_some()
                {
                    return Err("a general anchor takes no other field");
                }
            }
            AnchorKind::FileLine => {
                let path = self
                    .path
                    .as_deref()
                    .ok_or("a file_line anchor needs a path")?;
                check_path(path)?;
                let start = self
                    .start_line
                    .ok_or("a file_line anchor needs start_line")?;
                let end = self.last_line().unwrap_or(start);
                if start == 0 {
                    return Err("start_line counts from 1");
                }
                if end < start {
                    return Err("end_line must not precede start_line");
                }
                if end - start >= REVIEW_ANCHOR_MAX_LINES {
                    return Err("a file_line anchor spans too many lines");
                }
                if let Some(sha) = &self.anchor_snippet_sha256 {
                    if !is_sha256_hex(sha) {
                        return Err("anchor_snippet_sha256 must be 64 lowercase hex characters");
                    }
                }
                if self.doc_section.is_some() || self.checklist_item_id.is_some() {
                    return Err("a file_line anchor takes no section or checklist item");
                }
            }
            AnchorKind::DocSection => {
                let section = self
                    .doc_section
                    .as_deref()
                    .ok_or("a doc_section anchor needs doc_section")?;
                let chars = section.chars().count();
                if chars == 0 || chars > REVIEW_DOC_SECTION_MAX_CHARS {
                    return Err("doc_section is empty or too long");
                }
                if let Some(path) = &self.path {
                    check_path(path)?;
                }
                if lines || hash || self.checklist_item_id.is_some() {
                    return Err("a doc_section anchor takes no lines or checklist item");
                }
            }
            AnchorKind::ChecklistItem => {
                if self.checklist_item_id.as_deref().is_none_or(str::is_empty) {
                    return Err("a checklist_item anchor needs checklist_item_id");
                }
                if self.path.is_some() || lines || hash || self.doc_section.is_some() {
                    return Err("a checklist_item anchor takes no other field");
                }
            }
        }
        Ok(())
    }
}

fn check_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() || path.len() > REVIEW_PATH_MAX_LEN {
        return Err("anchor path is empty or too long");
    }
    if path.starts_with(['/', '-']) || path.chars().any(char::is_control) {
        return Err("anchor path must be relative, without control characters");
    }
    if path
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err("anchor path must be normalized");
    }
    Ok(())
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A finding raised by `review.finding` (`add`) or in a `review.verdict` batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingDraft {
    /// Canonical lowercase UUIDv7 the caller mints for the new finding.
    pub finding_id: String,
    pub severity: FindingSeverity,
    /// Defaults to a `general` anchor.
    #[serde(default)]
    pub anchor: FindingAnchor,
    pub body_markdown: String,
    /// Existing attachments the caller may read; the office binds each to the new finding
    /// (`review_finding`) in the same commit, as a re-attach.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
}

/// How reviewers are chosen. Which fields a mode takes is checked by
/// [`ReviewRoutingSpec::check`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRoutingSpec {
    pub mode: RoutingMode,
    /// `explicit`: 1 to [`REVIEW_MAX_REVIEWERS`] employees; `single_agent`: at most one agent
    /// (none means the office's default reviewer agent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewer_ids: Vec<String>,
    /// `pool`: the team whose reviewer pool the office picks from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
}

impl ReviewRoutingSpec {
    /// Checks the routing's shape for its mode, answering why it is malformed.
    pub fn check(&self) -> Result<(), &'static str> {
        let mut seen = std::collections::BTreeSet::new();
        if self
            .reviewer_ids
            .iter()
            .any(|id| id.is_empty() || !seen.insert(id))
        {
            return Err("reviewer_ids must be distinct and non-empty");
        }
        match self.mode {
            RoutingMode::Explicit => {
                if self.reviewer_ids.is_empty() || self.reviewer_ids.len() > REVIEW_MAX_REVIEWERS {
                    return Err("explicit routing names 1 to REVIEW_MAX_REVIEWERS reviewers");
                }
                if self.team_id.is_some() {
                    return Err("explicit routing takes no team_id");
                }
            }
            RoutingMode::Pool => {
                if self.team_id.as_deref().is_none_or(str::is_empty) {
                    return Err("pool routing needs team_id");
                }
                if !self.reviewer_ids.is_empty() {
                    return Err("pool routing takes no reviewer_ids");
                }
            }
            RoutingMode::SingleAgent => {
                if self.reviewer_ids.len() > 1 {
                    return Err("single_agent routing names at most one reviewer");
                }
                if self.team_id.is_some() {
                    return Err("single_agent routing takes no team_id");
                }
            }
        }
        Ok(())
    }
}

/// A review: what is reviewed, against which guidance and criteria, and where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewView {
    pub id: String,
    /// Office-coded reference `RV-<OFFICE><N>` (for example `RV-A12`).
    pub key: String,
    pub office_code: String,
    pub number: u64,
    pub organization_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The task the reviewed work belongs to (the reviewed task for a `task` review).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub target_kind: ReviewTargetKind,
    /// `code`: the reviewed repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    /// `code`: the reviewed task branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// `doc`: the document's path or id; `artifact`: the attachment id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<String>,
    /// The kind of review (for example `code-review`, `security-review`).
    pub kind_slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_slug: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_markdown: String,
    /// Reviewers must not have worked on the reviewed change, and run fresh sessions.
    #[serde(default)]
    pub independent: bool,
    pub status: ReviewStatus,
    /// The round verdicts apply to, from 1.
    pub current_round: u32,
    /// `code`: the branch moved past the current round's pinned head without a resubmission.
    #[serde(default)]
    pub pin_stale: bool,
    /// Rules, specs and documents the reviewers review against.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guidance_refs: Vec<String>,
    /// Acceptance criteria the reviewers check.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub criteria: Vec<String>,
    /// Findings still `open`.
    #[serde(default)]
    pub open_findings: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_reason: Option<String>,
    pub revision: u64,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

/// One reviewer's verdict on one round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewVerdictView {
    pub reviewer_id: String,
    pub round_number: u32,
    pub verdict: ReviewVerdict,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary_markdown: String,
    /// Findings raised with this verdict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finding_ids: Vec<String>,
    /// Attachments the summary cites.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    pub submitted_at: String,
}

/// One round: the pinned state reviewers judge, and their verdicts on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRoundView {
    pub round_number: u32,
    /// `code`: the commit the round is diffed against (the review's base).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_oid: Option<String>,
    /// `code`: the round's pinned head commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_oid: Option<String>,
    pub session_mode: ReviewSessionMode,
    pub submitted_by: String,
    pub submitted_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note_markdown: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verdicts: Vec<ReviewVerdictView>,
}

/// One reviewer assigned to a review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewReviewerView {
    pub reviewer_id: String,
    pub routing: RoutingMode,
    /// `pool`: the team the reviewer was picked from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub session_mode: ReviewSessionMode,
    pub assigned_by: String,
    pub assigned_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verdict: Option<ReviewVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verdict_round: Option<u32>,
    /// Unassigned by a later routing; kept for history.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
}

/// One reply on a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingReplyView {
    pub id: String,
    pub finding_id: String,
    pub author_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_markdown: String,
    /// The status this reply gave the finding, when it changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_status: Option<FindingStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    pub created_at: String,
}

/// One finding and its replies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingView {
    pub id: String,
    pub review_id: String,
    /// Finding number within the review, from 1.
    pub number: u64,
    /// The round the finding was raised in.
    pub round_number: u32,
    pub severity: FindingSeverity,
    pub status: FindingStatus,
    pub anchor: FindingAnchor,
    /// `file_line`: the anchored lines differ at the current round's head.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub anchor_stale: bool,
    pub body_markdown: String,
    pub author_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_in_round: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<ReviewFindingReplyView>,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

/// One changed file in a code review diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDiffFile {
    pub path: String,
    pub status: ReviewDiffFileStatus,
    /// Lines added (`0` for a binary file).
    pub added: u64,
    /// Lines deleted (`0` for a binary file).
    pub deleted: u64,
    /// No line counts and no text: Git treats the file as binary.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub binary: bool,
}

/// The bounded diff of one code review round: `base_oid..head_oid`, where the base is the
/// review's base, or the head of `since_round` for an inter-round diff.
///
/// `unified_diff` holds at most [`super::limits::REVIEW_DIFF_MAX_BYTES`] bytes and `files` at most
/// [`super::limits::REVIEW_DIFF_MAX_FILES`] entries. The office keeps the encoded reply within
/// [`crate::MAX_FRAME_BYTES`]: when escaping would overflow it, it cuts `unified_diff`
/// further (on a character boundary) and then `files`, and marks `truncated` and
/// `files_truncated`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDiffResult {
    pub review_id: String,
    pub round_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_round: Option<u32>,
    pub base_oid: String,
    pub head_oid: String,
    pub files: Vec<ReviewDiffFile>,
    /// Every changed file, including those past the `files` bound.
    pub total_files: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub files_truncated: bool,
    /// Empty for a summary-only diff.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unified_diff: String,
    /// `unified_diff` was cut short.
    #[serde(default)]
    pub truncated: bool,
}
