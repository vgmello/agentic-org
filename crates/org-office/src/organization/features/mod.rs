//! The organization family's operations: one file each, with its command and its
//! decision. A decision reads through the unit of work and returns the files the
//! operation leaves behind, or a refusal; it holds every rule, because an engine may have
//! no constraint of its own.
//!
//! Every decision of this family takes one position of the organization stream: that
//! stream's high-water is the directory generation. A refusal gives the position back.

pub mod create_organization;
pub mod create_team;

use org_store::{Ack, Actor, Id, Operation, Refusal, Sha256};
use serde::Serialize;

pub use create_organization::{CreateOrganization, Creator, OrganizationCreated};
pub use create_team::{CreateTeam, TeamCreated, TeamRefusal, TeamRejected, TeamRejection};

/// Every operation of the family. The one place their acknowledgement classes are
/// declared (ADR-0014).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    CreateOrganization,
    CreateTeam,
}

impl Op {
    pub const ALL: [Op; 2] = [Op::CreateOrganization, Op::CreateTeam];

    pub const fn ack(self) -> Ack {
        match self {
            Op::CreateOrganization | Op::CreateTeam => Ack::Durable,
        }
    }

    /// The kind a receipt records.
    pub const fn kind(self) -> &'static str {
        match self {
            Op::CreateOrganization => "organization.create",
            Op::CreateTeam => "organization.create_team",
        }
    }
}

/// What a retry of `command` is matched on: its kind, who asked and every field of it.
fn operation(op: Op, id: Id, actor: Actor, command: &impl Serialize) -> Operation {
    let fields = serde_json::to_vec(command).expect("a command is plain data");
    Operation {
        id,
        kind: op.kind(),
        actor,
        fingerprint: Sha256::of_parts(&[op.kind().as_bytes(), b"\0", &fields]),
    }
}

/// A reply in its byte form: JSON.
fn reply(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(value).expect("a reply is plain data")
}

/// Refused before admission: nothing is recorded.
fn invalid<T>(reason: &str) -> Result<T, Refusal> {
    Err(Refusal::invalid(reason))
}

fn require(holds: bool, reason: &str) -> Result<(), Refusal> {
    if holds {
        return Ok(());
    }
    invalid(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_declares_its_class_and_a_kind_of_its_own() {
        for op in Op::ALL {
            // Structure changes answer only once durable (ADR-0014).
            assert_eq!(op.ack(), Ack::Durable, "{op:?}");
            assert!(op.kind().starts_with("organization."), "{op:?}");
            let same = Op::ALL.iter().filter(|other| other.kind() == op.kind());
            assert_eq!(same.count(), 1, "{op:?}");
        }
    }

    #[test]
    fn a_retry_is_matched_on_the_kind_the_actor_and_every_field() {
        let id = |n: u64| Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap();
        let command = CreateTeam {
            id: id(1),
            name: "Core".into(),
            parent: id(2),
        };
        let by = Actor::Employee(id(3));
        let first = command.operation(id(9), &by);
        assert_eq!(first, command.clone().operation(id(9), &by));
        assert_eq!(first.kind, "organization.create_team");
        assert_eq!(first.actor, by);
        let renamed = CreateTeam {
            name: "Care".into(),
            ..command.clone()
        };
        assert_ne!(renamed.operation(id(9), &by).fingerprint, first.fingerprint);
        let moved = CreateTeam {
            parent: id(4),
            ..command.clone()
        };
        assert_ne!(moved.operation(id(9), &by).fingerprint, first.fingerprint);
        assert_ne!(command.operation(id(9), &Actor::System).actor, first.actor);
    }
}
