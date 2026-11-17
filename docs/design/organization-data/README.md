# Organization design document set

Status: **selected** data contract for v1, set revision 2.11 (2026-11-17). It specifies the selected data relationships and storage, including task assignment and activity history. The owner authorized moving the code to this model on 2026-11-17, one record family at a time; this document set does not itself change the code. See [Status and changelog](#status-and-changelog).

This set owns the **selected target data relationships, fields and canonical sources**. Current code and older designs remain evidence of existing behavior until an explicit migration is approved; they do not override later selected data decisions. The [documentation authority map](../documentation-authority.md) links each owning contract and distinguishes historical requirements, implementation status and deferred scope.

## Documents and boundaries

| Document | Owns | Does not define |
| --- | --- | --- |
| [Organization structure](01-organization-structure.md) | Organization, offices, team tree, roles, repository registrations, structural process/standard documents | Process-engine execution or replication algorithms |
| [Project structure](02-project-structure.md) | Project ownership, repository associations, integration branches, releases/milestones and project/feature document records | Agent provisioning or task lifecycle engine |
| [Features and work](03-features-and-work.md) | Feature definitions and linked knowledge, typed tasks, record identities and tombstones | Areas, advanced Undo or additional v1 workflows not selected yet |
| [Agent structure](04-agent-structure.md) | Employee identity, home team, relationships and placement boundary | Memory storage or consolidation implementation |
| [Operational data](05-operational-data.md) | Shared comments, checklists, watchers, questions, waiting spans, task history and private notes (including mirrors, receipts and retention) | Notification delivery, timers, scheduling or process-engine behaviour |
| [Identity and numbering](06-identity-and-numbering.md) | Stable IDs, readable references, slugs, handles and durable scoped allocations | Service deployment, failover or messaging implementation |
| [Release deployments](07-release-deployments.md) | Deployment attempts, environments, outcomes, rollback references and commit snapshots | Build artifacts or deployment execution |
| [Record contracts](08-record-contracts.md) | Accountability, provenance, lifecycle, work classification, typed relationships and the hash codec registry | Cost/budget engines, permissions or organization-wide event history |
| [Release content and flags](09-release-content-and-flags.md) | Explicit release contents, product/technical changelogs, flag definitions, intended settings and applied snapshots | Flag evaluation or deployment execution |
| [Usage and cost accounting](10-usage-and-cost-accounting.md) | Durable usage events, historical attribution, price versions, checkpoints and derived totals | Budgets, automatic pausing, actual billing or execution routing |
| [Model profiles and agent execution](11-model-profiles-and-agent-execution.md) | Provider/subscription connections, ordered model profiles, agent execution configuration, office bindings, permits, turns/attempts/processes and durable placement generations | Agent memory, automatic scheduling, installation implementation or task lifecycle |
| [Task assignment and history](12-task-assignment-and-history.md) | Owning versus assigned team, current individual assignment, reporter semantics, v1 evidence and migration boundaries | Private task-note application implementation, Undo, all v1 transition engines or a deployed mutation/replication service |
| [Model providers, connections and pools](14-model-providers-and-connection-pools.md) | Provider/driver identity, connections, ordered pools, office bindings, discovered models and agent selection in the common data-structure format | A second provider system, additional tables or supervisor/failover implementation |
| [Messaging and chat](15-messaging-and-chat.md) | Conversations, memberships, messages, recipient state, attachments, read markers and reactions; runtime ownership and routing rules merged from ADR-0003 | Transport/wake engines and external bridges |
| [Recovery and office continuity](16-recovery-and-office-continuity.md) | Mutation receipts, origin streams, checkpoints, snapshots and process cleanup evidence | Automatic promotion, restore engines or backup automation |
| [Budgets and controls](17-budgets-and-controls.md) | Task, agent and release policies, periods, threshold notices, extensions and holds | Budget service, automatic pausing or distributed admission implementation |

Each numbered document owns its relationships, field sources, examples and table DDL. [schema.sql](schema.sql) combines the primary documents, including model profiles/execution and task assignment/history, into one shared draft containing 75 tables and 10 views plus indexes/triggers. Document 14 describes the provider and pool records and links to document 11 for their SQL. Load it once; no separately applied execution extension remains. Document 05 defines [private-notes-schema.sql](private-notes-schema.sql) for a separate durable private database with 4 tables; never combine it with the shared projection. [Schema coverage](SCHEMA-COVERAGE.md) maps records and enforcement boundaries. Every write connection must enable `foreign_keys` and `recursive_triggers` as the scripts do. Cross-document foreign keys are intentional. A diagram's unexpanded entities refer to their owning document.

## Shared identity contract

Every entity keeps an immutable stable `id`, used for foreign keys, synchronization and filenames. IDs are UUID version 7 (RFC 9562), written as lowercase hyphenated text (owner decision 2026-11-17; [document 06](06-identity-and-numbering.md#stable-ids-readable-references-and-scopes) has the rule and the reason). The newer HQ-creation code already mints them; the rest of the code mints ULIDs and changes under FC-142. Readable identifiers never replace stable IDs.

| Entity | Readable identifier | Uniqueness / counter scope |
| --- | --- | --- |
| Organization | `slug`, e.g. `restaurant-saas` | Organization catalog; no counter |
| Office | `slug`, e.g. `hq` | Organization; no counter |
| Agent or person | `handle`, displayed as `@ana` | Organization, shared across both employee kinds; no counter |
| Team | `TEAM-12` | Team counter within organization |
| Project | `P-8` and `slug`, e.g. `reservations` | Project counter and slug uniqueness within organization |
| Task of any type | `T-B37`: `T-`, the creating office's letter code, a number | One sequence per office, minted by that office; unique in the organization ([document 06](06-identity-and-numbering.md)) |
| Feature | `F-6` and full slug `reservations/table-booking` | Feature counter and `local_slug` within project; full slug unique within organization |
| Release | `R-3` and optional text `version_label` | Release counter within project |
| Milestone | `M-2` | Milestone counter within release |
| Repository, role, provider connection or model profile | Descriptive `key` | Organization, separately per entity type; no counter |

Numbered records persist `number`; SQL derives `key` as the prefix plus number. The canonical file does not need a duplicate `key`; if supplied on import it must match. `number: null` denotes a locally created feature awaiting allocation and exposes no final readable reference. A task is numbered by its creating office at creation, so it is never null. Changing task type never changes its ID or number.

Slugs are lowercase kebab-case identifiers; handles are lowercase identifiers using letters, digits, dots, underscores or hyphens, stored without `@`. They are explicit fields rather than values recalculated when display names change. Teams need no additional slug. Projects have both a numeric reference and an organization-unique slug. Features store `local_slug`, and their public `slug` is automatically derived as `<project.slug>/<feature.local_slug>`. Never manually enter the project prefix on a feature. The resulting path is unique within the organization because both component scopes enforce uniqueness. Release version labels are plain text, not an enforced semantic-version type or identity.

`schema_version` selects the file format; `revision` is the entity's concurrency revision. Dates/timestamps are quoted strings in YAML. Domain rows inherit `organization_id` from `organization.yaml`. Nested comments, checklist steps, watchers, questions and waiting spans have stable IDs without counters. Task events retain the stable `(organization_id, task_id, revision)` identity described in document 05; revisions are not another readable-number allocation. See [identity and numbering](06-identity-and-numbering.md) for allocation, durability and index contracts.

## Common record contracts

[Record contracts](08-record-contracts.md) defines owner fields, common creator/update metadata, lifecycle states, typed relationships and Build/Run classification. All authored entity YAML/front matter stores the same-named SQL scalar fields. Defaults are only those explicitly documented; missing historical provenance stays unknown. Examples show selected fields rather than complete accepted-write payloads. Runtime-only rows and association projections do not become independently authored records.

## Shared storage contract

The organization repository must be remote. Its tracked content is mirrored as ordinary durable local files. Code repositories may be remote or local-only. All active Git clones and worktrees live under runtime.

```text
<data-root>/<organization-id>/
├── durable/                         # Sources. Back up all of it. Cache cleanup never touches it.
│   ├── state/                       # Latest organization repository content (published to Git)
│   ├── journal/                     # Pending local persistence; not Git content
│   ├── authority/                   # Numbering ledger and its recovery journal (06); HQ placement
│   │                                #   generations, permits and cooldowns (11)
│   ├── accounting/                  # Usage streams/checkpoints (10)
│   ├── messaging/                   # Protected conversations, messages, attachments (15)
│   ├── recovery/                    # Receipts, origin streams, acceptance journals, checkpoints (16)
│   ├── budgets/                     # Protected policies, periods, events, holds (17)
│   ├── private/task-notes.db        # HQ only: author-private task notes (05)
│   ├── local/offices/<office-id>/   # Machine-specific bindings, auth and supervision journal (11);
│   │                                #   agents/<employee-id>/home/, each agent's private home (ADR-0015)
│   └── checkpoints/                 # Local-only/unpushed code recovery bundles and edits
└── runtime/<office-id>/             # Caches. Rebuildable; read document 16 before deleting.
    ├── office.db                    # Projection + explicitly transient state
    ├── repositories/                # One bare clone per repository used by this office
    └── teams/<team-id>/.../agents/<employee-id>/
        ├── projects/<project-id>/tasks/<task-id>/repositories/<repository-id>/
        └── tasks/<task-id>/repositories/<repository-id>/  # Team tasks
```

**One root, two halves.** This tree is the only selected on-disk layout; every numbered document and every design or UX guide refers to it. `durable/` holds every organization data source: nothing under it is rebuilt from anything else, a backup must include all of it, and no cleanup may remove it. `runtime/<office-id>/` holds only what can be rebuilt from `durable/`, organization Git and verified probes. Only `durable/state/` is published to organization Git. `messaging/`, `recovery/`, `budgets/`, `accounting/`, `authority/`, `private/` and `local/` are protected local sources and never become Git content.

`<data-root>` is resolved once by the instance under the [standalone design](../standalone-server.md#3-instance-selection-and-storage) and saved in instance configuration; it is never inferred from a client ID, a shell's working directory or `HOME`. An operator may place `durable/` on separate storage, but its logical location stays `<data-root>/<organization-id>/durable/`. Earlier revisions used other names for parts of this tree: `<organization-data-root>` is `<data-root>`, `<organization-operational-root>` is `<data-root>/<organization-id>/durable`, and `<office-state>/cache/` is `<data-root>/<organization-id>/runtime/<office-id>/`. Do not introduce another root name. Two things are deliberately outside this tree and are backed up separately: the host's key and writer custody, and credential stores (document 16; [ADR-0002](../../adr/0002-storage-sqlite-sqlcipher-one-writer.md)).

This is the selected target. The checked code still keeps its authoritative `org.db` and `office.db` under `offices/<office-id>/` of the host home ([ADR-0002](../../adr/0002-storage-sqlite-sqlcipher-one-writer.md), [host boundary](../office-host-boundary.md)); those databases are sources, not caches, until an approved migration moves each record family here.

Durable scalar values are explicit YAML/front-matter fields. Association rows come from ID arrays or maps in their owning documents. Markdown bodies map to `body_markdown`. Array positions and resolver indexes are derived and have no independently authored source. Generated numeric keys are derived from persisted numbers. Null optional fields and documented defaults round-trip identically.

Shared SQLite is the indexed projection for domain records, task activity and accounting events. The private-note database is an explicit separate durable store, not a reconstructible shared cache. Published task activity lives in organization Git under `work-history/tasks/`; acknowledged unpublished changes also require their durable journal. Accounting recovery additionally requires the durable usage archive; rates alone or organization Git alone cannot restore accounting per-event history. Cache paths and worktree paths are transient. Document 11 requires durable placement/fencing evidence before execution; recreating an office database does not authorize starting another copy of an agent. Existing authentication/credential storage remains a separate subsystem and is not made disposable by this schema. Under [ADR-0016](../../adr/0016-no-application-level-data-encryption.md) organization data is not encrypted by the application: databases and files under this root are protected by disk encryption, private file permissions and the agent isolation mode of [ADR-0015](../../adr/0015-agent-isolation-modes.md). Private keys and credentials never live in plaintext in a database or in Git content: an office's private keys are stored wrapped by a host-held key, and vendor sign-in state has its own owner-only directory under `durable/local/` (document 11). Shared organization files do not contain credentials, private task notes or chat bodies. Document 15 defines protected messaging sources outside disposable office caches; Git alone does not restore chat.

Structural import validation rejects duplicate IDs, invalid references, team cycles, path/field mismatches and a team lead outside that team's direct membership. A lead manages the team's entire subtree; ordinary members gain no management authority from their location. This describes the relationship only, not tool permissions or authorization implementation.

Saving files and Git publication remain separate. Hourly pending-work publication and prompt important-knowledge publication are publication targets, not a newly implemented sync engine. Offices host agents and cache organization data; they do not own teams, projects or counters. HQ creates teams, projects, releases and milestones. Tasks and features may be created and durably saved in any office, including while disconnected. Offices submit their stable IDs and scopes to the shared organization authority for final number allocation and feature-slug registration; they never increment replicated counters independently. Numbering recovery must preserve every acknowledged allocation; hourly Git publication alone cannot provide that guarantee.

## No compatibility obligations before v1 ships

Nothing has been deployed. Until the owner declares v1 shipped (owner decision 2026-11-17):

- There are no existing users, organizations or data to preserve. No migration, import staging, alias, compatibility adapter or legacy shape is built or kept for a pre-release form.
- Wire names, method names, file formats, schemas, identifiers and the local API version may change freely. The local API version is 1.
- Where a document says to preserve, import, adopt, stage or map "v1", "legacy" or "existing" data, identifiers, references such as `B-` and `REQ-`, hashes or events, that requirement is withdrawn and the selected shape is built directly. This covers the legacy adoption profile of document 05 and ADR-0010, the imported `directed_thread` conversation kind, the `v1_lenprefixed_sha256` and schema-1 task-event codecs, and import staging. The draft schema still carries some of these structures; removing them is tracked in the [conflict resolution plan](../conflict-resolution-plan.md).
- The better design wins. When rebuilding is the cleaner architecture, rebuild; the result must be fast, memory-efficient and reliable.
- This rule is about the product's documents and code. It does not authorize deleting files on a real machine, such as the installed tool and spool that `AGENTS.md` says to leave alone.
- **One open point.** `AGENTS.md` and the cutover section of ADR-0003 require the development team's own Phase A message queue to be carried over (FC-082). That cutover uses the `directed_thread` kind, the `v1_lenprefixed_sha256` codec and import staging. Until the owner says whether the queue is carried over or dropped, those three stay in the draft schema for that purpose only.

## Outside this set: what the host owns

The instance, accounts, sessions and the office registry belong to the host, not to this set. They are defined in [status and host records](STATUS.md#outside-this-set-what-the-host-owns).

## Organization authority

In v1 the **organization authority is the HQ office worker**. It is a role of that process, not a separate service or daemon: the single writer at HQ owns `durable/authority/`, allocates readable numbers, registers feature slugs and flag keys, accepts usage corrections, issues turn permits and cooldowns, and holds placement generations and budget-control state. Wherever a document in this set says "shared organization authority", "shared logical organization authority" or "HQ's organization service", it means this role. Other offices submit requests to it and never act as it, including while disconnected. Moving the role to another office is promotion; it is manual, fenced by epoch, and deferred ([offices design](../offices-hq-and-satellites.md), [document 16](16-recovery-and-office-continuity.md)).

## Selected and pending v1 decisions

| Topic | Current decision |
| --- | --- |
| Stable IDs | UUIDv7, plus readable identifiers |
| Tasks | One entity with required team, optional project and extensible type |
| Task responsibility | Required accountable `team_id`; independent nullable `assigned_team_id` and individual `assignee_id`; assignment never renumbers a task |
| Reporter and task history | Immutable creator as reporter/originator; ordered task events preserve assignment, status and other task changes after closure/archival |
| Private task notes | Required: author-private notes attached to tasks, separate from shared comments, with acceptance criteria PN-01–06 and data structure in document 05/private-notes-schema.sql; v1 runtime, access controls and UI remain unimplemented |
| Readable references | `TEAM-`, `P-`, `T-`, `F-`, `R-`, `M-`; slugs/handles for selected identities |
| Single home team, nested teams, lead authority | Include the agreed structure |
| Offices | Durable organization records; support multiple offices |
| Creation authority | HQ creates teams/projects/releases/milestones; any office creates tasks/features |
| Organization authority | The HQ office worker in v1: one role for numbering, slug/key registration, permits, placement generations, budget-control state and corrections; promotion deferred |
| Project/feature slugs | Organization-unique project slug; feature full slug automatically prefixed by project |
| Knowledge documents | Required project, optional same-project feature/release/deployment scope; feature definitions stay in `feature.md` |
| Accountability and lifecycle | Feature/document owners; active/archived teams; employees `active`, `paused` (agents) or `inactive` in one status field; draft/current/deprecated documents |
| Task, milestone and release completion | Task status is the closed list `todo`, `in_progress`, `blocked`, `done`. Milestones and releases store no status; done is derived from their tasks |
| Compatibility before v1 ships | None. Nothing is deployed: no migration, import, alias or legacy shape is kept ([rule](#no-compatibility-obligations-before-v1-ships)) |
| Attribution and relationships | Common creation/update actors and times; typed authored links and derived inverse/scope relationships |
| Encryption at rest | None for data at the application level (ADR-0016); disk encryption, file permissions and agent isolation (ADR-0015) protect it. An office's private keys are stored wrapped by a host-held key |
| Budgets | Optional task limits, daily/monthly agent limits and release warning caps; protected definitions/operational controls in document 17, runtime service still unimplemented |
| Build / Run | Classification data captured in usage snapshots; no automatic budget or pause engine |
| Token/cost accounting | Durable usage ledger, model prices, historical attribution and derived task/agent/project totals; incomplete data stays explicit |
| Delivery | Release product changelog and content manifest; deployment technical changelogs, commit/flag snapshots; build artifacts remain out of scope |
| Remote organization Git and local-only code repositories | Include both requirements |
| Watchers, owner questions, waiting spans | Include their data structures in document 05 |
| Agent setup/execution | Document 11 defines model profiles, provider connections and execution data/behavior; memory consolidation and automatic scheduling remain separate pending work |
| Subscription connections | One connection per provider subscription, reused by office-local bindings and profiles; shared quota remains attached to that connection |
| Processes and standards | Content, scope and the override lock are selected, and so is resolution: the effective set for a task is the organization's entries, then the accountable team's chain, then the project's, and entries that may not be overridden are always given to the agent ([processes design](../processes.md)). Enforcement, approvals and checklist composition stay out of scope |
| Areas, advanced checklist workflows, Undo, organization-wide event history, retrospectives | Still unselected; do not silently port or delete them |

## Status and changelog

Every numbered document is **selected**. The status vocabulary, the changelog of this set and what the draft SQL does and does not establish are in [status and host records](STATUS.md#status-and-changelog).
