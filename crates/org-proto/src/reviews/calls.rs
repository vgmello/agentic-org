//! The review calls (`review.*`), one per `agent-os review` verb. Every write carries a
//! canonical lowercase UUIDv7 `command_id` (a retry with the same id and request is answered
//! once) and acts as the employee bound to the connection. `review` names a review by id or
//! by its `RV-<OFFICE><N>` key.

use serde::{Deserialize, Serialize};

use super::kinds::{
    FindingStatus, ReviewSessionMode, ReviewStatus, ReviewTargetKind, ReviewVerdict,
};
use super::records::{
    ReviewDiffResult, ReviewFindingDraft, ReviewFindingView, ReviewReviewerView, ReviewRoundView,
    ReviewRoutingSpec, ReviewVerdictView, ReviewView,
};
use crate::Call;

/// `review.request`: open a review and its first round. A `code` review pins the task
/// worktree's head commit, refusing a dirty worktree.
pub struct ReviewRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRequestParams {
    pub command_id: String,
    /// Canonical lowercase UUIDv7 the caller mints for the new review.
    pub review_id: String,
    pub target_kind: ReviewTargetKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_markdown: String,
    pub kind_slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// `code`: the reviewed repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    /// `code`: the reviewed task branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// `doc`: the document's path or id; `artifact`: the attachment id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<String>,
    /// `code`: the commit round 1 is diffed against; the office uses the branch's recorded
    /// base when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_oid: Option<String>,
    /// `code`: the head the caller means to submit; refused when the worktree's head differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_head_oid: Option<String>,
    /// Reviewers must not have worked on the change, and always run fresh sessions.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub independent: bool,
    /// The office's routing for the review kind when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<ReviewRoutingSpec>,
    /// Defaults to `warm` (always `fresh` for an independent review).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<ReviewSessionMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guidance_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub criteria: Vec<String>,
}

/// The reply of `review.request` and `review.resubmit`: the review and its new round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRoundResult {
    pub review: ReviewView,
    pub round: ReviewRoundView,
}

impl Call for ReviewRequest {
    const METHOD: &'static str = "review.request";
    type Params = ReviewRequestParams;
    type Result = ReviewRoundResult;
}

/// `review.list`: reviews matching every given filter, most recently updated first.
pub struct ReviewList;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ReviewStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<ReviewTargetKind>,
    /// Only reviews this employee is (still) assigned to review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// At most this many reviews (up to `REVIEW_LIST_MAX_LIMIT`), after `offset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewListResult {
    pub reviews: Vec<ReviewView>,
    /// Every match, so a client pages with `offset + reviews.len() < total`.
    pub total: u64,
}

impl Call for ReviewList {
    const METHOD: &'static str = "review.list";
    type Params = ReviewListParams;
    type Result = ReviewListResult;
}

/// `review.inspect`: one review with its rounds, reviewers and findings.
pub struct ReviewInspect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewInspectParams {
    pub review: String,
    /// Optional server-side `--fields` allowlist pruning unrequested sections (`TASK-92.1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewInspectResult {
    pub review: ReviewView,
    pub rounds: Vec<ReviewRoundView>,
    pub reviewers: Vec<ReviewReviewerView>,
    pub findings: Vec<ReviewFindingView>,
}

impl Call for ReviewInspect {
    const METHOD: &'static str = "review.inspect";
    type Params = ReviewInspectParams;
    type Result = ReviewInspectResult;
}

/// `review.diff`: the bounded diff of a `code` review round, against the review's base or,
/// with `since_round`, against that earlier round's head.
pub struct ReviewDiff;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDiffParams {
    pub review: String,
    /// Defaults to the current round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
    /// An earlier round: diff from its head instead of the review's base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_round: Option<u32>,
    /// Only this repository-relative file or directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Only the per-file summary.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stat_only: bool,
    /// Context lines, at most `REVIEW_DIFF_MAX_CONTEXT_LINES`; Git's 3 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_lines: Option<u32>,
}

impl Call for ReviewDiff {
    const METHOD: &'static str = "review.diff";
    type Params = ReviewDiffParams;
    type Result = ReviewDiffResult;
}

/// `review.route`: replace a review's reviewers.
pub struct ReviewRoute;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRouteParams {
    pub command_id: String,
    pub review: String,
    pub expected_revision: u64,
    pub routing: ReviewRoutingSpec,
    /// Defaults to the review's mode (always `fresh` for an independent review).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<ReviewSessionMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRouteResult {
    pub review: ReviewView,
    pub reviewers: Vec<ReviewReviewerView>,
}

impl Call for ReviewRoute {
    const METHOD: &'static str = "review.route";
    type Params = ReviewRouteParams;
    type Result = ReviewRouteResult;
}

/// `review.finding`: raise a finding in the current round, or reply to one (optionally
/// resolving, declining or reopening it).
pub struct ReviewFinding;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingParams {
    pub command_id: String,
    pub review: String,
    pub op: ReviewFindingOp,
}

/// What `review.finding` does, tagged by `action`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ReviewFindingOp {
    /// Raise a new finding in the current round.
    Add(ReviewFindingDraft),
    /// Reply to a finding. `body_markdown` may be empty only when `set_status` is given.
    Reply {
        finding_id: String,
        /// Canonical lowercase UUIDv7 the caller mints for the reply.
        reply_id: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        body_markdown: String,
        /// `resolved`, `wont_fix`, or `open` to reopen.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        set_status: Option<FindingStatus>,
        /// Existing attachments the office binds to the reply (`review_finding_reply`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachment_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingResult {
    pub review: ReviewView,
    pub finding: ReviewFindingView,
}

impl Call for ReviewFinding {
    const METHOD: &'static str = "review.finding";
    type Params = ReviewFindingParams;
    type Result = ReviewFindingResult;
}

/// `review.verdict`: one reviewer's verdict on the current round, committed atomically with
/// the findings it raises. Refused unless `round_number` is the current round and, for a
/// `code` review, `head_oid` is that round's pinned head.
pub struct ReviewVerdictCall;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewVerdictParams {
    pub command_id: String,
    pub review: String,
    pub round_number: u32,
    /// `code`: the head commit the reviewer reviewed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_oid: Option<String>,
    pub verdict: ReviewVerdict,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary_markdown: String,
    /// At most `REVIEW_MAX_FINDINGS_PER_VERDICT`; all or none are recorded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<ReviewFindingDraft>,
    /// Existing attachments the summary cites; each must be readable by the reviewer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewVerdictResult {
    pub review: ReviewView,
    pub verdict: ReviewVerdictView,
    pub findings: Vec<ReviewFindingView>,
}

impl Call for ReviewVerdictCall {
    const METHOD: &'static str = "review.verdict";
    type Params = ReviewVerdictParams;
    type Result = ReviewVerdictResult;
}

/// `review.resubmit`: the author submits a new round. A `code` review pins the worktree's
/// new head, refusing a dirty worktree.
pub struct ReviewResubmit;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewResubmitParams {
    pub command_id: String,
    pub review: String,
    pub expected_revision: u64,
    /// `code`: the head the caller means to submit; refused when the worktree's head differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_head_oid: Option<String>,
    /// The reviewers' sessions for the new round; see [`Self::requested_session_mode`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<ReviewSessionMode>,
    /// Shorthand for `session_mode: fresh`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fresh: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note_markdown: String,
}

impl ReviewResubmitParams {
    /// The session mode asked for: `fresh` when `fresh` is set, else `session_mode` (`None`
    /// keeps the review's mode). `fresh` with `session_mode: warm` is contradictory.
    pub fn requested_session_mode(&self) -> Result<Option<ReviewSessionMode>, &'static str> {
        match (self.fresh, self.session_mode) {
            (true, Some(ReviewSessionMode::Warm)) => Err("fresh contradicts session_mode warm"),
            (true, _) => Ok(Some(ReviewSessionMode::Fresh)),
            (false, mode) => Ok(mode),
        }
    }
}

impl Call for ReviewResubmit {
    const METHOD: &'static str = "review.resubmit";
    type Params = ReviewResubmitParams;
    type Result = ReviewRoundResult;
}

/// `review.close`: close a review for good.
pub struct ReviewClose;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCloseParams {
    pub command_id: String,
    pub review: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewMutationResult {
    pub review: ReviewView,
}

impl Call for ReviewClose {
    const METHOD: &'static str = "review.close";
    type Params = ReviewCloseParams;
    type Result = ReviewMutationResult;
}
