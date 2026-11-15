//! Revision coverage for selected task-history header claims, independent of payload codecs.

use std::collections::BTreeMap;
use std::fmt;

const MAX_SQLITE_REVISION: u64 = i64::MAX as u64;

/// Caller-supplied task-history header claims, not a validated or persisted event.
///
/// The caller must separately verify `payload_sha256` against the event payload and ensure
/// these claims describe that payload. Coverage checks only digest syntax/equality, not hashes.
/// Schema, actor, assignment, state, authentication, authorization and persistence checks are
/// out of scope. No filesystem path is inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskHistoryHeader<'a> {
    /// Opaque organization identity, compared exactly with the expected identity.
    pub organization_id: &'a str,
    /// Opaque task identity, compared exactly with the expected identity.
    pub task_id: &'a str,
    /// Revision claimed by the event, in `1..=i64::MAX`.
    pub revision: u64,
    /// Revision the caller extracted from the filename; this is a claim, not a path check.
    pub filename_revision: u64,
    /// Nonempty change tag. Only origin markers affect revision coverage.
    pub change_type: &'a str,
    /// Claimed payload digest: exactly 64 lowercase hexadecimal characters.
    pub payload_sha256: &'a str,
}

/// Explicit starting point for a task-history coverage check; never inferred from a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskHistoryOrigin {
    /// Require `created` at revision 1.
    Creation,
    /// Require `history_baseline` at this revision. Earlier history remains unavailable,
    /// even when `baseline_revision` is 1.
    Migration { baseline_revision: u64 },
}

/// Revision-coverage facts about supplied headers, not an event-validation or persistence receipt.
///
/// Input checksums need separate verification against their payloads. This result establishes
/// no schema, actor, assignment, reconstructed-state, authentication, authorization, persistence
/// or import/publication acceptance. It does not establish payload integrity or authenticity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskHistoryCoverage {
    /// Headers cover every revision from the claimed creation at 1 through `current_revision`.
    FromCreation {
        current_revision: u64,
        unique_revisions: usize,
    },
    /// Headers cover the baseline through `current_revision`; earlier history is unavailable.
    EarlierHistoryUnavailable {
        baseline_revision: u64,
        current_revision: u64,
        unique_revisions: usize,
    },
}

/// A failure of header claims or revision coverage, independent of payload verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskHistoryCoverageError {
    InvalidCurrentRevision {
        revision: u64,
    },
    InvalidBaselineRevision {
        revision: u64,
    },
    BaselineAfterCurrent {
        baseline_revision: u64,
        current_revision: u64,
    },
    InvalidEventRevision {
        header_index: usize,
        revision: u64,
    },
    InvalidFilenameRevision {
        header_index: usize,
        revision: u64,
    },
    OrganizationMismatch {
        header_index: usize,
    },
    TaskMismatch {
        header_index: usize,
    },
    FilenameRevisionMismatch {
        header_index: usize,
    },
    EmptyChangeType {
        header_index: usize,
    },
    InvalidChecksum {
        header_index: usize,
    },
    ConflictingRevision {
        revision: u64,
    },
    EmptyHistory,
    OriginRevisionMismatch {
        expected: u64,
        actual: u64,
    },
    OriginTagMismatch {
        revision: u64,
    },
    UnexpectedBaseline {
        revision: u64,
    },
    RevisionGap {
        previous_revision: u64,
        next_revision: u64,
    },
    FinalRevisionMismatch {
        expected: u64,
        actual: u64,
    },
}

impl fmt::Display for TaskHistoryCoverageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use TaskHistoryCoverageError::*;
        match self {
            InvalidCurrentRevision { revision } => {
                write!(f, "current revision {revision} is outside 1..=i64::MAX")
            }
            InvalidBaselineRevision { revision } => {
                write!(f, "baseline revision {revision} is outside 1..=i64::MAX")
            }
            BaselineAfterCurrent {
                baseline_revision,
                current_revision,
            } => write!(
                f,
                "baseline revision {baseline_revision} exceeds current revision {current_revision}"
            ),
            InvalidEventRevision {
                header_index,
                revision,
            } => write!(
                f,
                "header {header_index} event revision {revision} is outside 1..=i64::MAX"
            ),
            InvalidFilenameRevision {
                header_index,
                revision,
            } => write!(
                f,
                "header {header_index} filename revision {revision} is outside 1..=i64::MAX"
            ),
            OrganizationMismatch { header_index } => {
                write!(f, "header {header_index} has a different organization")
            }
            TaskMismatch { header_index } => {
                write!(f, "header {header_index} has a different task")
            }
            FilenameRevisionMismatch { header_index } => write!(
                f,
                "header {header_index} filename and event revisions disagree"
            ),
            EmptyChangeType { header_index } => {
                write!(f, "header {header_index} has an empty change tag")
            }
            InvalidChecksum { header_index } => write!(
                f,
                "header {header_index} checksum must be 64 lowercase hexadecimal characters"
            ),
            ConflictingRevision { revision } => {
                write!(f, "headers conflict at revision {revision}")
            }
            EmptyHistory => f.write_str("task history has no headers"),
            OriginRevisionMismatch { expected, actual } => write!(
                f,
                "history starts at revision {actual}, expected {expected}"
            ),
            OriginTagMismatch { revision } => {
                write!(f, "wrong change tag at history origin revision {revision}")
            }
            UnexpectedBaseline { revision } => write!(
                f,
                "unexpected later history baseline at revision {revision}"
            ),
            RevisionGap {
                previous_revision,
                next_revision,
            } => write!(
                f,
                "history has a gap between revisions {previous_revision} and {next_revision}"
            ),
            FinalRevisionMismatch { expected, actual } => {
                write!(f, "history ends at revision {actual}, expected {expected}")
            }
        }
    }
}

impl std::error::Error for TaskHistoryCoverageError {}

/// Check revision coverage of task-history header claims through a snapshot/tombstone revision.
///
/// The caller must independently verify every input checksum against its event payload and
/// ensure each header describes that payload. This function checks only header claims and
/// digest syntax/equality. Schema, actor, assignment, timestamp, reconstructed-state,
/// authentication, authorization, persistence and import/publication acceptance are out of scope.
/// In particular, success is neither a validated event nor a persisted-event receipt.
///
/// All revisions must be positive signed-SQLite integers. Every header is checked in input
/// order before retries are deduplicated, comparing all header facts as well as digests.
/// Input may be out of order; unique revisions must cover the explicit origin through exactly
/// `current_revision`. A later `history_baseline` is rejected, never used to hide a gap.
/// Unknown later nonempty tags are accepted. IDs are compared without normalization.
/// `filename_revision` is a caller-supplied claim; no real path is read or validated.
///
/// Inputs are not mutated. Time is O(n log n), with O(n) borrowed header storage; no work or
/// allocation is proportional to the numeric distance between revisions.
pub fn check_task_history_coverage(
    organization_id: &str,
    task_id: &str,
    current_revision: u64,
    origin: TaskHistoryOrigin,
    headers: &[TaskHistoryHeader<'_>],
) -> Result<TaskHistoryCoverage, TaskHistoryCoverageError> {
    use TaskHistoryCoverageError::*;

    if !valid_revision(current_revision) {
        return Err(InvalidCurrentRevision {
            revision: current_revision,
        });
    }
    let (first_revision, first_tag) = match origin {
        TaskHistoryOrigin::Creation => (1, "created"),
        TaskHistoryOrigin::Migration { baseline_revision } => {
            if !valid_revision(baseline_revision) {
                return Err(InvalidBaselineRevision {
                    revision: baseline_revision,
                });
            }
            if baseline_revision > current_revision {
                return Err(BaselineAfterCurrent {
                    baseline_revision,
                    current_revision,
                });
            }
            (baseline_revision, "history_baseline")
        }
    };

    // Complete header validation precedes all deduplication, including conflict checks.
    for (header_index, header) in headers.iter().enumerate() {
        if !valid_revision(header.revision) {
            return Err(InvalidEventRevision {
                header_index,
                revision: header.revision,
            });
        }
        if !valid_revision(header.filename_revision) {
            return Err(InvalidFilenameRevision {
                header_index,
                revision: header.filename_revision,
            });
        }
        if header.organization_id != organization_id {
            return Err(OrganizationMismatch { header_index });
        }
        if header.task_id != task_id {
            return Err(TaskMismatch { header_index });
        }
        if header.filename_revision != header.revision {
            return Err(FilenameRevisionMismatch { header_index });
        }
        if header.change_type.is_empty() {
            return Err(EmptyChangeType { header_index });
        }
        if header.payload_sha256.len() != 64
            || !header
                .payload_sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(InvalidChecksum { header_index });
        }
    }

    let mut unique = BTreeMap::new();
    for header in headers {
        if let Some(previous) = unique.insert(header.revision, header) {
            if previous != header {
                return Err(ConflictingRevision {
                    revision: header.revision,
                });
            }
        }
    }

    let mut ordered = unique.values();
    let first = ordered.next().ok_or(EmptyHistory)?;
    if first.revision != first_revision {
        return Err(OriginRevisionMismatch {
            expected: first_revision,
            actual: first.revision,
        });
    }
    if first.change_type != first_tag {
        return Err(OriginTagMismatch {
            revision: first_revision,
        });
    }
    let mut previous_revision = first.revision;
    for header in ordered {
        if header.revision.checked_sub(previous_revision) != Some(1) {
            return Err(RevisionGap {
                previous_revision,
                next_revision: header.revision,
            });
        }
        if header.change_type == "history_baseline" {
            return Err(UnexpectedBaseline {
                revision: header.revision,
            });
        }
        previous_revision = header.revision;
    }
    if previous_revision != current_revision {
        return Err(FinalRevisionMismatch {
            expected: current_revision,
            actual: previous_revision,
        });
    }

    Ok(match origin {
        TaskHistoryOrigin::Creation => TaskHistoryCoverage::FromCreation {
            current_revision,
            unique_revisions: unique.len(),
        },
        TaskHistoryOrigin::Migration { baseline_revision } => {
            TaskHistoryCoverage::EarlierHistoryUnavailable {
                baseline_revision,
                current_revision,
                unique_revisions: unique.len(),
            }
        }
    })
}

fn valid_revision(revision: u64) -> bool {
    (1..=MAX_SQLITE_REVISION).contains(&revision)
}
