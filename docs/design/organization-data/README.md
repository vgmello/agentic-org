# Organization design document set

Structural draft for v1. Document revision 2.4. This proposal specifies data relationships and storage. It does not change the implementation, authorize a migration, or select additional v1 capabilities for porting.

Existing v1 designs remain the implementation reference until a migration is approved: [teams and organization chart](../teams-and-org-chart.md), [tasks and projects](../tasks-projects-docs.md), and [project Git integration](../projects-git-github.md). This directory describes the proposed replacement data model and records pending decisions.

## Documents and boundaries

| Document | Owns | Does not define |
| --- | --- | --- |
| [Organization structure](01-organization-structure.md) | Organization, offices, team tree, roles, repository registrations, structural process/standard documents | Process-engine execution or replication algorithms |
| [Project structure](02-project-structure.md) | Project ownership, repository associations, integration branches, releases/milestones and project/feature document records | Agent provisioning or task lifecycle engine |
| [Features and work](03-features-and-work.md) | Feature definitions and linked knowledge, typed tasks, checklists, comments and links | Areas, advanced Undo or additional v1 workflows not selected yet |
| [Agent structure](04-agent-structure.md) | Employee identity, home team, relationships and placement boundary | Agent setup; separate specification pending |
| [Operational data](05-operational-data.md) | Watchers, owner questions and waiting spans | Notification delivery, timers, scheduling or process-engine behaviour |
| [Identity and numbering](06-identity-and-numbering.md) | Stable IDs, readable references, slugs, handles and durable scoped allocations | Service deployment, failover or messaging implementation |
| [Release deployments](07-release-deployments.md) | Deployment attempts, environments, outcomes, rollback references and commit snapshots | Build artifacts or deployment execution |
| [Record contracts](08-record-contracts.md) | Accountability, provenance, lifecycle, work classification and typed relationships | Cost/budget engines, permissions or full event history |

Each document contains its own table diagrams, field sources, examples and table DDL. [schema.sql](schema.sql) combines those tables with indexes and team-cycle checks. Load all sections together: cross-document foreign keys are intentional. A diagram's unexpanded entities are references to their owning document.

## Shared identity contract

Every entity keeps an immutable stable `id`, used for foreign keys, synchronization and filenames. New records use UUIDv7; preserve existing v1 IDs exactly without regenerating or reformatting them. Readable identifiers never replace stable IDs.

| Entity | Readable identifier | Uniqueness / counter scope |
| --- | --- | --- |
| Organization | `slug`, e.g. `restaurant-saas` | Organization catalog; no counter |
| Office | `slug`, e.g. `hq` | Organization; no counter |
| Agent or person | `handle`, displayed as `@ana` | Organization, shared across both employee kinds; no counter |
| Team | `TEAM-12` | Team counter within organization |
| Project | `P-8` and `slug`, e.g. `reservations` | Project counter and slug uniqueness within organization |
| Task of any type | `T-42` | Task counter within project, or within team when projectless |
| Feature | `F-6` and full slug `reservations/table-booking` | Feature counter and `local_slug` within project; full slug unique within organization |
| Release | `R-3` and optional text `version_label` | Release counter within project |
| Milestone | `M-2` | Milestone counter within release |
| Repository or role | Existing descriptive `key` | Organization, separately per entity type; no counter |

Numbered records persist `number`; SQL derives `key` as the prefix plus number. The canonical file does not need a duplicate `key`; if supplied on import it must match. `number: null` denotes a locally created task/feature awaiting allocation, or an unresolved import and exposes no final readable reference. A record accepted by the organization authority must have a number and a durable allocation; local tasks/features can be used by stable ID while registration is pending. Separate scopes can both contain `T-42`: use `P-8/T-42` or `TEAM-12/T-42` outside an already selected scope. Changing task type never changes its ID or number.

Slugs are lowercase kebab-case identifiers; handles are lowercase identifiers using letters, digits, dots, underscores or hyphens, stored without `@`. They are explicit fields rather than values recalculated when display names change. Teams need no additional slug. Projects have both a numeric reference and an organization-unique slug. Features store `local_slug`, and their public `slug` is automatically derived as `<project.slug>/<feature.local_slug>`. Never manually enter the project prefix on a feature. The resulting path is unique within the organization because both component scopes enforce uniqueness. Release version labels are plain text, not an enforced semantic-version type or identity.

`schema_version` selects the file format; `revision` is the entity's concurrency revision. Dates/timestamps are quoted strings in YAML. Domain rows inherit `organization_id` from `organization.yaml`. Nested comments, checklist steps, watchers, questions and waiting spans have stable IDs without counters. See [identity and numbering](06-identity-and-numbering.md) for allocation, durability and index contracts.

## Common record contracts

[Record contracts](08-record-contracts.md) defines owner fields, common creator/update metadata, lifecycle states, typed relationships and Build/Run classification. All authored entity YAML/front matter stores the same-named SQL scalar fields. Defaults are only those explicitly documented; missing historical provenance stays unknown. Examples show selected fields rather than complete accepted-write payloads. Runtime-only rows and association projections do not become independently authored records.

## Shared storage contract

The organization repository must be remote. Its tracked content is mirrored as ordinary durable local files. Code repositories may be remote or local-only. All active Git clones and worktrees live under runtime.

```text
<data-root>/<organization-id>/
├── durable/
│   ├── state/                 # Latest organization repository content
│   ├── authority/             # Durable numbering ledger and recovery journal
│   ├── journal/               # Pending local persistence; not Git content
│   ├── local/offices/         # Machine-specific bindings; not Git content
│   └── checkpoints/           # Local-only/unpushed code recovery bundles and edits
└── runtime/<office-id>/
    ├── office.db              # Projection + explicitly transient state
    ├── repositories/          # One bare clone per repository used by this office
    └── teams/<team-id>/.../agents/<employee-id>/
        ├── projects/<project-id>/tasks/<task-id>/repositories/<repository-id>/
        └── tasks/<task-id>/repositories/<repository-id>/  # Team tasks
```

Durable scalar values are explicit YAML/front-matter fields. Association rows come from ID arrays or maps in their owning documents. Markdown bodies map to `body_markdown`. Array positions and resolver indexes are derived and have no independently authored source. Generated numeric keys are derived from persisted numbers. Null optional fields and documented defaults round-trip identically.

SQLite is the indexed projection for domain records. Runtime placements, cache paths and worktree paths are deliberately transient. Existing authentication/credential storage remains a separate subsystem and is not made disposable by this schema. Durable files do not contain credentials.

Structural import validation rejects duplicate IDs, invalid references, team cycles, path/field mismatches and a team lead outside that team's direct membership. A lead manages the team's entire subtree; ordinary members gain no management authority from their location. This describes the relationship only, not tool permissions or authorization implementation.

Saving files and Git publication remain separate. Hourly pending-work publication and prompt important-knowledge publication are publication targets, not a newly implemented sync engine. Offices host agents and cache organization data; they do not own teams, projects or counters. HQ creates teams, projects, releases and milestones. Tasks and features may be created and durably saved in any office, including while disconnected. Offices submit their stable IDs and scopes to the shared organization authority for final number allocation and feature-slug registration; they never increment replicated counters independently. Numbering recovery must preserve every acknowledged allocation; hourly Git publication alone cannot provide that guarantee.

## Selected and pending v1 decisions

| Topic | Current decision |
| --- | --- |
| Stable IDs | Preserve existing IDs; use UUIDv7 for new records, plus readable identifiers |
| Tasks | One entity with required team, optional project and extensible type |
| Readable references | `TEAM-`, `P-`, `T-`, `F-`, `R-`, `M-`; slugs/handles for selected identities |
| Single home team, nested teams, lead authority | Include the agreed structure |
| Offices | Durable organization records; support multiple offices |
| Creation authority | HQ creates teams/projects/releases/milestones; any office creates tasks/features |
| Project/feature slugs | Organization-unique project slug; feature full slug automatically prefixed by project |
| Knowledge documents | Required project, optional same-project feature; feature definitions stay in `feature.md` |
| Accountability and lifecycle | Feature/document owners; active/archived teams, active/inactive employees, draft/current/deprecated documents |
| Attribution and relationships | Common creation/update actors and times; typed authored links and derived inverse/scope relationships |
| Build / Run | Proposed classification data, informed by v1's Build/R&D and deferred KTLO UX; no cost-engine port |
| Delivery | Release deployment records and commit snapshots; build artifacts remain out of scope |
| Remote organization Git and local-only code repositories | Include both requirements |
| Watchers, owner questions, waiting spans | Include their data structures in document 05 |
| Agent setup | Separate specification pending; do not port old setup automatically |
| Process engine | Out of scope for now; retain process/standard content and scope metadata only |
| Areas, advanced checklist workflows, Undo, full event history, budgets, retrospectives | Still unselected; do not silently port or delete them |

The SQL is a structural draft for the selected scope. Runtime startup, agent setup, mutation transitions, publication/replication, analytics retention and remaining v1 migration choices require their own approved contracts. Existing data not mapped by this set must stay intact in import staging until a disposition is chosen.
