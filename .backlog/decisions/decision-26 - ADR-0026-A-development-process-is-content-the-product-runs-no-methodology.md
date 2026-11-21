---
id: decision-26
title: 'ADR-0026: A development process is content; the product runs no methodology'
date: '2026-11-21T00:47:00Z'
status: accepted
---

> **Review note (2026-11-21):** Strongly aligned with `doc-1` (Vision: customizable processes and DSS without hardcoded bureaucracy). Implementation pending in `TASK-55` (L7/L11 project documentation tree and index) and `TASK-56` (L12/L13/L14 composable development processes, issued doc versions, code links, and built-in DSS/feature-delivery).

# ADR-0026 · A development process is content; the product runs no methodology

Status: **decided by the lead 2026-11-18** to place the owner's
[REQ-06](../specs/req-06-desired-state-spec-authoring-and-reconciliation.md) and
[REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md) in the
selected architecture, and updated the same day with the owner's answers: a
development process is a primitive separate from organization processes, it declares
the project's tabs, it can be imported as a file, only someone who may hire can hire
([ADR-0029](0029-only-who-may-hire-hires.md), which replaced the approval of
[ADR-0027](0027-a-lead-approves-a-hire.md)) and agents start other agents
([ADR-0028](0028-an-agent-is-always-available.md)). Implementation pending.
The requirement calls it a process package; the product calls it a development
process.

## Context

The owner wants a project to follow a way of working, such as the desired state
specification factory or test-driven development, that is chosen per project, starts
with a small seed team, tells agents when to hire, and defines where the project's
documents live, with one index to find them. The same requirement says the product
must not hardcode a methodology or its trigger logic.

Three rules already in force constrain how:

- The product has no process execution engine and blocks nothing for breaking a
  process ([process design](../design/processes.md), decision 7).
- An agent reaches the product only through `agent-os`, never through files of the
  organization ([non-negotiables](../architecture-non-negotiables.md) 7 and 10).
- A turn starts only from an explicit instruction, the answer to a required ask, or a
  recovery ([agent sessions](../design/agent-sessions.md)).

## Options considered

1. **A workflow engine driven by the development process.** The manifest declares triggers ("when
   a document of this kind is ready, start that role") and the office runs them. It
   makes the factory automatic. It is the engine the product decided not to have, it
   moves methodology into machinery that must then be versioned, debugged and secured,
   and it would start agents by itself, which the turn rule forbids.
2. **Methodology in product code,** with DSS as the first built-in workflow. It is the
   fastest to a demo and the opposite of the requirement.
3. **A development process as content that agents read.** The development process holds roles, skills,
   document kinds and a layout. The product holds general records. Agents act because
   a skill says so and they have a turn. **Selected.**

## Decision

1. **A development process is content:** a manifest, role definitions, role-bound skills
   and document templates, stored in organization Git or shipped with the product. It
   contains no code, names no tool, model, credential, budget or approver, and grants
   nothing. No product code names a development process, a development process role or a process-defined
   document kind.
2. **Nothing in the product acts on a development process's stages.** Choosing a development process for a
   project creates missing roles and documentation sections and starts nothing.
3. **A development process is a different primitive from organization processes**
   (owner, 2026-11-18). Its skills reach an agent as a section of their own, filtered
   by the agent's role. They take no part in the resolution of organization processes
   and standards. A skill may adjust overridable organization guidance for its
   project; a non-overridable document is always followed.
4. **A development process gives no right to hire and starts no agent.** A development process can tell a lead
   when to hire. Who may hire is decided by the lead appointment and the
   permission to hire ([ADR-0029](0029-only-who-may-hire-hires.md)), never by a development process.
5. **A project's own documents live in one tree, `projects/<project-id>/docs/`, with
   one index, `index.yaml`.** The development process declares the starting sections. The office
   writes the index in the same accepted change as every write to the tree, from what
   agents and people do through the product. Project-wide decisions, designs and
   reports move into the tree; documents of a feature, a release or a deployment stay
   beside their record.
6. **An agent reads and writes the documentation of its own projects only.** A
   project's tree has one writing office at a time, handed over like a task's writer;
   a write from another office waits for it, as a cross-office edit of a task does.
7. **The desired state of a versioned document is its latest issued version,** written
   once. Whether the code matches it is derived from the tasks that realize it.
8. **DSS is the first built-in development process** and proves the rule: it is delivered as
   content. Releases, milestones and features are primitives of the system that a development
   process chooses to use;
   software delivery by features is a second built-in development process.
9. **Built-in development processes, and ones anyone in the organization creates or
   edits.** A person may import a process file and download one. Nothing connects two
   organizations.
10. **A development process declares the project's tabs,** each built from one of the
    product's generic views. A few tabs are fixed in every project.

## Acceptance criteria

The criteria are PKG-1 to PKG-14 in the
[development process design](../design/development-processes.md#11-acceptance-criteria), DSS-1
to DSS-11 in the
[desired state design](../design/desired-state-specifications.md#7-acceptance-criteria)
and PPD-1 to PPD-10 in
[document 18](../design/organization-data/18-development-processes-and-project-documentation.md#acceptance-criteria).
PKG-1 is the test of this record: a second built-in development process is added with content only.

## Consequences

- A factory is as reliable as its skills and the agents reading them. If handing work
  on proves unreliable, declared handoffs run by the office are the next step, and
  they would need a new decision record because they are option 1 in small.
- Agents hand work to each other: an ask or an assignment starts the next agent
  ([ADR-0028](0028-an-agent-is-always-available.md)). That is a rule about
  turns, not about development processes, and no process can change it.
- The product gains general parts that other development processes can use: the documentation tree
  and index, issued versions, code links and tasks that realize a version.
- The fixed project-level `decisions/`, `designs/` and `reports/` folders are replaced
  by sections of the tree. Nothing has shipped, so nothing is migrated.
