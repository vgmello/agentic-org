# Schema coverage and integrity review

The primary design documents define four SQL artifacts so sensitive operational projections are isolated from shared plaintext `office.db` ([ADR-0016](../../adr/0016-no-application-level-data-encryption.md), `TASK-10`):

- Load [`schema.sql`](schema.sql) once into the shared office projection (`runtime/<office-id>/office.db`): **76 tables and 12 views**.
- Load [`private-notes-schema.sql`](private-notes-schema.sql) into the separate durable private-notes database (`runtime/<office-id>/private/task-notes.db`): **4 tables**.
- Load [`messaging-schema.sql`](messaging-schema.sql) into the separate access-controlled messaging database (`runtime/<office-id>/protected/messaging.db`): **8 messaging tables + 5 identity mirror tables** (`messaging_organizations`, `messaging_teams`, `messaging_projects`, `messaging_employees`, `messaging_offices`).
- Load [`budgets-schema.sql`](budgets-schema.sql) into the separate access-controlled budgets database (`runtime/<office-id>/protected/budgets.db`): **6 budget tables + 4 identity mirror tables** (`budget_organizations`, `budget_tasks`, `budget_employees`, `budget_releases`).

These are draft definitions, not application migrations.

## Record inventory

| Source | SQL artifact | Tables and views |
| --- | --- | --- |
| [01](01-organization-structure.md) | [schema.sql](schema.sql) | `organizations`, `repositories`, `roles`, `teams`, `offices`, `structure_documents`, `local_repository_locations`, `repository_caches` |
| [02](02-project-structure.md) | [schema.sql](schema.sql) | `projects`, `project_contributing_teams`, `project_repositories`, `releases`, `release_repository_commits`, `milestones`, `documents`, `milestone_progress`, `release_progress` |
| [03](03-features-and-work.md) | [schema.sql](schema.sql) | `features`, `tasks`, `task_repositories`, `record_refs`, `tombstones`, `feature_slugs` |
| [04](04-agent-structure.md) | [schema.sql](schema.sql) | `employees`, `agent_placements`, `task_workspaces`, `hire_requests` |
| [05](05-operational-data.md) | [schema.sql](schema.sql) | `comments`, `checklist_steps`, `watchers`, `owner_questions`, `waiting_spans`, `task_events` |
| [06](06-identity-and-numbering.md) | [schema.sql](schema.sql) | `reference_counters`, `reference_allocations`, `office_codes`, `office_task_counters` |
| [07](07-release-deployments.md) | [schema.sql](schema.sql) | `deployments`, `deployment_repository_commits` |
| [08](08-record-contracts.md) | [schema.sql](schema.sql) | `links`, `entity_relationships` |
| [09](09-release-content-and-flags.md) | [schema.sql](schema.sql) | `release_features`, `release_tasks`, `feature_flags`, `release_flag_settings`, `deployment_flag_settings` |
| [10](10-usage-and-cost-accounting.md) | [schema.sql](schema.sql) | `model_prices`, `usage_records`, `usage_stream_checkpoints`, `usage_stream_coverage`, `effective_usage_records`, `usage_task_totals`, `usage_agent_totals`, `usage_project_totals` |
| [11](11-model-profiles-and-agent-execution.md) | [schema.sql](schema.sql) | `provider_connections`, `model_profiles`, `model_profile_connections`, `agent_execution_configs`, `office_connection_bindings`, `office_connection_state`, `office_connection_models`, `connection_quota_state`, `connection_turn_permits`, `agent_execution_turns`, `agent_process_instances`, `agent_execution_attempts`, `model_profile_model_union` |
| [12](12-task-assignment-and-history.md) | Task semantics | Uses `tasks` from 03 and `task_events` from 05; no additional tables |
| [05: private operational data](05-operational-data.md#private-notes-attached-to-tasks) | [private-notes-schema.sql](private-notes-schema.sql) | `private_note_authors`, `private_note_tasks`, `private_task_notes`, `private_note_command_receipts` |
| [15](15-messaging-and-chat.md) | [messaging-schema.sql](messaging-schema.sql) | `messaging_organizations`, `messaging_teams`, `messaging_projects`, `messaging_employees`, `messaging_offices`, `conversations`, `conversation_memberships`, `messages`, `message_recipient_states`, `conversation_preferences`, `conversation_read_markers`, `message_attachments`, `message_reactions` |
| [03: task details](03-features-and-work.md#detailed-task-payload-and-feature-lifecycle) | [schema.sql](schema.sql) | `task_details` |
| [16](16-recovery-and-office-continuity.md) | [schema.sql](schema.sql) | `mutation_receipts`, `office_origin_streams`, `office_recovery_checkpoints`, `recovery_snapshot_transfers`, `process_cleanup_records` |
| [17](17-budgets-and-controls.md) | [budgets-schema.sql](budgets-schema.sql) | `budget_organizations`, `budget_tasks`, `budget_employees`, `budget_releases`, `budgets`, `budget_policy_versions`, `budget_periods`, `budget_events`, `budget_notices`, `budget_holds` |
| [18](18-development-processes-and-project-documentation.md) | [schema.sql](schema.sql) | `development_processes`, `development_process_roles`, `development_process_skills`, `development_process_skill_roles`, `development_process_kinds`, `development_process_folders`, `development_process_tabs`, `documentation_sections`, `documentation_media`, `document_versions`, `document_version_sources`, `document_code_links`, `task_document_versions`, `document_version_progress`, `task_releases`; adds `projects.development_process_id` and `documents.summary`, `path`, `version`, `issued_revision` to document 02's tables |

[Document 14](14-model-providers-and-connection-pools.md) presents the provider/pool subset of document 11 in the common data-structure format: seven existing tables and `model_profile_model_union`. Its field diagrams and SQL subset are checked against `schema.sql`; it adds no tables.

Document 04 owns `agent_placements`, including `placement_generation`; document 11 defines the execution relationships and fencing constraints against that same consolidated schema. No private-note body, author association, receipt or note count belongs in the shared database.

## Implementation status

This tracks the draft schema against the code, so that drift is visible. Update the row in the same change that implements or reshapes a table. States: **Implemented** (the code loads this table with these columns and actively reads/writes it through its domain port), **DDL loaded only** (created verbatim from `schema.sql` by `crates/org-office/src/store/sqlite/schema.rs`, with no domain projector or read/write port yet), **Different shape in code** (the code holds the same facts in a different table or columns), **Absent** (not loaded in the runtime schema and no code).

As of 2026-11-21 (`B1b`, `B2.1`, and `TASK-10` schema isolation landed; legacy `org.db` v3 and `office.db` v8 were deleted), `crates/org-office/src/store/sqlite/schema.rs` embeds and executes `schema.sql` verbatim on every office database open, creating all **76 shared tables and 12 views** plus 4 SQLite adapter metadata tables (`store_schema`, `store_cursor`, `store_sources`, `store_receipts`). **10 organization-family tables are Implemented** and actively projected/queried in `crates/org-office/src/store/sqlite/organization.rs`; the remaining **66 shared tables and 12 views** are **DDL loaded only** (awaiting their family ports/projectors in `B3`–`B8`), while the isolated databases (`private-notes-schema.sql` 4 tables, `messaging-schema.sql` 8 domain + 5 mirror tables, `budgets-schema.sql` 6 domain + 4 mirror tables) remain **Absent** from runtime code until their increments (`B6`, `L3`, `L8`) open their dedicated protected databases.

| Record | Kind | State | Note |
| --- | --- | --- | --- |
| `agent_execution_attempts` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `agent_execution_configs` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `agent_execution_turns` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `agent_placements` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `agent_process_instances` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `budget_events` | table | Absent | Isolated in `protected/budgets.db` (`budgets-schema.sql`); no projector yet |
| `budget_holds` | table | Absent | Isolated in `protected/budgets.db` (`budgets-schema.sql`); no projector yet |
| `budget_notices` | table | Absent | Isolated in `protected/budgets.db` (`budgets-schema.sql`); no projector yet |
| `budget_periods` | table | Absent | Isolated in `protected/budgets.db` (`budgets-schema.sql`); no projector yet |
| `budget_policy_versions` | table | Absent | Isolated in `protected/budgets.db` (`budgets-schema.sql`); no projector yet |
| `budgets` | table | Absent | Isolated in `protected/budgets.db` (`budgets-schema.sql`); no projector yet |
| `checklist_steps` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `comments` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `connection_quota_state` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `connection_turn_permits` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `conversation_memberships` | table | Absent | Isolated in `protected/messaging.db` (`messaging-schema.sql`); no projector yet |
| `conversation_preferences` | table | Absent | Isolated in `protected/messaging.db` (`messaging-schema.sql`); no projector yet |
| `conversation_read_markers` | table | Absent | Isolated in `protected/messaging.db` (`messaging-schema.sql`); no projector yet |
| `conversations` | table | Absent | Isolated in `protected/messaging.db` (`messaging-schema.sql`); no projector yet (legacy `msg_threads` deleted) |
| `deployment_flag_settings` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `deployment_repository_commits` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `deployments` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `document_code_links` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `document_version_sources` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `document_versions` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `documentation_media` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `documentation_sections` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `documents` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `employees` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `feature_flags` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `hire_requests` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `features` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `links` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `local_repository_locations` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `message_attachments` | table | Absent | Isolated in `protected/messaging.db` (`messaging-schema.sql`); no projector yet |
| `message_reactions` | table | Absent | Isolated in `protected/messaging.db` (`messaging-schema.sql`); no projector yet |
| `message_recipient_states` | table | Absent | Isolated in `protected/messaging.db` (`messaging-schema.sql`); no projector yet (legacy `msg_state`/`msg_unread` deleted) |
| `messages` | table | Absent | Isolated in `protected/messaging.db` (`messaging-schema.sql`); no projector yet (legacy `msg_headers`/`msg_bodies` deleted) |
| `milestones` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `model_prices` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `model_profile_connections` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `model_profiles` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `mutation_receipts` | table | DDL loaded only | Created by verbatim `schema.sql`; adapter receipts currently tracked in `store_receipts` |
| `office_codes` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `office_connection_bindings` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `office_connection_models` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `office_connection_state` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `office_origin_streams` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet (legacy `work_origin_*` deleted) |
| `office_recovery_checkpoints` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet (legacy `org_replica*` deleted) |
| `office_task_counters` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `offices` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `organizations` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `owner_questions` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `process_cleanup_records` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet (legacy `owned_processes` deleted) |
| `development_process_folders` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `development_process_kinds` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `development_process_roles` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `development_process_skill_roles` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `development_process_skills` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `development_process_tabs` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `development_processes` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `project_contributing_teams` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet (legacy `project_teams` deleted) |
| `project_repositories` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `projects` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet (legacy `org.db` table deleted) |
| `provider_connections` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `record_refs` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `recovery_snapshot_transfers` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet (legacy `org_snapshot_*` deleted) |
| `reference_allocations` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `reference_counters` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `release_features` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `release_flag_settings` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `release_repository_commits` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `release_tasks` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `releases` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `repositories` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `repository_caches` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `roles` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `structure_documents` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `task_details` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `task_document_versions` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `task_events` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet (legacy `work_events`/`selected_events` deleted) |
| `task_repositories` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `task_workspaces` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `tasks` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet (legacy `work_items` deleted) |
| `teams` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `tombstones` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `usage_records` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `usage_stream_checkpoints` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `waiting_spans` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `watchers` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `document_version_progress` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `effective_usage_records` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `entity_relationships` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `feature_slugs` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `milestone_progress` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `model_profile_model_union` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `release_progress` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `task_releases` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `usage_agent_totals` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `usage_project_totals` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `usage_stream_coverage` | view | DDL loaded only | Created by verbatim `schema.sql` |
| `usage_task_totals` | view | DDL loaded only | Created by verbatim `schema.sql` |

### SQLite adapter metadata tables (outside `schema.sql`)

In addition to the verbatim `schema.sql` contract tables above, `crates/org-office/src/store/sqlite/schema.rs` maintains four adapter-internal metadata tables inside the disposable SQLite projection database:

- `store_schema` — schema version and SHA-256 digest of the loaded `schema.sql` used to detect schema drift and trigger clean rebuild.
- `store_cursor` — last applied acceptance-journal ordinal and anchor hash.
- `store_sources` — tracked durable source files and digests for rebuild verification.
- `store_receipts` — adapter-level idempotency receipt projection keyed by operation ID.

These four tables are engine-private adapter state ([ADR-0018](../../adr/0018-storage-engine-independence.md)), never part of the portable domain contract or organization Git.

### Engine portability and trigger classification (ADR-0018)

To preserve storage-engine replaceability (SQLite today, PostgreSQL in future under [ADR-0018](../../adr/0018-storage-engine-independence.md)) and minimize SQLite projection write overhead:

- **Declarative portable SQL layer (kept in DDL on every engine):** `PRIMARY KEY`, `FOREIGN KEY`, `UNIQUE`, partial unique indexes, and scalar `CHECK` constraints (portable across SQLite and PostgreSQL, with SQLite `GLOB` patterns translating directly to PostgreSQL `~` regex checks).
- **Single-writer / projector layer (enforced in Rust domain/projector code, mirrored in SQLite draft triggers only as executable spec guards):** recursive graph cycle checks (`teams`, `employees`, `features`, `deployments`, `links`), append-only immutability/monotonicity guards (`task_events`, `usage_records`, `mutation_receipts`, `budgets`, `document_versions`), and cross-table project-scoping guards. Redundant `BEFORE INSERT` duplicate-key triggers (`receipt_no_replace`, `origin_stream_no_replace`, `checkpoint_no_replace`) have been removed because declarative `PRIMARY KEY` plus `BEFORE DELETE` guards (with `recursive_triggers = ON`) already enforce both `INSERT` and `INSERT OR REPLACE`. A future PostgreSQL adapter enforces these invariants in the shared Rust single-writer/projector rather than porting SQLite triggers verbatim.

## Relationships and enforcement

| Invariant | Enforcement / boundary |
| --- | --- |
| Organization scope, stable identities, task/project/team ownership | Composite keys/FKs and checks; shared source records and tombstones keep identities that past records refer to |
| Task owner versus assigned team/person | Independent references; project/task accountable team still matches; active membership and authorization are command-service checks |
| Team tree, employee reporting and task dependency cycles | SQL insertion/update guards, each with a regression case on insert (including a parent written later in the same transaction) and on update; reporting chain is separate from team hierarchy |
| Affected feature, cleanup record and placement consistency | Guards keep a task's affected feature in the task's project (on either side's change, once both rows exist), a cleanup record in its process's office, and placements and execution configs on agents |
| One-of identities | Partial unique indexes: one HQ office, one built-in HR lead, one open turn and one live main process per agent, one automatic channel per organization/team/project, one changelog per release or deployment, one budget per task/release and per agent period, one watcher entry per employee and one owner entry per task |
| Release/deployment repository history | Stable repository FK; capture validates project eligibility, while later project detachment leaves history valid |
| Flag types and used identity | Both setting-side and definition-side validation, including deferred projection order; used identity deletion/replacement guarded |
| Immutable task/usage events and historical prices | Revision/stream identity and mutation guards; corrections append new facts; recursive triggers required on every write connection |
| Usage stream checkpoints | Immutable stream identity, monotonic coverage and sealed-state checks; replacement/deletion guarded |
| Subscription uniqueness | Canonical provider/subscription key and replacement guards; external identity verification is adapter/service work |
| Private note attachment/author and receipts | Same-organization private mirror FKs, immutable attachment/author, expected revisions, deletion tombstones and durable command receipts; authenticated reads/writes remain service work |
| Execution state and recoverability | SQL references/checks plus journal files with distinct full-envelope checksums; ADR-0002 platform flush guarantees |

All write connections must set `foreign_keys=ON` and `recursive_triggers=ON`. The latter makes SQL replacement run deletion guards; bypassing either connection setting invalidates the contract. SQL columns that contain timestamps, file paths or canonical identifiers still need format/path validation at the service boundary.

## Durable data outside tables

A task event (schema 2, the only version) carries its recorded resource profile and
its complete event bytes alongside lossless change/provenance text. Codec/hash,
duplicate, header and coverage validation is admission work, not SQL. The
acceptance journal ordinal/anchor in document 16 is not an event-source stream
sequence. See ADR-0006 for its frozen mapping.

Organization Markdown/YAML, task event JSON, accounting streams/checkpoints, the office acceptance journal (16), and execution transaction envelopes/input/checkpoints/tool receipts have explicit serialization/storage contracts in documents 01, 05, 06, 10 and 11. They are not missing tables merely because their durable source is a file. Accounts, credentials and the link from an account to its person record are host data; private mirrors do not create credentials. Private notes and their command receipts are authoritative in the separate private database defined by document 05; document 13 points to that section.

## Verification and limits

`python3 -B docs/design/organization-data/test_schema.py` checks structural document/SQL equality, document 11's execution definitions in the consolidated schema, every diagram field against its owning database, this inventory's coverage, FK integrity and regression cases for the relationships above. Every trigger and every unique index in `schema.sql` has at least one case that the rule refuses, asserting that rule's own message (or the unique-constraint failure), and a case that it allows; as of 2026-11-18 removing any one of them (71 triggers, 15 unique indexes) from a copy of the schema makes the suite fail. CHECK constraints are covered by sample cases, not exhaustively, and the private schema's triggers were not part of that removal run. Private tests use in-memory SQLite to check structure; they do not implement authenticated privacy.

Required service behavior still includes actor binding/access control, active-record validation, atomic revisioned mutations and idempotency, projection validation, allocation arbitration, generated-profile membership rules, permit capacity/freshness, durable journals and authorized backup recovery. These are specified contracts rather than facts established by passing DDL tests. The application is not implementation-complete.

The ledger deliberately rounds each event half-up to integer micro-USD. Partitioning consumption across events can change the rounded total; sums report recorded estimates rather than exact provider bills. This precision policy is deliberate.
