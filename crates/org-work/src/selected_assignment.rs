//! Pure planning for the selected task assignment contract (organization-data document 12).
//!
//! Assignment is a complete replacement of the independent assigned-team/employee pair.
//! All four nullable states are valid, including for projectless tasks. Accountable ownership,
//! project/scope, readable number, reporter, permissions and file location are outside this API.
//! IDs are opaque: nonempty task, organization and desired target IDs are preserved byte-for-byte,
//! without trimming, UUID/path validation or home-team inference. Old pair values are historical
//! and require no lookup, even when inactive or no longer resolvable.
//!
//! A successful [`plan_assignment`] returns one change even for an unchanged pair. It does not
//! accept a write. The accepting service must authenticate/authorize, replay the original result
//! of same-operation retries before calling this function, and resolve trusted target facts
//! consistently with its acceptance operation. It must enforce the accepting writer and fencing,
//! preserve creation/reporter metadata, supply actual updater/actor/time attribution, and commit
//! the task snapshot, full event and idempotency receipt as one recoverable operation before ACK.
//! Hashing, integrity validation and publication also remain the caller's responsibility. A plan
//! is neither a complete attributed event nor proof of authorization, persistence or integrity.
//!
//! This module is independent of legacy task commands, storage ports and runtime adapters.

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::selected_codec::{self as codec, Json, NumberToken};

/// Immutable command scope, supplied by authenticated registry/channel context.
/// This is business retry identity, not an authorization token or event identity.
pub struct AssignmentScope<'a> {
    pub instance_id: &'a str,
    pub organization_id: &'a str,
    pub office_id: &'a str,
    pub task_id: &'a str,
}

/// Versioned typed retry fingerprint. Equivalent decoded string escapes and
/// native/browser revision adaptations yield the same hash. Actor/account binding
/// remains separate protected receipt metadata. Historical event-number spellings
/// never participate in this command encoding.
pub fn assignment_fingerprint(
    scope: AssignmentScope<'_>,
    command: &AssignmentCommand,
) -> Result<String, FingerprintError> {
    if [
        scope.instance_id,
        scope.organization_id,
        scope.office_id,
        scope.task_id,
    ]
    .iter()
    .any(|s| s.is_empty())
        || command.expected_revision <= 0
        || command
            .assigned_team_id
            .as_ref()
            .is_some_and(|s| s.is_empty())
        || command.assignee_id.as_ref().is_some_and(|s| s.is_empty())
    {
        return Err(FingerprintError::InvalidIntent);
    }
    let string = |s: &str| Json::String(s.to_owned());
    let nullable = |s: Option<&str>| s.map(string).unwrap_or(Json::Null);
    let value = Json::Object(
        [
            (
                "fingerprint_version".into(),
                Json::Number(NumberToken::new("1")?),
            ),
            ("method".into(), string("task.set_assignment")),
            ("instance_id".into(), string(scope.instance_id)),
            ("organization_id".into(), string(scope.organization_id)),
            ("office_id".into(), string(scope.office_id)),
            ("task_id".into(), string(scope.task_id)),
            (
                "expected_revision".into(),
                Json::Number(NumberToken::new(&command.expected_revision.to_string())?),
            ),
            (
                "assigned_team_id".into(),
                nullable(command.assigned_team_id.as_deref()),
            ),
            (
                "assignee_id".into(),
                nullable(command.assignee_id.as_deref()),
            ),
        ]
        .into(),
    );
    Ok(codec::sha256(&codec::canonical(&value)?))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FingerprintError {
    InvalidIntent,
    Codec(codec::Error),
}

impl From<codec::Error> for FingerprintError {
    fn from(value: codec::Error) -> Self {
        Self::Codec(value)
    }
}

/// A current or desired assignment pair. Serialization always includes both nullable keys.
///
/// Construction does not validate historical references. Desired IDs are validated by
/// [`plan_assignment`], including when identical to the historical pair.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AssignmentPair {
    assigned_team_id: Option<String>,
    assignee_id: Option<String>,
}

impl AssignmentPair {
    pub fn new(assigned_team_id: Option<String>, assignee_id: Option<String>) -> Self {
        Self {
            assigned_team_id,
            assignee_id,
        }
    }

    pub fn assigned_team_id(&self) -> Option<&str> {
        self.assigned_team_id.as_deref()
    }

    pub fn assignee_id(&self) -> Option<&str> {
        self.assignee_id.as_deref()
    }
}

/// Immutable input to the decision; resolving task identity belongs to the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskAssignmentSnapshot {
    pub organization_id: String,
    pub task_id: String,
    pub revision: i64,
    pub assignment: AssignmentPair,
}

/// Complete desired replacement. JSON must be an object containing exactly these three keys.
///
/// Each assignment field is a string or explicit null; absence is an error, never a patch or
/// clearing alias. Duplicate/unknown fields, noninteger/out-of-i64 revisions and malformed
/// values fail decoding. Positive revisions and nonempty IDs are checked by [`plan_assignment`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AssignmentCommand {
    pub expected_revision: i64,
    pub assigned_team_id: Option<String>,
    pub assignee_id: Option<String>,
}

impl<'de> Deserialize<'de> for AssignmentCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CommandVisitor;

        impl<'de> Visitor<'de> for CommandVisitor {
            type Value = AssignmentCommand;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .write_str("an object with expected_revision, assigned_team_id and assignee_id")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut expected_revision = None;
                // Outer Some records key presence; inner None records an explicit null.
                let mut assigned_team_id: Option<Option<String>> = None;
                let mut assignee_id: Option<Option<String>> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "expected_revision" => {
                            if expected_revision.is_some() {
                                return Err(de::Error::duplicate_field("expected_revision"));
                            }
                            expected_revision = Some(map.next_value()?);
                        }
                        "assigned_team_id" => {
                            if assigned_team_id.is_some() {
                                return Err(de::Error::duplicate_field("assigned_team_id"));
                            }
                            assigned_team_id = Some(map.next_value()?);
                        }
                        "assignee_id" => {
                            if assignee_id.is_some() {
                                return Err(de::Error::duplicate_field("assignee_id"));
                            }
                            assignee_id = Some(map.next_value()?);
                        }
                        _ => {
                            return Err(de::Error::unknown_field(
                                &key,
                                &["expected_revision", "assigned_team_id", "assignee_id"],
                            ));
                        }
                    }
                }
                Ok(AssignmentCommand {
                    expected_revision: expected_revision
                        .ok_or_else(|| de::Error::missing_field("expected_revision"))?,
                    assigned_team_id: assigned_team_id
                        .ok_or_else(|| de::Error::missing_field("assigned_team_id"))?,
                    assignee_id: assignee_id
                        .ok_or_else(|| de::Error::missing_field("assignee_id"))?,
                })
            }
        }

        // deserialize_map deliberately excludes positional/array representations of the command.
        deserializer.deserialize_map(CommandVisitor)
    }
}

/// Trusted projection of the exact requested team, supplied by the accepting service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamFact {
    pub id: String,
    pub organization_id: String,
    pub active: bool,
}

/// Trusted projection of the exact requested employee. Home-team membership is irrelevant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmployeeFact {
    pub id: String,
    pub organization_id: String,
    pub active: bool,
}

/// Facts are consulted only for their corresponding desired non-null target.
///
/// Null ignores a surplus fact. No fact can populate an omitted/cleared field or infer a team.
#[derive(Clone, Copy, Debug, Default)]
pub struct DesiredTargetFacts<'a> {
    pub team: Option<&'a TeamFact>,
    pub employee: Option<&'a EmployeeFact>,
}

/// Exactly the document-05 change payload; identity, revision, actor and time surround it later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AssignmentChange {
    #[serde(rename = "type")]
    change_type: AssignmentChangeType,
    from: AssignmentPair,
    to: AssignmentPair,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
enum AssignmentChangeType {
    #[serde(rename = "assignment_changed")]
    AssignmentChanged,
}

impl AssignmentChange {
    pub fn from(&self) -> &AssignmentPair {
        &self.from
    }

    pub fn to(&self) -> &AssignmentPair {
        &self.to
    }
}

/// A decision to propose one assignment change, not an accepted write or durable history event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignmentPlan {
    organization_id: String,
    task_id: String,
    next_revision: i64,
    change: AssignmentChange,
}

impl AssignmentPlan {
    pub fn organization_id(&self) -> &str {
        &self.organization_id
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn next_revision(&self) -> i64 {
        self.next_revision
    }

    pub fn change(&self) -> &AssignmentChange {
        &self.change
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentTarget {
    Team,
    Employee,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetProblem {
    EmptyDesiredId,
    MissingFact,
    WrongId,
    WrongOrganization,
    Inactive,
}

/// Deterministic domain diagnostics, not an authorization/disclosure policy for a service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentError {
    EmptyOrganizationId,
    EmptyTaskId,
    InvalidCurrentRevision,
    InvalidExpectedRevision,
    RevisionConflict,
    RevisionOverflow,
    InvalidTarget {
        target: AssignmentTarget,
        problem: TargetProblem,
    },
}

impl fmt::Display for AssignmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyOrganizationId => f.write_str("organization ID must be nonempty"),
            Self::EmptyTaskId => f.write_str("task ID must be nonempty"),
            Self::InvalidCurrentRevision => f.write_str("current revision must be positive"),
            Self::InvalidExpectedRevision => f.write_str("expected revision must be positive"),
            Self::RevisionConflict => {
                f.write_str("expected revision does not match current revision")
            }
            Self::RevisionOverflow => f.write_str("next revision exceeds the signed SQLite range"),
            Self::InvalidTarget { target, problem } => {
                let target = match target {
                    AssignmentTarget::Team => "assigned team",
                    AssignmentTarget::Employee => "assignee",
                };
                let reason = match problem {
                    TargetProblem::EmptyDesiredId => "desired ID must be nonempty",
                    TargetProblem::MissingFact => "target fact is missing",
                    TargetProblem::WrongId => "target fact has a different ID",
                    TargetProblem::WrongOrganization => "target belongs to another organization",
                    TargetProblem::Inactive => "target is inactive",
                };
                write!(f, "{target}: {reason}")
            }
        }
    }
}

impl std::error::Error for AssignmentError {}

/// Plan a complete assignment replacement without mutating inputs or performing I/O.
///
/// The first domain error is selected in this order:
/// 1. Empty organization ID, then empty task ID.
/// 2. Nonpositive current revision, then nonpositive expected revision.
/// 3. Expected/current mismatch, then checked next-revision overflow.
/// 4. Desired team validation, then desired employee validation. Each checks empty desired ID,
///    missing fact, wrong ID, wrong organization, then inactivity, in that order.
///
/// Revision inputs use `i64`, excluding values outside the signed SQLite integer domain. Null
/// targets ignore supplied facts. Every non-null desired target is validated on every successful
/// call, even when unchanged; historical references are never resolved. Equal pairs still plan
/// exactly one change. Same-operation receipt retries must be handled before this function.
pub fn plan_assignment(
    current: &TaskAssignmentSnapshot,
    command: &AssignmentCommand,
    targets: DesiredTargetFacts<'_>,
) -> Result<AssignmentPlan, AssignmentError> {
    if current.organization_id.is_empty() {
        return Err(AssignmentError::EmptyOrganizationId);
    }
    if current.task_id.is_empty() {
        return Err(AssignmentError::EmptyTaskId);
    }
    if current.revision <= 0 {
        return Err(AssignmentError::InvalidCurrentRevision);
    }
    if command.expected_revision <= 0 {
        return Err(AssignmentError::InvalidExpectedRevision);
    }
    if command.expected_revision != current.revision {
        return Err(AssignmentError::RevisionConflict);
    }
    let next_revision = current
        .revision
        .checked_add(1)
        .ok_or(AssignmentError::RevisionOverflow)?;

    validate_target(
        AssignmentTarget::Team,
        command.assigned_team_id.as_deref(),
        targets
            .team
            .map(|fact| (fact.id.as_str(), fact.organization_id.as_str(), fact.active)),
        &current.organization_id,
    )?;
    validate_target(
        AssignmentTarget::Employee,
        command.assignee_id.as_deref(),
        targets
            .employee
            .map(|fact| (fact.id.as_str(), fact.organization_id.as_str(), fact.active)),
        &current.organization_id,
    )?;

    Ok(AssignmentPlan {
        organization_id: current.organization_id.clone(),
        task_id: current.task_id.clone(),
        next_revision,
        change: AssignmentChange {
            change_type: AssignmentChangeType::AssignmentChanged,
            from: current.assignment.clone(),
            to: AssignmentPair::new(
                command.assigned_team_id.clone(),
                command.assignee_id.clone(),
            ),
        },
    })
}

fn validate_target(
    target: AssignmentTarget,
    desired_id: Option<&str>,
    fact: Option<(&str, &str, bool)>,
    organization_id: &str,
) -> Result<(), AssignmentError> {
    let Some(desired_id) = desired_id else {
        return Ok(());
    };
    let invalid = |problem| AssignmentError::InvalidTarget { target, problem };
    if desired_id.is_empty() {
        return Err(invalid(TargetProblem::EmptyDesiredId));
    }
    let (fact_id, fact_organization_id, active) =
        fact.ok_or_else(|| invalid(TargetProblem::MissingFact))?;
    if fact_id != desired_id {
        return Err(invalid(TargetProblem::WrongId));
    }
    if fact_organization_id != organization_id {
        return Err(invalid(TargetProblem::WrongOrganization));
    }
    if !active {
        return Err(invalid(TargetProblem::Inactive));
    }
    Ok(())
}
