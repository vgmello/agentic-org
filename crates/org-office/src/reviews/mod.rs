//! Universal reviews domain family (`TASK-92.1`, ADR-0026, ADR-0028, ADR-0031).
//!
//! Cohesive vertical-slice domain module decoupled from sibling families (`Ce = 0` to `tasks`,
//! `organization`, `projects`, `project_docs`, `attachments`, `messaging`) via
//! [`ports::ReviewContextRead`]. Runs identically on `MemoryEngine` and `SqliteEngine` (`ENG-1`).

pub mod features;
pub mod files;
pub mod ports;
pub mod records;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
#[cfg(test)]
mod tests;
pub mod validation;

pub use features::ReviewInspectSections;
pub use files::ReviewFiles;
pub use ports::{
    ReviewActorInfo, ReviewContextRead, ReviewListQuery, ReviewProjectInfo, ReviewTaskInfo,
    ReviewsEngine, ReviewsRead, ReviewsWrite,
};
pub use records::{
    compute_review_status, parse_review_key, OfficeReviewCounterRecord, ResolvedCodePin,
    ReviewDocument, ReviewEventRecord, ReviewFindingRecord, ReviewFindingReplyRecord, ReviewRecord,
    ReviewReviewerRecord, ReviewRoundRecord,
};
