//! The closed value sets of reviews: statuses, targets, session modes, verdicts, finding
//! severities and statuses, anchor kinds, routing modes and diff file statuses. Each has a
//! stable snake_case wire name ([`ReviewStatus::as_str`]) that its serde form matches.

use serde::{Deserialize, Serialize};

/// Declares a closed wire enum with `ALL`, `as_str` and an exact, case-sensitive `parse`.
macro_rules! wire_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $($(#[$vmeta:meta])* $variant:ident => $wire:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            /// Every value, in contract order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The stable snake_case name used on the wire and in records.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire,)+
                }
            }

            /// The value a wire name stands for (exact and case-sensitive).
            pub fn parse(raw: &str) -> Option<Self> {
                match raw {
                    $($wire => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

wire_enum! {
    /// Where a review stands.
    ReviewStatus {
        /// Waiting for verdicts on the current round.
        Open => "open",
        /// A reviewer asked for changes; the author resubmits a new round.
        ChangesRequested => "changes_requested",
        /// Approved on the current round.
        Approved => "approved",
        /// Closed for good (merged, abandoned or superseded); no further rounds.
        Closed => "closed",
    }
}

wire_enum! {
    /// What a review looks at.
    ReviewTargetKind {
        /// Commits of a task branch, pinned per round by commit OID.
        Code => "code",
        /// A document in the repository or the knowledge base.
        Doc => "doc",
        /// A task's outcome against its definition of done.
        Task => "task",
        /// An attachment (a file produced by the work).
        Artifact => "artifact",
    }
}

wire_enum! {
    /// How a reviewer's agent session runs for a round (default `warm`).
    #[derive(Default)]
    ReviewSessionMode {
        /// Continue the reviewer's session from the previous round (keeps its context).
        #[default]
        Warm => "warm",
        /// Start a new session with no memory of earlier rounds (independent re-review).
        Fresh => "fresh",
    }
}

wire_enum! {
    /// One reviewer's conclusion about one round.
    ReviewVerdict {
        Approve => "approve",
        ChangesRequested => "changes_requested",
        /// Feedback that neither approves nor blocks.
        Comment => "comment",
        /// The reviewer could not finish the round (for example missing access or context);
        /// the review stays open.
        Incomplete => "incomplete",
    }
}

wire_enum! {
    /// How much a finding matters.
    FindingSeverity {
        /// Must be resolved before the review can be approved.
        Blocker => "blocker",
        Major => "major",
        Minor => "minor",
        Nit => "nit",
        /// A question for the author rather than a defect.
        Question => "question",
    }
}

wire_enum! {
    /// Where a finding stands.
    FindingStatus {
        Open => "open",
        Resolved => "resolved",
        /// Acknowledged and deliberately left as is.
        WontFix => "wont_fix",
    }
}

wire_enum! {
    /// What a finding's anchor points at (default `general`).
    #[derive(Default)]
    AnchorKind {
        /// The review as a whole.
        #[default]
        General => "general",
        /// A line range of one file at the round's head.
        FileLine => "file_line",
        /// A section of a document.
        DocSection => "doc_section",
        /// One checklist item of the reviewed task.
        ChecklistItem => "checklist_item",
    }
}

wire_enum! {
    /// How reviewers are chosen.
    RoutingMode {
        /// The named reviewers (people or agents).
        Explicit => "explicit",
        /// The office picks a reviewer from one team's pool.
        Pool => "pool",
        /// One agent reviewer: the named one, or the office's default reviewer agent.
        SingleAgent => "single_agent",
    }
}

wire_enum! {
    /// How one file changed between two commits of a code review.
    ReviewDiffFileStatus {
        Added => "added",
        Modified => "modified",
        Deleted => "deleted",
        /// The entry changed kind (for example a file became a symbolic link).
        TypeChanged => "type_changed",
    }
}
