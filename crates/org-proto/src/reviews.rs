//! Wire types and typed calls for reviews (`TASK-92.1`): asking for a review of code, a
//! document, a task or an artifact; routing it to reviewers; reading a code round's bounded
//! diff; findings anchored to lines, sections or checklist items; round-pinned verdicts with
//! an atomic batch of findings; resubmitted rounds; and closing.
//!
//! A review moves through numbered rounds. A `code` round pins one commit of the task
//! branch (never a dirty worktree), so every verdict names the exact commit it judged; a
//! later round's diff can be read against the review's base or an earlier round's head.
//! Each verb of `agent-os review` is one call here; [`REVIEW_METHODS`] lists them.

mod calls;
mod inspect_fields;
mod kinds;
mod limits;
mod records;
#[cfg(test)]
mod tests;

pub use calls::{
    ReviewClose, ReviewCloseParams, ReviewDiff, ReviewDiffParams, ReviewFinding, ReviewFindingOp,
    ReviewFindingParams, ReviewFindingResult, ReviewInspect, ReviewInspectParams,
    ReviewInspectResult, ReviewList, ReviewListParams, ReviewListResult, ReviewMutationResult,
    ReviewRequest, ReviewRequestParams, ReviewResubmit, ReviewResubmitParams, ReviewRoundResult,
    ReviewRoute, ReviewRouteParams, ReviewRouteResult, ReviewVerdictCall, ReviewVerdictParams,
    ReviewVerdictResult,
};
pub use inspect_fields::ReviewInspectSelection;
pub use kinds::{
    AnchorKind, FindingSeverity, FindingStatus, ReviewDiffFileStatus, ReviewSessionMode,
    ReviewStatus, ReviewTargetKind, ReviewVerdict, RoutingMode,
};
pub use limits::{
    REVIEW_ANCHOR_MAX_LINES, REVIEW_BODY_MAX_CHARS, REVIEW_CLOSE_REASON_MAX_CHARS,
    REVIEW_CRITERION_MAX_CHARS, REVIEW_DIFF_MAX_BYTES, REVIEW_DIFF_MAX_CONTEXT_LINES,
    REVIEW_DIFF_MAX_FILES, REVIEW_DOC_SECTION_MAX_CHARS, REVIEW_FINDING_BODY_MAX_CHARS,
    REVIEW_GUIDANCE_REF_MAX_CHARS, REVIEW_KEY_PREFIX, REVIEW_LIST_MAX_LIMIT,
    REVIEW_MAX_ATTACHMENTS, REVIEW_MAX_CRITERIA, REVIEW_MAX_FINDINGS_PER_VERDICT,
    REVIEW_MAX_GUIDANCE_REFS, REVIEW_MAX_REVIEWERS, REVIEW_PATH_MAX_LEN,
    REVIEW_REPLY_BODY_MAX_CHARS, REVIEW_SLUG_MAX_LEN, REVIEW_SUMMARY_MAX_CHARS,
    REVIEW_TITLE_MAX_CHARS,
};
pub use records::{
    FindingAnchor, ReviewDiffFile, ReviewDiffResult, ReviewFindingDraft, ReviewFindingReplyView,
    ReviewFindingView, ReviewReviewerView, ReviewRoundView, ReviewRoutingSpec, ReviewVerdictView,
    ReviewView,
};

use crate::Call;

/// Every review method, in the order of the `agent-os review` verbs.
pub const REVIEW_METHODS: [&str; 9] = [
    ReviewRequest::METHOD,
    ReviewList::METHOD,
    ReviewInspect::METHOD,
    ReviewDiff::METHOD,
    ReviewRoute::METHOD,
    ReviewFinding::METHOD,
    ReviewVerdictCall::METHOD,
    ReviewResubmit::METHOD,
    ReviewClose::METHOD,
];
