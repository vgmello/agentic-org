# Documentation authority

Which document answers which question, and which one wins when two disagree. The
[data contract](organization-data/README.md) owns records, fields, identities and
storage. Decision records in [`docs/adr/`](../adr/) own the decisions they state.
Designs and UX specs apply those contracts in their own context. Where a design or
spec shows a different field or rule from the data contract, the data contract
wins. When this map does not settle a disagreement, take the reading that fits the
[architecture non-negotiables](../architecture-non-negotiables.md) best, then fix
the owning document.

A requirement is not evidence that code exists: draft SQL and acceptance criteria
describe the target. What runs today is in the code, in the checklist evidence and
in the "What the code does today" section of each design.

## Which document answers which question

| Question | Authoritative reference |
| --- | --- |
| Where to start, and what to build next | [START-HERE](../START-HERE.md) and the [implementation plan](../implementation-plan.md). Neither decides behaviour. |
| What never varies, and what an engineer decides | [Architecture non-negotiables](../architecture-non-negotiables.md) |
| How far each feature has got | [Feature checklist](../FEATURES.md), with evidence; [feature areas](../FEATURE-AREAS.md) for categories |
| Records, fields, canonical files, draft SQL | [Data contract index](organization-data/README.md) and its numbered documents |
| Process packages, a project's process, the project documentation tree and its index, issued versions and code links | [Document 18](organization-data/18-process-packages-and-project-documentation.md); behaviour in [process packages](process-packages.md) and [desired state specifications](desired-state-specifications.md); the decision in [ADR-0021](../adr/0021-process-packages-are-content.md) |
| Teams, home team, reporting, project accountability | [Organization](organization-data/01-organization-structure.md), [projects](organization-data/02-project-structure.md), [employees](organization-data/04-agent-structure.md) |
| Typed tasks, references, assignment and history | [Tasks and features](organization-data/03-features-and-work.md), [identity and numbering](organization-data/06-identity-and-numbering.md), [assignment](organization-data/12-task-assignment-and-history.md), [ADR-0019](../adr/0019-office-coded-task-numbers.md) |
| Chat data, visibility and who writes what | [Messaging](organization-data/15-messaging-and-chat.md), [ADR-0003](../adr/0003-agent-messaging-office-first.md) |
| How fast an operation answers, and when it is acknowledged | [ADR-0014](../adr/0014-acknowledgement-classes-and-agent-os-latency.md) |
| How an ask waits; required asks | [ADR-0020](../adr/0020-required-asks-suspend-and-resume.md) |
| Durable sources versus rebuildable caches | [Storage contract](organization-data/README.md#shared-storage-contract), [recovery](organization-data/16-recovery-and-office-continuity.md) |
| Replaceable storage engine | [ADR-0018](../adr/0018-storage-engine-independence.md) |
| Encryption and secrets | [ADR-0016](../adr/0016-no-application-level-data-encryption.md) |
| Budgets | [Budgets](organization-data/17-budgets-and-controls.md); the [cost UX](../specs/costs-and-insights-ux.md) for presentation and access |
| Model profiles, providers, permits, placement | [Execution](organization-data/11-model-profiles-and-agent-execution.md), [providers and pools](organization-data/14-model-providers-and-connection-pools.md) |
| How agents run, isolation, private homes | [ADR-0004](../adr/0004-headless-agent-sessions.md), [ADR-0015](../adr/0015-agent-isolation-modes.md), [agent sessions](agent-sessions.md) |
| Browser sessions | [ADR-0013](../adr/0013-stored-browser-sessions.md), [web API](web-api-v1.md) |
| Platforms and containers | [ADR-0017](../adr/0017-linux-and-container-hosting.md) |
| Installation, service and privilege boundaries | [Standalone design](standalone-server.md), [host boundary](office-host-boundary.md), [installation UX](../specs/server-installation-and-access-ux.md) |
| Executable names and which entry points work | [Executables and entry points](executable-migration.md) |
| Code layout and dependency direction | [Architecture rules](../architecture-rules.md), [project structure](project-structure.md) |
| What must be preserved from before | Nothing: [no compatibility obligations before v1 ships](organization-data/README.md#no-compatibility-obligations-before-v1-ships) |
| Names | The executables are `agentic-org`, `agentic-org-office`, `agentic-org-web` and `agent-os`. The browser-facing process is "web"; `gateway` survives only in code identifiers until they are renamed. |

## Designs and UX guides by topic

| Concern | Detailed guides | Owning data documents |
| --- | --- | --- |
| Navigation, shared interaction rules, the owner's inbox | [Shell](../specs/app-shell-ux.md), [Home and Inbox](../specs/our-org-in-the-app.md) | Identity and access contracts |
| Organization creation, home teams, leads, project accountability | [Creation](create-organization.md), [teams](teams-and-org-chart.md), [offices and teams UX](../specs/offices-and-teams-ux.md), [hiring and profiles](../specs/our-org-in-the-app.md), [roles](../specs/roles-ux.md) | 01, 02, 04, 06, 08 |
| Typed tasks, assignment, reporter, history, private notes | [Task design](tasks-projects-docs.md), [task UX](../specs/work-items-ux.md), [request UX](../specs/feature-requests-ux.md) | 03, 05, 06, 08, 12 |
| Repositories, features, knowledge, releases | [Git design](projects-git-github.md), [features](../specs/features-ux.md), [knowledge](../specs/decisions-and-tech-debt-ux.md), [status reports](../specs/project-status-ux.md) | 02, 03, 07, 08, 09, 18 |
| Execution, routing, placement and recovery | [Sessions](agent-sessions.md), [supervision](employee-supervision.md), [employee UX](../specs/employee-supervision-ux.md), [smart features](smart-features.md) | 04, 10, 11, 14, 16, 17 |
| Messaging, accounting and budgets | [Chat UX](../specs/chat-ux.md), [REQ-04](../specs/req-04-channel-messaging-and-question-components.md), [cost UX](../specs/costs-and-insights-ux.md) | 10, 15, 16, 17 |
| Offices, host, HTTP and installation | [Offices](offices-hq-and-satellites.md), [host boundary](office-host-boundary.md), [web API](web-api-v1.md), [standalone design](standalone-server.md), [installation UX](../specs/server-installation-and-access-ux.md) | 01, 04, 06, 11, 15, 16, 17 |
| Processes and standards | [Process design](processes.md), [process UX](../specs/processes-ux.md) | 01 |
| Process packages, seed teams, skills and project documentation | [Process packages](process-packages.md), [REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md), [knowledge UX](../specs/decisions-and-tech-debt-ux.md) | 01, 02, 04, 18 |
| Desired state specifications | [Desired state design](desired-state-specifications.md), [REQ-06](../specs/req-06-desired-state-spec-authoring-and-reconciliation.md) | 02, 03, 18 |

Library and CLI READMEs describe the interfaces the code implements. Research
records in [`docs/research/`](../research/) are dated observations; they are not
rewritten to match a target, and they are not requirements.

## Relationships at a glance

- An organization has a recursive team tree; each employee has one home team. A lead
  is a direct member and manages the subtree; an ordinary member of an ancestor team
  gains no authority. Person-to-person reporting is separate.
- A project has one accountable owner team and may have contributing teams. Teams and
  projects belong to the organization; offices place agents, not teams or projects.
- A task always has an accountable team and optionally a project. Task, bug and
  request are task types, all with `T-` references minted by the creating office
  (`T-B37`), unique in the organization. Assignment is independent of accountability.
- HQ allocates `TEAM-` and `P-` numbers for the organization, `F-` and `R-` within
  a project and `M-` within a release. Organizations and offices have slugs and no
  counter.
- A repository is registered once and may be shared by projects. Integration
  branches belong to each project-repository association. Code repositories may be
  local; the organization repository is remote.
- Features have project-prefixed slugs and stable-ID paths. Release product
  changelogs and deployment technical changelogs are distinct.
- A project may follow one process package. The package is content: roles, skills,
  document kinds and a documentation layout. Its skills are a layer of process
  resolution between the team chain and the project. It starts nothing and grants
  nothing.
- A project's own documents live in one tree, `projects/<project-id>/docs/`, with one
  index. A versioned document's desired state is its latest issued version, and a
  version is converged when the tasks that realize it are finished.
- The built-in HR agent is an employee with a home team; its chart placement is
  presentation.
- The owner is an authenticated account. A person record for the owner has a home
  team like any employee; showing the owner above the chart grants nothing.

## Folder and privacy boundaries

Portable definitions and open work live in organization Git; task work and code live
in runtime worktrees. Supporting knowledge has a required project scope and at most
one feature, release or deployment scope in the same project. An agent reads and
writes the documentation tree of its own projects only; people read every project's. Release content and
intended flags are authored records; deployment snapshots record what was applied.
Protected chat, budget amounts and history, private notes, usage archives, receipts
and recovery evidence keep their visibility and durability boundary even though
their projection shares one draft schema. Private task notes are visible to their
author only, including against owners and administrators. Published task history is
shared and contains no private-note body or metadata.

The runtime layout is defined by [employee structure](organization-data/04-agent-structure.md#runtime-location):

```text
runtime/<office-id>/
├── repositories/<repository-id>.git/              # cache, no project integration branch
└── teams/<root-team-id>/teams/<child-team-id>/.../
    └── agents/<employee-id>/
        ├── projects/<project-id>/tasks/<task-id>/repositories/<repository-id>/
        └── tasks/<task-id>/repositories/<repository-id>/   # projectless task
```

Workspaces survive handoff until the task closes. Moving an agent to another office
needs an explicit safe workspace move, never a recursive delete. The office derives
the Git-metadata, credential, other-task and managed-state denies for these paths
and verifies them before it launches an agent.

Deleting an office cache is safe only when protected sources are complete,
credentials can be bound again and processes are reconciled. A Git checkout alone
does not restore chat, budgets, usage or authority.

## Not in v1

Areas, Undo, advanced checklist execution, retrospectives, automatic scheduling,
agent memory consolidation and a process execution engine are not part of v1. A
process package is not an engine: its stages are read by people and agents and run
by nothing ([process packages](process-packages.md), decision 2). A UX
criterion for one of them is not approval to build it. Process and standard content,
its scope and the override lock are selected, and so is resolution
([process design](processes.md)). Budgets and recovery are selected as data and
control contracts; their evaluators, pause coordination and restore services are
implementation work.
