//! Selected task snapshot adapter. The store/application service must still
//! authenticate, verify public classification/directory/writer authority, freeze
//! and durably accept the resulting snapshot/event/receipt before completion.
//! These source operations perform no I/O and publish no capability.

use std::fmt;

pub use crate::store::acceptance::{
    AcceptanceConfig, AcceptanceIdentity, JournalPosition, WriterAuthority,
};
use org_docs::{RootScalarEdit, ScalarValue, SourceDoc, SourceError};
use org_work::selected_assignment::{AssignmentPair, AssignmentPlan, TaskAssignmentSnapshot};
mod adoption;
mod bootstrap;
pub(crate) mod directory;
pub(crate) mod read;
pub(crate) mod service;
mod staging;
pub use service::{AssignmentOutcome, AssignmentRequest, OwnerAccount, OwnerAuthority};

use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    Source(SourceError),
    UnsupportedSchema,
    InvalidFacts,
    PlanMismatch,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(_) => f.write_str("selected snapshot source rejected"),
            other => write!(f, "selected snapshot: {other:?}"),
        }
    }
}
impl std::error::Error for SnapshotError {}
impl From<SourceError> for SnapshotError {
    fn from(e: SourceError) -> Self {
        Self::Source(e)
    }
}

/// Public immutable facts; no inferred owner, home team, scope or reporter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImmutableTaskFacts {
    pub organization_id: String,
    pub task_id: String,
    pub team_id: String,
    pub project_id: Option<String>,
    pub number: Option<i64>,
    pub task_type: String,
    pub title: String,
    pub status: String,
    pub created_at: Option<String>,
    pub created_by_kind: String,
    pub created_by_id: Option<String>,
}

/// A source-retaining selected snapshot. Organization identity comes from the
/// authenticated registry/import context, not a task-controlled authority field.
pub struct TaskSnapshot<'a> {
    source: SourceDoc<'a>,
    immutable: ImmutableTaskFacts,
    revision: i64,
    assignment: AssignmentPair,
}

/// Trusted updater representation; this value is not an authorization grant.
#[derive(Clone, Copy)]
pub enum Updater<'a> {
    Owner,
    Employee(&'a str),
    System,
}

impl<'a> TaskSnapshot<'a> {
    pub fn parse(organization_id: &str, markdown: &'a str) -> Result<Self, SnapshotError> {
        if organization_id.is_empty() {
            return Err(SnapshotError::InvalidFacts);
        }
        let source = SourceDoc::parse(markdown)?;
        let map = source.front_matter();
        if map.get("schema_version").and_then(Value::as_i64) != Some(1) {
            return Err(SnapshotError::UnsupportedSchema);
        }
        // If an extension redundantly records organization identity, it must not
        // contradict its actual admitted scope. It never determines that scope.
        if map
            .get("organization_id")
            .is_some_and(|v| v.as_str() != Some(organization_id))
        {
            return Err(SnapshotError::InvalidFacts);
        }
        let revision = map
            .get("revision")
            .and_then(Value::as_i64)
            .filter(|n| *n > 0)
            .ok_or(SnapshotError::InvalidFacts)?;
        let created_by_kind = text(map, "created_by_kind")?;
        let created_by_id = nullable_id(map, "created_by_id")?;
        check_actor(&created_by_kind, created_by_id.as_deref())?;
        let updated_kind = text(map, "updated_by_kind")?;
        let updated_id = nullable_id(map, "updated_by_id")?;
        check_actor(&updated_kind, updated_id.as_deref())?;
        nullable_text(map, "updated_at")?;
        let number = match map.get("number") {
            Some(Value::Null) => None,
            Some(v) => Some(
                v.as_i64()
                    .filter(|n| *n > 0)
                    .ok_or(SnapshotError::InvalidFacts)?,
            ),
            None => return Err(SnapshotError::InvalidFacts),
        };
        let immutable = ImmutableTaskFacts {
            organization_id: organization_id.to_owned(),
            task_id: text(map, "id")?,
            team_id: text(map, "team_id")?,
            project_id: nullable_id(map, "project_id")?,
            number,
            task_type: text(map, "type")?,
            title: text(map, "title")?,
            status: text(map, "status")?,
            created_at: nullable_text(map, "created_at")?,
            created_by_kind,
            created_by_id,
        };
        let assignment = AssignmentPair::new(
            nullable_id(map, "assigned_team_id")?,
            nullable_id(map, "assignee_id")?,
        );
        Ok(Self {
            source,
            immutable,
            revision,
            assignment,
        })
    }

    pub fn immutable(&self) -> &ImmutableTaskFacts {
        &self.immutable
    }
    pub fn revision(&self) -> i64 {
        self.revision
    }
    pub fn assignment(&self) -> &AssignmentPair {
        &self.assignment
    }
    pub fn markdown(&self) -> &'a str {
        self.source.source()
    }

    pub fn assignment_state(&self) -> TaskAssignmentSnapshot {
        TaskAssignmentSnapshot {
            organization_id: self.immutable.organization_id.clone(),
            task_id: self.immutable.task_id.clone(),
            revision: self.revision,
            assignment: self.assignment.clone(),
        }
    }

    /// Materialize the reviewed pure decision using only the six permitted
    /// scalar tokens. No accepted outcome, write, permission or durability is
    /// implied. The writer must prevent task/directory generation races.
    pub fn rewrite_assignment(
        &self,
        plan: &AssignmentPlan,
        updated_at: &str,
        updater: Updater<'_>,
    ) -> Result<String, SnapshotError> {
        if plan.organization_id() != self.immutable.organization_id
            || plan.task_id() != self.immutable.task_id
            || self.revision.checked_add(1) != Some(plan.next_revision())
            || plan.change().from() != &self.assignment
            || org_work::parse_ms(updated_at).is_none()
        {
            return Err(SnapshotError::PlanMismatch);
        }
        let (kind, actor_id) = match updater {
            Updater::Owner => ("owner", None),
            Updater::System => ("system", None),
            Updater::Employee(id) if !id.is_empty() => ("employee", Some(id)),
            Updater::Employee(_) => return Err(SnapshotError::InvalidFacts),
        };
        fn nullable(id: Option<&str>) -> ScalarValue<'_> {
            id.map(ScalarValue::String).unwrap_or(ScalarValue::Null)
        }
        let output = self.source.replace(&[
            RootScalarEdit {
                key: "revision",
                value: ScalarValue::Integer(plan.next_revision()),
            },
            RootScalarEdit {
                key: "assigned_team_id",
                value: nullable(plan.change().to().assigned_team_id()),
            },
            RootScalarEdit {
                key: "assignee_id",
                value: nullable(plan.change().to().assignee_id()),
            },
            RootScalarEdit {
                key: "updated_at",
                value: ScalarValue::String(updated_at),
            },
            RootScalarEdit {
                key: "updated_by_kind",
                value: ScalarValue::String(kind),
            },
            RootScalarEdit {
                key: "updated_by_id",
                value: nullable(actor_id),
            },
        ])?;
        let reparsed = TaskSnapshot::parse(&self.immutable.organization_id, &output)?;
        if reparsed.immutable != self.immutable
            || reparsed.revision != plan.next_revision()
            || &reparsed.assignment != plan.change().to()
        {
            return Err(SnapshotError::PlanMismatch);
        }
        Ok(output)
    }
}

fn text(map: &Map<String, Value>, key: &str) -> Result<String, SnapshotError> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or(SnapshotError::InvalidFacts)
}

fn nullable_id(map: &Map<String, Value>, key: &str) -> Result<Option<String>, SnapshotError> {
    match map.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
        _ => Err(SnapshotError::InvalidFacts),
    }
}

fn nullable_text(map: &Map<String, Value>, key: &str) -> Result<Option<String>, SnapshotError> {
    // Unknown import timestamps remain null; original nonnull source is retained.
    nullable_id(map, key)
}

fn check_actor(kind: &str, id: Option<&str>) -> Result<(), SnapshotError> {
    match (kind, id) {
        ("employee", Some(id)) if !id.is_empty() => Ok(()),
        ("owner" | "system" | "unknown", None) => Ok(()),
        _ => Err(SnapshotError::InvalidFacts),
    }
}

#[cfg(test)]
mod tests;
