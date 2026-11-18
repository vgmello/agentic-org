# ADR-0021 · A process package is content; the product runs no methodology

Status: **decided by the lead 2026-11-18** to place the owner's
[REQ-06](../specs/req-06-desired-state-spec-authoring-and-reconciliation.md) and
[REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md) in the
selected architecture. Implementation pending. Five points are with the owner and are
listed in REQ-07's alignment section; none of them changes the decisions below.

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

1. **A workflow engine driven by the package.** The manifest declares triggers ("when
   a document of this kind is ready, start that role") and the office runs them. It
   makes the factory automatic. It is the engine the product decided not to have, it
   moves methodology into machinery that must then be versioned, debugged and secured,
   and it would start agents by itself, which the turn rule forbids.
2. **Methodology in product code,** with DSS as the first built-in workflow. It is the
   fastest to a demo and the opposite of the requirement.
3. **A package as content that agents read.** The package holds roles, skills,
   document kinds and a layout. The product holds general records. Agents act because
   a skill says so and they have a turn. **Selected.**

## Decision

1. **A process package is content:** a manifest, role definitions, role-bound skills
   and document templates, stored in organization Git or shipped with the product. It
   contains no code, names no tool, model, credential, budget or approver, and grants
   nothing. No product code names a package, a package role or a package-defined
   document kind.
2. **Nothing in the product acts on a package's stages.** Choosing a package for a
   project creates missing roles and documentation sections and starts nothing.
3. **Skills are a layer of the existing resolution,** between the accountable team's
   chain and the project, filtered by the agent's role. They are overridable guidance.
   Non-overridable documents of the organization and its teams keep their force.
4. **Hiring, approval and starting agents are unchanged.** A package can tell a lead
   when to ask for a hire. It cannot approve one, and it cannot start an agent.
5. **A project's own documents live in one tree, `projects/<project-id>/docs/`, with
   one index, `index.yaml`.** The package declares the starting sections. The office
   writes the index in the same accepted change as every write to the tree, from what
   agents and people do through the product. Project-wide decisions, designs and
   reports move into the tree; documents of a feature, a release or a deployment stay
   beside their record.
6. **An agent reads and writes the documentation of its own projects only.**
7. **The desired state of a versioned document is its latest issued version,** written
   once. Whether the code matches it is derived from the tasks that realize it.
8. **DSS is the first built-in package** and proves the rule: it is delivered as
   content.

## Acceptance criteria

The criteria are PKG-1 to PKG-14 in the
[process package design](../design/process-packages.md#9-acceptance-criteria), DSS-1
to DSS-11 in the
[desired state design](../design/desired-state-specifications.md#7-acceptance-criteria)
and PPD-1 to PPD-10 in
[document 18](../design/organization-data/18-process-packages-and-project-documentation.md#acceptance-criteria).
PKG-1 is the test of this record: a second built-in package is added with content only.

## Consequences

- A factory is as reliable as its skills and the agents reading them. If handing work
  on proves unreliable, declared handoffs run by the office are the next step, and
  they would need a new decision record because they are option 1 in small.
- With the turn rule as it is, a person starts the next agent at each handoff. Whether
  an ask or an assignment starts an agent is the owner's decision and changes no
  record here.
- The product gains general parts that other packages can use: the documentation tree
  and index, issued versions, code links and tasks that realize a version.
- The fixed project-level `decisions/`, `designs/` and `reports/` folders are replaced
  by sections of the tree. Nothing has shipped, so nothing is migrated.
