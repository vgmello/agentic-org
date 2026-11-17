# Documentation authority and selected v1 model

This is the reading map for the selected organization data design and its reconciled design/UX guides (2026-11-15). The owning organization-data documents govern selected contracts; additive requirement refinements are made in their owning documents. Documentation does not change application behavior, approve a migration or select deferred engines.

## Which document answers which question?

| Question | Authoritative reference |
| --- | --- |
| What features are complete, partial, planned, blocked, deferred or awaiting verification? | [Feature checklist](../FEATURES.md), with scoped implementation evidence and links to the requirements below |
| How are features categorized, and what does each area cover or need verified? | [Feature area guide](../FEATURE-AREAS.md), with category boundaries, requirement links and a summary of checklist statuses |
| Selected entity relationships, fields, canonical files and draft SQL | [Organization data index](organization-data/README.md) and its owning documents |
| Teams, home membership, reporting and accountable project ownership | [Organization](organization-data/01-organization-structure.md), [projects](organization-data/02-project-structure.md), [employees](organization-data/04-agent-structure.md) |
| Typed tasks, retained bug/request detail, feature lifecycle and references | [Tasks/features](organization-data/03-features-and-work.md), [numbering](organization-data/06-identity-and-numbering.md), [assignment](organization-data/12-task-assignment-and-history.md) |
| Chat source data, import visibility and hashes | [Messaging](organization-data/15-messaging-and-chat.md) |
| Durable evidence versus disposable office state | [Recovery](organization-data/16-recovery-and-office-continuity.md), plus [private notes](organization-data/05-operational-data.md#private-notes-attached-to-tasks) and [accounting](organization-data/10-usage-and-cost-accounting.md) |
| Budget definitions, periods, extensions, notices and holds | [Budgets](organization-data/17-budgets-and-controls.md); visible [cost UX](../specs/costs-and-insights-ux.md) for presentation and access rules |
| Execution model/profile/permit/failover data and responsibilities | [Execution](organization-data/11-model-profiles-and-agent-execution.md), [provider/pool data](organization-data/14-model-providers-and-connection-pools.md) |
| Currently implemented executable names and unavailable commands | [Executable migration](executable-migration.md), verified against source at the implementation commit being discussed |
| Installation, service and privilege-boundary requirements | [Standalone design](standalone-server.md), [installation UX](../specs/server-installation-and-access-ux.md), ADR-0005 |
| Which contradictions between documents, contracts and code are open, and how each is resolved | [Conflict resolution plan](conflict-resolution-plan.md): both sides, the recommended resolution, its cost, and whether it is applied |
| Owner decisions from the 2026-11-17 architecture review that amend earlier documents | [ADR-0013](../adr/0013-stored-browser-sessions.md) stored browser sessions; [ADR-0014](../adr/0014-acknowledgement-classes-and-agent-os-latency.md) acknowledgement classes and the agent-os target; [ADR-0015](../adr/0015-agent-isolation-modes.md) agent isolation modes; [ADR-0016](../adr/0016-no-application-level-data-encryption.md) no application-level data encryption. Each affected document carries a dated amendment note Also from the same review: [ADR-0004](../adr/0004-headless-agent-sessions.md) headless sessions, chat interaction and no browser terminal; [ADR-0017](../adr/0017-linux-and-container-hosting.md) macOS and Linux including containers; [ADR-0018](../adr/0018-storage-engine-independence.md) replaceable storage engine. [ADR-0019](../adr/0019-office-coded-task-numbers.md) office-minted task references is **accepted** and applied to the identity contract (a letter code, tasks only, one sequence per office). Owner requirements received in the same review: [REQ-04](../specs/req-04-channel-messaging-and-question-components.md) questions to people and [REQ-05](../specs/req-05-virtual-console-conversation-log-viewer.md) the session view. |
| What must be preserved from earlier versions | Nothing. Nothing is deployed, so there is no compatibility, migration or legacy-import obligation before v1 ships: [organization-data index](organization-data/README.md#no-compatibility-obligations-before-v1-ships) |
| Names | The executables are `agentic-org`, `agentic-org-office`, `agentic-org-web` and `agent-os`. The browser-facing process is "web"; the word gateway survives only in code identifiers until they are renamed |
| Current runtime/wire/storage behavior before migration | The actual code/contracts and the explicitly historical sections of older designs; preserve existing compatibility and safety obligations |

For selected target data, the organization-data contract takes precedence over contradictory older relationship, identifier and folder examples. An old design's approved status/date does not supersede later owner decisions. Conversely, the target draft does not prove code or migrations have landed. Keep current-runtime evidence and selected-target rules explicitly separated. New conflicts require a recorded decision; do not infer a missing engine from a data field.

## Reconciled implementation and UX guides

Use these guides for concrete flows, examples and acceptance criteria. Follow the
primary contract when a secondary guide's level of detail does not cover a field;
do not reconstruct a target entity from an old API example. Criterion IDs retained
from older specs may now describe a selected target, a current-runtime check or a
deferred proposal; the criterion's text and scope decide which evidence is needed.
An old revision's claim that it is approved does not activate its deferred engine.

| Concern | Detailed guides | Primary contract |
| --- | --- | --- |
| Navigation, shared visual/interaction rules and the owner decision loop | [Shell](../specs/app-shell-ux.md), [Home/Inbox](../specs/our-org-in-the-app.md) | Selected identity/access contracts; individually gated services in those specs |
| Organization bootstrap, home teams, leads and project accountability | [Creation](create-organization.md), [team model](teams-and-org-chart.md), [office/team UX](../specs/offices-and-teams-ux.md), [hiring/profile UX](../specs/our-org-in-the-app.md), [roles](../specs/roles-ux.md) | 01, 02, 04, 06, 08 |
| Typed tasks, assignment, reporter, history and private notes | [Task design](tasks-projects-docs.md), [task UX](../specs/work-items-ux.md), [request UX](../specs/feature-requests-ux.md) | 03, 05, 06, 08, 12 |
| Repository associations, feature/knowledge/release content | [Git design](projects-git-github.md), [features](../specs/features-ux.md), [knowledge](../specs/decisions-and-tech-debt-ux.md), [status reports](../specs/project-status-ux.md) | 02, 03, 07, 08, 09 |
| Execution configuration, routing, placement and recovery | [Sessions](agent-sessions.md), [supervision](employee-supervision.md), [employee UX](../specs/employee-supervision-ux.md), [run-mode UX](../specs/agent-run-modes-ux.md), [smart features](smart-features.md) | 04, 10, 11, 14, 16, 17 |
| Messaging, accounting and budget controls | [Messaging ADR](../adr/0003-agent-messaging-office-first.md), [chat UX](../specs/chat-ux.md), [cost UX](../specs/costs-and-insights-ux.md) | 10, 15, 16, 17 |
| Durable source layout, API adaptation and acceptance | [Storage ADR](../adr/0002-storage-sqlite-sqlcipher-one-writer.md), [offices](offices-hq-and-satellites.md), [host boundary](office-host-boundary.md), [HTTP contract](web-api-v1.md), [standalone design](standalone-server.md), [installation UX](../specs/server-installation-and-access-ux.md), [dogfood](../specs/dogfood-run.md) | 01, 04, 06, 11, 15, 16, 17 |
| Structural processes versus unselected engines | [Process design](processes.md), [process UX](../specs/processes-ux.md), [retrospectives proposal](../specs/retrospectives-ux.md) | 01 and the selected/pending scope in the data index |

Current CLI/library READMEs describe implemented interfaces. Their target mappings
are adaptation requirements, not newly supported commands. The
[local-HQ runbook](../runbooks/local-hq.md) must distinguish commands usable against
the checked source from installation or recovery procedures still awaiting
implementation. Research/spike records and pinned test evidence remain historical
observations; changing them to match a target would falsify their results.

## Selected relationships and references

- Organization → recursive team tree; each employee has one home team. A lead is a direct member appointed to manage the subtree; an ordinary ancestor-team member gains no such authority. Explicit person-to-person reporting remains distinct.
- A project requires one accountable owner team and may have contributing teams. Teams/projects are organization-wide; offices place agents, not teams/projects.
- A task always has an accountable team and optionally a project. Task/bug/request are task types, all using T- references. Assignment is independent of accountable ownership. Preserve old stable IDs and resolve old B-/REQ- references through explicit migration mappings, never by stripping/reusing the number.
- TEAM- and P- counters are organization-scoped; T- is project-scoped or team-scoped when projectless; F-/R- are project-scoped; M- belongs to its release. Slugs/handles follow document 06. Organization/office IDs do not get new human counters.
- A repository is registered once under the organization and may be shared by projects. Integration branches belong to each project/repository association, never to the organization cache. Local code repositories are supported; the organization metadata repository itself must be remote.
- Features use project-prefixed slugs and stable-ID file paths in organization Git. Feature definitions explain behavior/limitations; supporting project/feature/release/deployment documents have the ownership defined in document 02. Release product changelogs and deployment technical changelogs remain distinct.
- A built-in HR agent is an employee and therefore requires a home team; bootstrap must choose a valid team explicitly before admitting the profile. Its special chart placement is presentation, not an identity exception.
- An authenticated owner account remains a security principal. If represented by an employee/person record, that record has the same required home-team constraint; displaying the owner above the chart does not create a teamless employee or grant authority from a YAML flag.

## Folder and privacy boundaries

Portable definitions/open work live in organization Git; task work and code live in runtime worktrees. Supporting knowledge uses a required project scope and at most one same-project feature/release/deployment scope. Release content and intended flags are authored records; deployment commit/flag snapshots record what was actually applied. These fields do not implement deployment or flag evaluation. Protected chat, budget amounts/history, private notes, usage archives, command receipts and recovery evidence are not made public by putting their SQL projection in one draft schema. Each source's visibility and durability boundary still applies. Private task notes
are author-only by default, including against organization owners/administrators;
existing shared note comments are not private. Published task history is immutable
shared evidence and must contain no private-note body or identifying metadata.

The target runtime layout is defined by [employee structure](organization-data/04-agent-structure.md#runtime-location):

```text
runtime/<office-id>/
├── repositories/<repository-id>.git/              # cache, no project integration branch
└── teams/<root-team-id>/teams/<child-team-id>/.../
    └── agents/<employee-id>/
        ├── projects/<project-id>/tasks/<task-id>/repositories/<repository-id>/
        └── tasks/<task-id>/repositories/<repository-id>/   # projectless task
```

Workspaces survive handoff until task closure; changing current agent/office placement requires an explicit safe workspace migration, not recursive deletion. This diagram defines ownership, not a completed sandbox migration. Re-derive Git metadata, credential, other-task and managed-state denies for the target paths and verify them before launching against that layout. Do not transplant old allowlists based on a renamed prefix.

Deleting an office cache is safe only with protected acknowledged sources, rebindable credentials and process reconciliation. A Git checkout alone does not restore chat, budgets, usage or authority. Preserve live Phase A asks/replies/triage until the separately verified transport cutover.

## Deferred requirements and historical evidence

Areas, Undo, advanced checklist execution, retrospectives, automatic scheduling/memory and the process engine remain unselected. Older UX acceptance criteria for them remain earlier requirements, not approval to port them. Process/standard content and override flags are selected structural data; inherited resolution engines remain separate work. Budgets and recovery are selected as data/control contracts; their runtime evaluators, pause coordination and restore services remain implementation work.

Compatible cost/budget, duration and target-reader requirements from earlier
PRDs are integrated in the existing cost UX and primary documents 10/17. Those
current documents govern. Older current-assignment fallback, movement of costs
through current plans, parent-budget aggregation, special refusal of a raise at
or below spent usage and disconnected per-office allowances remain superseded.
Detailed analytics, account-zone persistence/digests, additional model-policy
slots and further smart-feature candidates are individually gated in their
owning guides. Older PRD release labels do not establish an implemented service.

Implement the semantic changes against the current selected contracts. A new
acceptance criterion is a target requirement, not evidence of delivery; a deferred
or unresolved item needs its stated contract/decision before implementation.
Historical source documents and personal work journals are not additional
independent implementation checklists.

Historical commit links, retired wire examples, required signing identifiers/key namespaces and installed legacy CLI instructions are evidence, not current source/package naming. Do not rewrite cryptographic identities or switch live tooling through documentation cleanup. Current source/package/executable names are agentic-org, agentic-org-office, agentic-org-web and agent-os; org-host is a library. Setup/service/download/update requirements must continue to be distinguished from implemented entry points.
