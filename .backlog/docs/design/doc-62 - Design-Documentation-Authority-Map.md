---
id: doc-62
title: 'Design: Documentation Authority Map'
type: guide
created_date: '2026-11-21T00:48:00Z'
updated_date: '2026-11-23T03:54:00Z'
tags:
  - design
  - governance
  - architecture
  - authority
---
> **Review note (2026-11-21):** Core documentation authority map (Organization Data contracts own records/fields/identities/storage; ADRs own architectural decisions; designs and specs apply them; `doc-1` states the Vision). Relative links still point to legacy `docs/` paths pending cutover to `.backlog/docs/` and `.backlog/decisions/` in `TASK-32`. Note also that the "What starts an agent's turn" row should be read alongside ADR-0030 (`decision-30`: durable pending starts, automatic restart continuation, and stall detection).

# Documentation authority

Which document answers which question, and which one wins when two disagree. The
[data contract](../organization-data/doc-72%20-%20Organization-Data-—-README.md) owns records, fields, identities and
storage. Decision records in [`.backlog/decisions/`](../../decisions) own the decisions they state.
Designs and UX/AX specs apply those contracts in their own context. Where a design or
spec shows a different field or rule from the data contract, the data contract
wins. When this map does not settle a disagreement, take the reading that fits the
[architecture non-negotiables](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md) best, then fix
the owning document.

A requirement is not evidence that code exists: draft SQL and acceptance criteria
describe the target. What runs today is in the code, in the checklist evidence and
in the "What the code does today" section of each design.

## Which document answers which question

| Question | Authoritative reference |
| --- | --- |
| Where to start, and what to build next | [START-HERE](../doc-2%20-%20Start-Here.md) and the [implementation plan](../doc-5%20-%20v1-Implementation-Plan.md). Neither decides behaviour. |
| What never varies, and what an engineer decides | [Architecture non-negotiables](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md) |
| How far each feature has got | [Feature checklist](../doc-3%20-%20Feature-Checklist.md), with evidence; [feature areas](../doc-4%20-%20Feature-Areas.md) for categories |
| Records, fields, canonical files, draft SQL | [Data contract index](../organization-data/doc-72%20-%20Organization-Data-—-README.md) and its numbered documents |
| Development processes, a project's process, the project documentation tree and its index, issued versions and code links | [Document 18](../organization-data/doc-86%20-%20Organization-Data-18-—-Development-Processes-and-Project-Documentation.md); behaviour in [development processes](doc-32%20-%20Design-Development-Processes-Process-Packages.md) and [desired state specifications](doc-31%20-%20Design-Desired-State-Specifications-DSS.md); the decision in [ADR-0026](../../decisions/decision-26%20-%20ADR-0026-A-development-process-is-content-the-product-runs-no-methodology.md) |
| Who may hire; an agent lead's permission to hire | [ADR-0029](../../decisions/decision-29%20-%20ADR-0029-Only-someone-who-may-hire-can-hire-there-is-no-approval.md), [employees](../organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md#who-may-hire), [hire flow](../specs/doc-45%20-%20Spec-Hiring-Profiles-and-Team-Moves-UX.md) |
| Teams, home team, reporting, project accountability | [Organization](../organization-data/doc-75%20-%20Organization-Data-01-—-Organization-Structure.md), [projects](../organization-data/doc-76%20-%20Organization-Data-02-—-Project-Structure.md), [employees](../organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md) |
| Typed tasks, references, assignment and history | [Tasks and features](../organization-data/doc-77%20-%20Organization-Data-03-—-Features-and-Work.md), [identity and numbering](../organization-data/doc-80%20-%20Organization-Data-06-—-Identity-and-Numbering.md), [assignment](../organization-data/doc-84%20-%20Organization-Data-12-—-Task-Assignment-and-History.md), [ADR-0019](../../decisions/decision-19%20-%20ADR-0019-Task-numbers-are-minted-by-the-creating-office.md) |
| Chat data, visibility and who writes what | [Messaging](../organization-data/doc-90%20-%20Organization-Data-15-—-Messaging-and-Chat.md), [ADR-0003](../../decisions/decision-3%20-%20ADR-0003-Agent-messaging-is-office-first.md) |
| How fast an operation answers, and when it is acknowledged | [ADR-0014](../../decisions/decision-14%20-%20ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md) |
| How an ask waits; required asks | [ADR-0020](../../decisions/decision-20%20-%20ADR-0020-An-ask-waits-with-a-timeout-a-required-ask-suspends-the-agent-until-it-is-answered.md) |
| What starts an agent's turn | [Run modes](../specs/doc-50%20-%20Spec-Agent-Run-Modes-and-Availability-UX.md): a person's Run, the answer to a required ask (ADR-0020), being addressed by another agent or answered ([ADR-0023](../../decisions/decision-23%20-%20ADR-0023-An-agents-ask-hand-off-or-mention-and-an-answer-to-its-own-question-start-a-turn-for-an-idle-agent.md)), a person's direct message or mention or a task assignment ([ADR-0028](../../decisions/decision-28%20-%20ADR-0028-An-agent-is-always-available-a-persons-message-and-a-task-assignment-also-start-it.md)), a reactive completion wake from the agent's own active background task or sub-agent ([ADR-0025](../../decisions/decision-25%20-%20ADR-0025-An-agent-can-start-sub-agents-inside-its-turn.md) decision 6), or a recovery |
| What reaches an agent's inbox; mentions; the channel watcher | [ADR-0024](../../decisions/decision-24%20-%20ADR-0024-An-agents-inbox-holds-what-is-addressed-to-it-and-each-channel-has-a-watcher-that-feeds-responses-back-to-the-agents-who-asked.md) |
| Sub-agents inside an agent's turn | [ADR-0025](../../decisions/decision-25%20-%20ADR-0025-An-agent-can-start-sub-agents-inside-its-turn.md) |
| Agent Experience (AX) standards, discovery tiers, token budgets, Day-1 onboarding, skills and AX quality gates | [Agent Experience (AX) Spec](../specs/doc-96%20-%20Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md) (`AX-1`–`AX-24`), with [agent instructions](doc-27%20-%20Design-Agent-Instructions-Assembly.md), [agent command line](doc-28%20-%20Design-agent-os-Command-Line.md) and [REQ-08](../specs/doc-59%20-%20REQ-08-agent-os-CLI-and-Kernel-Primitives.md). Outranks the `principal-ax-designer` persona; yields to data contracts and ADRs |
| What every agent is told about how to work in the product | [Agent instructions](doc-27%20-%20Design-Agent-Instructions-Assembly.md). It follows the contracts it names and never overrides one |
| The agents' command line: grammar, answer format, errors and the list of commands | [Agent command line](doc-28%20-%20Design-agent-os-Command-Line.md), from [REQ-08](../specs/doc-59%20-%20REQ-08-agent-os-CLI-and-Kernel-Primitives.md). What a command means stays with the contract its row names |
| Durable sources versus rebuildable caches | [Storage contract](../organization-data/doc-72%20-%20Organization-Data-—-README.md#shared-storage-contract), [recovery](../organization-data/doc-91%20-%20Organization-Data-16-—-Recovery-and-Office-Continuity.md) |
| How the office stores, acknowledges and rebuilds data | [Office store design](doc-66%20-%20Design-Office-Store-Architecture.md), [ADR-0021](../../decisions/decision-21%20-%20ADR-0021-The-office-store-one-journal-of-file-changes-a-projection-rebuilt-from-files.md) |
| Replaceable storage engine | [ADR-0018](../../decisions/decision-18%20-%20ADR-0018-The-storage-engine-is-replaceable-PostgreSQL-must-be-possible.md) |
| Encryption and secrets | [ADR-0016](../../decisions/decision-16%20-%20ADR-0016-No-application-level-encryption-of-organization-data.md) |
| Budgets | [Budgets](../organization-data/doc-92%20-%20Organization-Data-17-—-Budgets-and-Controls.md); the [cost UX](../specs/doc-51%20-%20Spec-Costs-Budgets-and-Insights-UX.md) for presentation and access |
| Model profiles, providers, permits, placement | [Execution](../organization-data/doc-88%20-%20Organization-Data-11-—-Model-Profiles-and-Agent-Execution.md), [providers and pools](../organization-data/doc-89%20-%20Organization-Data-14-—-Model-Providers-and-Connection-Pools.md) |
| Browser-driven CLI sign-in enrollment, provider credential wrapping/distribution, and permission presets | [ADR-0032](../../decisions/decision-32%20-%20ADR-0032-Browser-driven-CLI-sign-in-enrollment-credential-distribution-and-permission-presets.md), [providers and pools](../organization-data/doc-89%20-%20Organization-Data-14-—-Model-Providers-and-Connection-Pools.md), [execution](../organization-data/doc-88%20-%20Organization-Data-11-—-Model-Profiles-and-Agent-Execution.md), [ADR-0016](../../decisions/decision-16%20-%20ADR-0016-No-application-level-encryption-of-organization-data.md) |
| How agents run, isolation, private homes | [ADR-0004](../../decisions/decision-4%20-%20ADR-0004-Agent-sessions-run-headless-people-interact-through-chat.md), [ADR-0015](../../decisions/decision-15%20-%20ADR-0015-Agent-isolation-is-a-selectable-mode-with-a-dedicated-OS-user-provisioned-at-setup.md), [agent sessions](doc-29%20-%20Design-Agent-Sessions-and-Runtime-Isolation.md) |
| Browser sessions | [ADR-0013](../../decisions/decision-13%20-%20ADR-0013-Browser-sessions-are-stored-at-the-host-and-survive-restarts.md), [web API](doc-71%20-%20Design-Web-HTTP-API-v1.md) |
| Platforms and containers | [ADR-0017](../../decisions/decision-17%20-%20ADR-0017-v1-runs-on-macOS-and-Linux-including-inside-a-container.md) |
| Installation, service and privilege boundaries | [Standalone design](doc-70%20-%20Design-Standalone-Agentic-Org-Server.md), [host boundary](doc-65%20-%20Design-Host-and-Office-Worker-Boundary.md), [installation UX](../specs/doc-48%20-%20Spec-Server-Installation-Sign-In-and-Access-UX.md), [ADR-0022](../../decisions/decision-22%20-%20ADR-0022-The-service-host-creates-its-own-registry-setup-has-no-reverse-bootstrap.md). Proposed, not accepted: [dedicated accounts, the agent sandbox and extensions](doc-61%20-%20Design-Dedicated-Accounts-Agent-Sandbox-and-Extensions.md); the accepted documents win until its ADRs are written |
| Executable names and which entry points work | [Executables and entry points](doc-63%20-%20Design-Executables-and-Entry-Points.md) |
| Code layout and dependency direction | [Architecture rules](../doc-8%20-%20Architecture-Rules.md), [project structure](doc-68%20-%20Design-v1-Project-Structure.md) |
| What must be preserved from before | Nothing: [no compatibility obligations before v1 ships](../organization-data/doc-72%20-%20Organization-Data-—-README.md#no-compatibility-obligations-before-v1-ships) |
| Names | The executables are `agentic-org`, `agentic-org-office`, `agentic-org-web` and `agent-os`. The browser-facing process is "web"; `gateway` survives only in code identifiers until they are renamed. |

## Designs and UX guides by topic

| Concern | Detailed guides | Owning data documents |
| --- | --- | --- |
| Navigation, shared interaction rules, the owner's inbox | [Shell](../specs/doc-39%20-%20Spec-App-Shell-and-Navigation-UX.md), [Home and Inbox](../specs/doc-45%20-%20Spec-Hiring-Profiles-and-Team-Moves-UX.md) | Identity and access contracts |
| Agent Experience (AX): `agent-os` CLI, Day-1 onboarding, skills, conflict recovery and evals | [Agent Experience (AX) Spec](../specs/doc-96%20-%20Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md), [agent instructions](doc-27%20-%20Design-Agent-Instructions-Assembly.md), [agent command line](doc-28%20-%20Design-agent-os-Command-Line.md), [REQ-08](../specs/doc-59%20-%20REQ-08-agent-os-CLI-and-Kernel-Primitives.md) | 04, 11, 15, 18 |
| Organization creation, home teams, leads, project accountability | [Creation](doc-30%20-%20Design-Create-Organization.md), [teams](doc-38%20-%20Design-Teams-People-and-Org-Chart.md), [offices and teams UX](../specs/doc-44%20-%20Spec-Offices-Teams-People-and-Org-Chart-UX.md), [hiring and profiles](../specs/doc-45%20-%20Spec-Hiring-Profiles-and-Team-Moves-UX.md), [roles](../specs/doc-47%20-%20Spec-Agent-Roles-Directory-UX.md) | 01, 02, 04, 06, 08 |
| Typed tasks, assignment, reporter, history, private notes | [Task design](doc-37%20-%20Design-Tasks-Projects-and-Documents.md), [task UX](../specs/doc-49%20-%20Spec-Work-Items-and-Tasks-UX.md), [request UX](../specs/doc-42%20-%20Spec-Feature-Requests-and-Intake-UX.md) | 03, 05, 06, 08, 12 |
| Repositories, features, knowledge, releases | [Git design](doc-35%20-%20Design-Projects-Git-and-GitHub.md), [features](../specs/doc-43%20-%20Spec-Features-Milestones-and-Releases-UX.md), [knowledge](../specs/doc-52%20-%20Spec-Knowledge-Decisions-and-Tech-Debt-UX.md), [status reports](../specs/doc-46%20-%20Spec-Project-Status-Overview-UX-—-Not-in-v1.md) | 02, 03, 07, 08, 09, 18 |
| Execution, routing, placement and recovery | [Sessions](doc-29%20-%20Design-Agent-Sessions-and-Runtime-Isolation.md), [supervision](doc-33%20-%20Design-Employee-Supervision.md), [employee UX](../specs/doc-53%20-%20Spec-Employee-Supervision-and-Approvals-UX.md), [smart features](doc-36%20-%20Design-Smart-Features-Not-in-v1.md) | 04, 10, 11, 14, 16, 17 |
| Messaging, accounting and budgets | [Chat UX](../specs/doc-40%20-%20Spec-Chat-Channels-and-Structured-Asks-UX.md), [REQ-04](../specs/doc-55%20-%20REQ-04-Channel-Messaging-and-Question-Components.md), [cost UX](../specs/doc-51%20-%20Spec-Costs-Budgets-and-Insights-UX.md) | 10, 15, 16, 17 |
| Offices, host, HTTP and installation | [Offices](doc-67%20-%20Design-Offices-HQ-and-Satellites.md), [host boundary](doc-65%20-%20Design-Host-and-Office-Worker-Boundary.md), [web API](doc-71%20-%20Design-Web-HTTP-API-v1.md), [standalone design](doc-70%20-%20Design-Standalone-Agentic-Org-Server.md), [installation UX](../specs/doc-48%20-%20Spec-Server-Installation-Sign-In-and-Access-UX.md) | 01, 04, 06, 11, 15, 16, 17 |
| Processes and standards | [Process design](doc-34%20-%20Design-Organization-Processes-and-Standards.md), [process UX](../specs/doc-54%20-%20Spec-Organization-Processes-and-Standards-UX.md) | 01 |
| Development processes, seed teams, skills and project documentation | [Development processes](doc-32%20-%20Design-Development-Processes-Process-Packages.md), [REQ-07](../specs/doc-58%20-%20REQ-07-Composable-Process-Packages-and-Team-Blueprints.md), [knowledge UX](../specs/doc-52%20-%20Spec-Knowledge-Decisions-and-Tech-Debt-UX.md) | 01, 02, 04, 18 |
| Desired state specifications | [Desired state design](doc-31%20-%20Design-Desired-State-Specifications-DSS.md), [REQ-06](../specs/doc-57%20-%20REQ-06-Desired-State-Specification-DSS-and-Factory-Reconciliation.md) | 02, 03, 18 |

Library and CLI READMEs describe the interfaces the code implements. Research
records in [`docs/research/`](../research) are dated observations; they are not
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
- A project may follow one development process. The development process is content: roles, skills,
  document kinds, tabs and a documentation layout. It is a different primitive from
  organization processes and standards: neither inherits from nor overrides the
  other, and the organization's documents win where they disagree. It starts nothing
  and grants nothing.
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
their projection shares one draft schema. A private task note is read by its author
and by people who can read the task, never by another agent
([ADR-0031](../../decisions/decision-31%20-%20ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md)). Published task history is
shared and contains no private-note body or metadata.

The runtime layout is defined by [employee structure](../organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md#runtime-location):

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
development process is not an engine: its stages are read by people and agents and run
by nothing ([development processes](doc-32%20-%20Design-Development-Processes-Process-Packages.md), decision 2). A UX
criterion for one of them is not approval to build it. Process and standard content,
its scope and the override lock are selected, and so is resolution
([process design](doc-34%20-%20Design-Organization-Processes-and-Standards.md)). Budgets and recovery are selected as data and
control contracts; their evaluators, pause coordination and restore services are
implementation work.
