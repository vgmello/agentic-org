# Documentation authority and selected v1 model

This is the reading map for the selected organization data design after PRs #6 and #12. It does not change application behavior, approve a migration or select deferred engines.

## Which document answers which question?

| Question | Authoritative reference |
| --- | --- |
| Selected entity relationships, fields, canonical files and draft SQL | [Organization data index](organization-data/README.md) and its owning documents |
| Teams, home membership, reporting and accountable project ownership | [Organization](organization-data/01-organization-structure.md), [projects](organization-data/02-project-structure.md), [employees](organization-data/04-agent-structure.md) |
| Typed tasks, retained bug/request detail, feature lifecycle and references | [Tasks/features](organization-data/03-features-and-work.md), [numbering](organization-data/06-identity-and-numbering.md), [assignment](organization-data/12-task-assignment-and-history.md) |
| Chat source data, import visibility and hashes | [Messaging](organization-data/15-messaging-and-chat.md) |
| Durable evidence versus disposable office state | [Recovery](organization-data/16-recovery-and-office-continuity.md), plus [private notes](organization-data/05-operational-data.md#private-notes-attached-to-tasks) and [accounting](organization-data/10-usage-and-cost-accounting.md) |
| Budget definitions, periods, extensions, notices and holds | [Budgets](organization-data/17-budgets-and-controls.md); visible [cost UX](../specs/costs-and-insights-ux.md) for presentation and access rules |
| Execution model/profile/permit/failover data and responsibilities | [Execution](organization-data/11-model-profiles-and-agent-execution.md), [provider/pool data](organization-data/14-model-providers-and-connection-pools.md) |
| Currently implemented executable names and unavailable commands | [Executable migration](executable-migration.md), verified against source at the implementation commit being discussed |
| Installation, service and privilege-boundary requirements | [Standalone design](standalone-server.md), [installation UX](../specs/server-installation-and-access-ux.md), ADR-0005 |
| Current runtime/wire/storage behavior before migration | The actual code/contracts and the explicitly historical sections of older designs; preserve existing compatibility and safety obligations |

For selected target data, the organization-data contract takes precedence over contradictory older relationship, identifier and folder examples. An old design's approved status/date does not supersede later owner decisions. Conversely, the target draft does not prove code or migrations have landed. Keep current-runtime evidence and selected-target rules explicitly separated. New conflicts require a recorded decision; do not infer a missing engine from a data field.

## Selected relationships and references

- Organization → recursive team tree; each employee has one home team. A lead is a direct member appointed to manage the subtree; an ordinary ancestor-team member gains no such authority. Explicit person-to-person reporting remains distinct.
- A project requires one accountable owner team and may have contributing teams. Teams/projects are organization-wide; offices place agents, not teams/projects.
- A task always has an accountable team and optionally a project. Task/bug/request are task types, all using T- references. Assignment is independent of accountable ownership. Preserve old stable IDs and resolve old B-/REQ- references through explicit migration mappings, never by stripping/reusing the number.
- TEAM- and P- counters are organization-scoped; T- is project-scoped or team-scoped when projectless; F-/R- are project-scoped; M- belongs to its release. Slugs/handles follow document 06. Organization/office IDs do not get new human counters.
- A repository is registered once under the organization and may be shared by projects. Integration branches belong to each project/repository association, never to the organization cache. Local code repositories are supported; the organization metadata repository itself must be remote.
- Features use project-prefixed slugs and stable-ID file paths in organization Git. Feature definitions explain behavior/limitations; supporting project/feature/release/deployment documents have the ownership defined in document 02. Release product changelogs and deployment technical changelogs remain distinct.
- An authenticated owner account remains a security principal. If represented by an employee/person record, that record has the same required home-team constraint; displaying the owner above the chart does not create a teamless employee or grant authority from a YAML flag.

## Folder and privacy boundaries

Portable definitions/open work live in organization Git; task work and code live in runtime worktrees. Protected chat, budget amounts/history, private notes, usage archives, command receipts and recovery evidence are not made public by putting their SQL projection in one draft schema. Each source's visibility and durability boundary still applies.

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

The referenced costs-and-budgets-v1 PRD is not present in this repository. It cannot silently override available reviewed contracts. Preserve its references as historical context; do not invent its absent decision tables. Uncovered rules stay pending explicit decisions.

Historical commit links, retired wire examples, required signing identifiers/key namespaces and installed legacy CLI instructions are evidence, not current source/package naming. Do not rewrite cryptographic identities or switch live tooling through documentation cleanup. Current source/package/executable names are agentic-org, agentic-org-office, agentic-org-web and agent-os; org-host is a library. Setup/service/download/update requirements must continue to be distinguished from implemented entry points.
