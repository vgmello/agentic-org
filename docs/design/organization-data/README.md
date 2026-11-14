# Organization design document set

Structural draft for v1. Document revision 2.0. This proposal specifies data relationships and storage. It does not change the implementation, authorize a migration, or select additional v1 capabilities for porting.

Existing v1 designs remain the implementation reference until a migration is approved: [teams and organization chart](../teams-and-org-chart.md), [tasks and projects](../tasks-projects-docs.md), and [project Git integration](../projects-git-github.md). This directory describes the proposed replacement data model and records pending decisions.

## Documents and boundaries

| Document | Owns | Does not define |
| --- | --- | --- |
| [Organization structure](01-organization-structure.md) | Organization, offices, team tree, roles, repository registrations, structural process/standard documents | Process-engine execution or replication algorithms |
| [Project structure](02-project-structure.md) | Project ownership, repository associations, integration branches, releases and milestones | Agent provisioning or task lifecycle engine |
| [Features and work](03-features-and-work.md) | Explicit features, tasks, bugs, requests, checklists, comments and links | Areas, advanced Undo or additional v1 workflows not selected yet |
| [Agent structure](04-agent-structure.md) | Employee identity, home team, relationships and placement boundary | Agent setup; separate specification pending |
| [Operational data](05-operational-data.md) | Watchers, owner questions and waiting spans | Notification delivery, timers, scheduling or process-engine behaviour |

Each document contains its own table diagrams, field sources, examples and table DDL. [schema.sql](schema.sql) combines those tables with indexes and team-cycle checks. Load all sections together: cross-document foreign keys are intentional. A diagram's unexpanded entities are references to their owning document.

## Shared identity contract

- `id` is the immutable stable identifier and SQL primary key. Preserve existing v1 IDs exactly; do not regenerate them, lowercase them, or replace them with folder names.
- `key` is a separate readable identifier. It is not the primary key and never replaces IDs in foreign keys. Existing readable v1 references can populate appropriate keys during an explicitly approved import.
- Keys are optional while importing existing records. When present, they are unique within the scopes below; changing a key does not change identity.
- Names are display labels. Directory segments use stable IDs; the examples sometimes show readable team names to explain the hierarchy, not to prescribe a new ID format.
- `schema_version` selects the file format; `revision` is the entity's concurrency revision. Dates and timestamps are quoted strings in YAML. All domain rows' `organization_id` values come from their containing `organization.yaml`.

| Entity | Readable key scope |
| --- | --- |
| Organization | Organization catalog |
| Office, repository, role, project, employee | Organization, independently per entity type |
| Team | Parent team; root teams share the organization's root scope |
| Feature, release, task, bug | Project, independently per entity type |
| Milestone | Release |
| Request | Organization, including requests not routed to a project |

Nested comments, checklist steps, watchers, questions and waiting spans have stable IDs within their owning record; they do not need additional readable keys.

## Shared storage contract

The organization repository must be remote. Its tracked content is mirrored as ordinary durable local files. Code repositories may be remote or local-only. All active Git clones and worktrees live under runtime.

```text
<data-root>/<organization-id>/
├── durable/
│   ├── state/                 # Latest organization repository content
│   ├── journal/               # Pending local persistence; not Git content
│   ├── local/offices/         # Machine-specific bindings; not Git content
│   └── checkpoints/           # Local-only/unpushed code recovery bundles and edits
└── runtime/<office-id>/
    ├── office.db              # Projection + explicitly transient state
    ├── repositories/          # One bare clone per repository used by this office
    └── teams/<team-id>/.../agents/<employee-id>/
        └── projects/<project-id>/tasks/<task-id>/repositories/<repository-id>/
```

Durable scalar values are explicit YAML/front-matter fields. Association rows come from ID arrays or maps in their owning documents. Markdown bodies map to `body_markdown`. Array positions and resolver indexes are derived and have no independently authored source. Null optional keys and documented defaults round-trip identically.

SQLite is the indexed projection for domain records. Runtime placements, cache paths and worktree paths are deliberately transient. Existing authentication/credential storage remains a separate subsystem and is not made disposable by this schema. Durable files do not contain credentials.

Structural import validation rejects duplicate IDs, invalid references, team cycles, path/field mismatches and a team lead outside that team's direct membership. A lead manages the team's entire subtree; ordinary members gain no management authority from their location. This describes the relationship only, not tool permissions or authorization implementation.

Saving files and Git publication remain separate. Hourly pending-work publication and prompt important-knowledge publication are publication targets, not a newly implemented sync engine. This set does not mandate an HQ-only work writer: multi-office ownership and synchronization must be designed without silently removing local-office capabilities.

## Selected and pending v1 decisions

| Topic | Current decision |
| --- | --- |
| Stable IDs | Keep; add readable keys without re-identifying records |
| Single home team, nested teams, lead authority | Include the agreed structure |
| Offices | Durable organization records; support multiple offices |
| Remote organization Git and local-only code repositories | Include both requirements |
| Watchers, owner questions, waiting spans | Include their data structures in document 05 |
| Agent setup | Separate specification pending; do not port old setup automatically |
| Process engine | Out of scope for now; retain process/standard content and scope metadata only |
| Areas, advanced checklist workflows, Undo, full event history, budgets, retrospectives | Still unselected; do not silently port or delete them |

The SQL is a structural draft for the selected scope. Runtime startup, agent setup, mutation transitions, publication/replication, analytics retention and remaining v1 migration choices require their own approved contracts. Existing data not mapped by this set must stay intact in import staging until a disposition is chosen.
