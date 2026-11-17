---
name: hq-creation-provider
description: Work on or review the protected path that creates an organization with its HQ office, against the current plan and its owning documents.
---

The protected creation path is how an organization and its HQ office come into being. Its current requirements are increment A4 of docs/implementation-plan.md ("Create the organization"), then B1, which rebuilds the office store underneath it. Read before changing it: docs/design/create-organization.md, docs/design/organization-data/01-organization-structure.md, the creation parts of docs/design/organization-data/16-recovery-and-office-continuity.md (what recovery means, ownership, storage and rebuilding, and the first directory generation), ADR-0007, ADR-0011 and ADR-0016. The task records under .agent-kit/tasks/ that the skill catalog attaches to this skill are closed records of earlier work; they are not requirements.

What the code has today: the host command `organization.provision.v1` (crates/org-host/src/provisioning.rs) carries the selected create payload from crates/org-proto/src/selected_bootstrap.rs and drives the office worker's `hq_creation` (crates/org-office/src/hq_creation.rs, crates/org-office/src/office/hq_creation, crates/org-office/src/store/hq_creation.rs, apps/agentic-org-office/src/hq_creation). A4 keeps that path and deletes the name-only `organization.create`. Nothing reaches the path from a person yet.

Rules that hold for any change here:

- The signed-in administrator creates the organization; whoever creates it is its owner. Creation makes one top-level team named after the organization, the creator as a person in it and its lead, and the built-in HR lead as an employee in that team. Organization and office IDs are UUIDv7.
- The CA private key is created in the office worker, stays there and is never sent to the host, the web process, a log, a public file or the journal. A retry of the same creation returns the same CA and the same public package; a conflicting public copy is refused and kept. ADR-0016 wraps the CA key with the host-held office secrets key (increment A1); until A1 lands it sits in the encrypted office database.
- Creation is recoverable phase by phase: a crash or a lost reply at any point leaves state from which the same request completes, never a second organization or a fresh owner over an existing HQ. The office is not ready, and binds no socket, until owner authority and the organization's first records are installed.
- The recovery code is released once, on the HQ machine (ADR-0011).
- Bootstrap authority is not a person's session; it grants nothing to a browser or command-line request.
- Nothing has shipped: no migration, legacy compatibility, dual store or second writer. B1 makes creation the first accepted transaction of the organization's journal and its first directory generation.

Prove the path early with an executable test that creates, faults and reopens in a temporary root, through the real worker and store. Test wrong scope or account, missing control, a closed connection, a revoked owner and retry after each phase at the side that enforces it. A test driver is a fixture, not proof of the host's production custody.
