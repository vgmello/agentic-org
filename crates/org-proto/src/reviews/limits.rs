//! Bounds of the review contract. Text bounds count Unicode scalar values, path and diff
//! bounds count bytes. The diff and slice bounds match the office's read-only Git helpers.

/// Prefix of a review's office-coded key, `RV-<OFFICE><N>` (for example `RV-A12`).
pub const REVIEW_KEY_PREFIX: &str = "RV-";
/// Longest review title.
pub const REVIEW_TITLE_MAX_CHARS: usize = 200;
/// Longest review description (Markdown).
pub const REVIEW_BODY_MAX_CHARS: usize = 64_000;
/// Longest `kind_slug` or `category_slug`, in ASCII bytes.
pub const REVIEW_SLUG_MAX_LEN: usize = 64;
/// Most acceptance criteria on one review.
pub const REVIEW_MAX_CRITERIA: usize = 64;
/// Longest acceptance criterion.
pub const REVIEW_CRITERION_MAX_CHARS: usize = 500;
/// Most guidance references (rules, specs, docs) on one review.
pub const REVIEW_MAX_GUIDANCE_REFS: usize = 32;
/// Longest guidance reference.
pub const REVIEW_GUIDANCE_REF_MAX_CHARS: usize = 512;
/// Most reviewers one routing names.
pub const REVIEW_MAX_REVIEWERS: usize = 16;
/// Longest finding body (Markdown).
pub const REVIEW_FINDING_BODY_MAX_CHARS: usize = 16_000;
/// Longest finding reply body (Markdown).
pub const REVIEW_REPLY_BODY_MAX_CHARS: usize = 16_000;
/// Longest verdict summary or resubmission note (Markdown).
pub const REVIEW_SUMMARY_MAX_CHARS: usize = 16_000;
/// Longest close reason.
pub const REVIEW_CLOSE_REASON_MAX_CHARS: usize = 500;
/// Most findings one verdict raises.
pub const REVIEW_MAX_FINDINGS_PER_VERDICT: usize = 100;
/// Most attachments on one finding, reply or verdict.
pub const REVIEW_MAX_ATTACHMENTS: usize = 16;
/// Longest repository-relative path in an anchor or a diff filter, in bytes.
pub const REVIEW_PATH_MAX_LEN: usize = 1024;
/// Longest document section name in an anchor.
pub const REVIEW_DOC_SECTION_MAX_CHARS: usize = 500;
/// Most lines one file-line anchor spans (the office's line-slice bound).
pub const REVIEW_ANCHOR_MAX_LINES: u32 = 500;
/// Largest unified diff text in a `review.diff` reply (256 KiB).
pub const REVIEW_DIFF_MAX_BYTES: usize = 256 * 1024;
/// Most context lines a `review.diff` may ask for.
pub const REVIEW_DIFF_MAX_CONTEXT_LINES: u32 = 20;
/// Most per-file entries in a `review.diff` reply.
pub const REVIEW_DIFF_MAX_FILES: usize = 1000;
/// Most reviews one `review.list` page returns.
pub const REVIEW_LIST_MAX_LIMIT: u32 = 200;
