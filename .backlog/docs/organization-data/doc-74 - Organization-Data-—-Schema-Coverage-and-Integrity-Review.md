---
id: doc-74
title: Organization Data — Schema Coverage and Integrity Review
type: specification
created_date: '2026-11-21T01:30:00Z'
updated_date: '2026-11-22T01:24:00Z'
tags:
  - organization-data
  - contract
  - schema
---
<!-- Source migrated from docs/design/organization-data/SCHEMA-COVERAGE.md (set revision 2.24, reviewed 2026-11-21) -->

# Schema coverage and integrity review

The primary design documents define four SQL artifacts so sensitive operational projections are isolated from shared plaintext `office.db` ([ADR-0016](../../decisions/decision-16%20-%20ADR-0016-No-application-level-encryption-of-organization-data.md), `TASK-10`):

- Load [`schema.sql`](../../../docs/design/organization-data/schema.sql) once into the shared office projection (`runtime/<office-id>/office.db`): **79 tables and 12 views**.
- Load [`private-notes-schema.sql`](../../../docs/design/organization-data/private-notes-schema.sql) into the separate durable private-notes database (`runtime/<office-id>/private/task-notes.db`): **4 tables**.
- Load [`messaging-schema.sql`](../../../docs/design/organization-data/messaging-schema.sql) into the separate access-controlled messaging database (`runtime/<office-id>/protected/messaging.db`): **8 messaging tables + 5 identity mirror tables** (`messaging_organizations`, `messaging_teams`, `messaging_projects`, `messaging_employees`, `messaging_offices`).
- Load [`budgets-schema.sql`](../../../docs/design/organization-data/budgets-schema.sql) into the separate access-controlled budgets database (`runtime/<office-id>/protected/budgets.db`): **6 budget tables + 4 identity mirror tables** (`budget_organizations`, `budget_tasks`, `budget_employees`, `budget_releases`).

These are draft definitions, not application migrations.

## Record inventory

| Source | SQL artifact | Tables and views |
| --- | --- | --- |
| [01](doc-75%20-%20Organization-Data-01-—-Organization-Structure.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `organizations`, `repositories`, `roles`, `teams`, `offices`, `structure_documents`, `local_repository_locations`, `repository_caches` |
| [02](doc-76%20-%20Organization-Data-02-—-Project-Structure.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `projects`, `project_contributing_teams`, `project_repositories`, `releases`, `release_repository_commits`, `milestones`, `documents`, `milestone_progress`, `release_progress` |
| [03](doc-77%20-%20Organization-Data-03-—-Features-and-Work.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `features`, `tasks`, `task_repositories`, `task_labels`, `task_files`, `record_refs`, `tombstones`, `feature_slugs` |
| [04](doc-78%20-%20Organization-Data-04-—-Agent-Structure.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `employees`, `agent_placements`, `task_workspaces`, `hire_requests` |
| [05](doc-79%20-%20Organization-Data-05-—-Operational-Data-and-Private-Notes.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `comments`, `checklist_steps`, `watchers`, `owner_questions`, `waiting_spans`, `task_events` |
| [06](doc-80%20-%20Organization-Data-06-—-Identity-and-Numbering.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `reference_counters`, `reference_allocations`, `office_codes`, `office_task_counters` |
| [07](doc-81%20-%20Organization-Data-07-—-Release-Deployments.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `deployments`, `deployment_repository_commits` |
| [08](doc-82%20-%20Organization-Data-08-—-Record-Contracts-and-Hash-Codec-Registry.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `links`, `entity_relationships` |
| [09](doc-83%20-%20Organization-Data-09-—-Release-Content-and-Flags.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `release_features`, `release_tasks`, `feature_flags`, `release_flag_settings`, `deployment_flag_settings` |
| [10](doc-87%20-%20Organization-Data-10-—-Usage-and-Cost-Accounting.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `model_prices`, `usage_records`, `usage_stream_checkpoints`, `usage_stream_coverage`, `effective_usage_records`, `usage_task_totals`, `usage_agent_totals`, `usage_project_totals` |
| [11](doc-88%20-%20Organization-Data-11-—-Model-Profiles-and-Agent-Execution.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `provider_connections`, `model_profiles`, `model_profile_connections`, `agent_execution_configs`, `watching_model_settings`, `office_connection_bindings`, `office_connection_state`, `office_connection_models`, `connection_quota_state`, `connection_turn_permits`, `agent_execution_turns`, `agent_process_instances`, `agent_execution_attempts`, `model_profile_model_union` |
| [12](doc-84%20-%20Organization-Data-12-—-Task-Assignment-and-History.md) | Task semantics | Uses `tasks` from 03 and `task_events` from 05; no additional tables |
| [05: private operational data](doc-79%20-%20Organization-Data-05-—-Operational-Data-and-Private-Notes.md#private-notes-attached-to-tasks) | [private-notes-schema.sql](../../../docs/design/organization-data/private-notes-schema.sql) | `private_note_authors`, `private_note_tasks`, `private_task_notes`, `private_note_command_receipts` |
| [15](doc-90%20-%20Organization-Data-15-—-Messaging-and-Chat.md) | [messaging-schema.sql](../../../docs/design/organization-data/messaging-schema.sql) | `messaging_organizations`, `messaging_teams`, `messaging_projects`, `messaging_employees`, `messaging_offices`, `conversations`, `conversation_memberships`, `messages`, `message_recipient_states`, `conversation_preferences`, `conversation_read_markers`, `message_attachments`, `message_reactions` |
| [03: task details](doc-77%20-%20Organization-Data-03-—-Features-and-Work.md#detailed-task-payload-and-feature-lifecycle) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `task_details` |
| [16](doc-91%20-%20Organization-Data-16-—-Recovery-and-Office-Continuity.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `mutation_receipts`, `office_origin_streams`, `office_recovery_checkpoints`, `recovery_snapshot_transfers`, `process_cleanup_records` |
| [17](doc-92%20-%20Organization-Data-17-—-Budgets-and-Controls.md) | [budgets-schema.sql](../../../docs/design/organization-data/budgets-schema.sql) | `budget_organizations`, `budget_tasks`, `budget_employees`, `budget_releases`, `budgets`, `budget_policy_versions`, `budget_periods`, `budget_events`, `budget_notices`, `budget_holds` |
| [18](doc-86%20-%20Organization-Data-18-—-Development-Processes-and-Project-Documentation.md) | [schema.sql](../../../docs/design/organization-data/schema.sql) | `development_processes`, `development_process_roles`, `development_process_skills`, `development_process_skill_roles`, `development_process_kinds`, `development_process_folders`, `development_process_tabs`, `documentation_sections`, `documentation_media`, `document_versions`, `document_version_sources`, `document_code_links`, `task_document_versions`, `document_version_progress`, `task_releases`; adds `projects.development_process_id` and `documents.summary`, `path`, `version`, `issued_revision` to document 02's tables |

[Document 14](doc-89%20-%20Organization-Data-14-—-Model-Providers-and-Connection-Pools.md) presents the provider/pool subset of document 11 in the common data-structure format: seven existing tables and `model_profile_model_union`. Its field diagrams and SQL subset are checked against `schema.sql`; it adds no tables.

Document 04 owns `agent_placements`, including `placement_generation`; document 11 defines the execution relationships and fencing constraints against that same consolidated schema. No private-note body, author association, receipt or note count belongs in the shared database.

## Implementation status

This tracks the draft schema against the code, so that drift is visible. Update the row in the same change that implements or reshapes a table. States: **Implemented** (the code loads this table with these columns and actively reads/writes it through its domain port), **DDL loaded only** (created verbatim from `schema.sql` by `crates/org-office/src/store/sqlite/schema.rs`, with no domain projector or read/write port yet), **Different shape in code** (the code holds the same facts in a different table or columns), **Absent** (not loaded in the runtime schema and no code).

As of 2026-11-21 (`B1b`, `B2.1`, and `TASK-10` schema isolation landed; legacy `org.db` v3 and `office.db` v8 were deleted), `crates/org-office/src/store/sqlite/schema.rs` embeds and executes `schema.sql` verbatim on every office database open, creating all **79 shared tables and 12 views** plus 4 SQLite adapter metadata tables (`store_schema`, `store_cursor`, `store_sources`, `store_receipts`). **10 organization-family tables are Implemented** and actively projected/queried in `crates/org-office/src/store/sqlite/organization.rs`; the remaining **66 shared tables and 12 views** are **DDL loaded only** (awaiting their family ports/projectors in `B3`–`B8`), while the isolated databases (`private-notes-schema.sql` 4 tables, `messaging-schema.sql` 8 domain + 5 mirror tables, `budgets-schema.sql` 6 domain + 4 mirror tables) remain **Absent** from runtime code until their increments (`B6`, `L3`, `L8`) open their dedicated protected databases.

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
| `checklist_steps` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` |
| `comments` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` |
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
| `links` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` (`TASK-77` depends_on DAG + links) |
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
| `office_task_counters` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` (local `T-<CODE><N>` minting) |
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
| `task_details` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` |
| `task_document_versions` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `task_events` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` (verbatim `schema.sql`) |
| `task_files` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` (`TASK-77` reference & modified files) |
| `task_labels` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` (`TASK-77` searchable labels) |
| `task_repositories` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `task_workspaces` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `tasks` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` (`TASK-49` + `TASK-77` rank_order) |
| `teams` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`) |
| `tombstones` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `usage_records` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `usage_stream_checkpoints` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `waiting_spans` | table | DDL loaded only | Created by verbatim `schema.sql`; no projector yet |
| `watchers` | table | Implemented | Active in `crates/org-office/src/store/sqlite/tasks.rs` |
| `watching_model_settings` | table | Implemented | Active in `crates/org-office/src/store/sqlite/organization.rs` (verbatim `schema.sql`, `ADR-0024` d13) |
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

These four tables are engine-private adapter state ([ADR-0018](../../decisions/decision-18%20-%20ADR-0018-The-storage-engine-is-replaceable-PostgreSQL-must-be-possible.md)), never part of the portable domain contract or organization Git.

### Engine portability and trigger classification (ADR-0018)

To preserve storage-engine replaceability (SQLite today, PostgreSQL in future under [ADR-0018](../../decisions/decision-18%20-%20ADR-0018-The-storage-engine-is-replaceable-PostgreSQL-must-be-possible.md)) and minimize SQLite projection write overhead:

- **Declarative portable SQL layer (kept in DDL on every engine):** `PRIMARY KEY`, `FOREIGN KEY`, `UNIQUE`, partial unique indexes, and scalar `CHECK` constraints (portable across SQLite and PostgreSQL, with SQLite `GLOB` patterns translating directly to PostgreSQL `~` regex checks).
- **Single-writer / projector layer (enforced in Rust domain/projector code, mirrored in SQLite draft triggers only as executable spec guards):** recursive graph cycle checks (`teams`, `employees`, `features`, `deployments`, `links`), append-only immutability/monotonicity guards (`task_events`, `usage_records`, `mutation_receipts`, `budgets`, `document_versions`), and cross-table project-scoping guards. Redundant `BEFORE INSERT` duplicate-key triggers (`receipt_no_replace`, `origin_stream_no_replace`, `checkpoint_no_replace`) have been removed because declarative `PRIMARY KEY` plus `BEFORE DELETE` guards (with `recursive_triggers = ON`) already enforce both `INSERT` and `INSERT OR REPLACE`. A future PostgreSQL adapter enforces these invariants in the shared Rust single-writer/projector rather than porting SQLite triggers verbatim.

### Generated columns and trigger inventory (ADR-0018 ENG-5)

Every SQLite-specific generated column and trigger across the contract schemas is enumerated below so another storage engine (such as PostgreSQL) can enforce the exact same invariants in its own dialect or in the shared Rust projector/single-writer layer (`test_schema.py::test_eng5_schema_coverage_lists_all_triggers_and_generated_columns`).

| Generated column | Schema file | Portable expression |
| --- | --- | --- |
| `teams.key` | `schema.sql` | `CASE WHEN number IS NOT NULL THEN 'TEAM-' || number END` |
| `projects.key` | `schema.sql` | `CASE WHEN number IS NOT NULL THEN 'P-' || number END` |
| `releases.key` | `schema.sql` | `CASE WHEN number IS NOT NULL THEN 'R-' || number END` |
| `milestones.key` | `schema.sql` | `CASE WHEN number IS NOT NULL THEN 'M-' || number END` |
| `features.key` | `schema.sql` | `CASE WHEN number IS NOT NULL THEN 'F-' || number END` |
| `tasks.key` | `schema.sql` | `'T-' || office_code || number` |

- **`budgets-schema.sql` (16 triggers):** `budget_tasks_no_undelete`, `budget_target_immutable`, `budget_agent_insert`, `employee_budget_kind_update`, `employee_budget_kind_insert`, `budgets_no_delete`, `budget_policy_versions_no_update`, `budget_policy_versions_no_delete`, `budget_periods_no_delete`, `budget_events_no_update`, `budget_events_no_delete`, `budget_notices_no_delete`, `budget_holds_no_delete`, `budget_period_identity_update`, `budget_release_no_hold`, `budget_release_no_hold_update`
- **`private-notes-schema.sql` (10 triggers):** `private_note_tasks_no_undelete`, `private_notes_no_replace`, `private_notes_initial_state`, `private_notes_update_contract`, `private_notes_keep_tombstone`, `private_receipts_no_replace`, `private_receipt_matches_author`, `private_receipts_no_delete`, `private_receipts_finish_once`, `private_note_pending_publication`
- **`schema.sql` (86 triggers):** `task_events_no_replace`, `task_events_no_update`, `task_events_no_delete`, `organization_repository_must_be_remote_insert`, `organization_repository_must_be_remote_update`, `organization_remote_reference_insert`, `organization_remote_reference_update`, `teams_no_cycle_insert`, `teams_no_cycle_update`, `links_no_dependency_cycle_insert`, `links_no_dependency_cycle_update`, `release_flag_settings_value_type_insert`, `release_flag_settings_value_type_update`, `deployment_flag_settings_value_type_insert`, `deployment_flag_settings_value_type_update`, `used_flag_identity_immutable`, `usage_records_immutable_update`, `usage_records_immutable_delete`, `used_model_price_immutable_update`, `usage_actor_must_be_agent`, `usage_correction_parent_exists`, `attribution_correction_preserves_facts`, `usage_checkpoint_monotonic`, `usage_checkpoint_no_delete`, `used_model_price_no_delete`, `used_flag_no_delete`, `inserted_flag_checks_existing_settings`, `employees_no_reporting_cycle_insert`, `employees_no_reporting_cycle_update`, `employee_usage_kind_insert`, `employee_usage_kind_update`, `registered_subscription_cannot_be_replaced`, `registered_subscription_cannot_be_displaced`, `registered_subscription_identity_is_immutable`, `placement_generation_must_advance`, `agent_config_requires_agent`, `agent_config_update_requires_agent`, `employee_kind_never_changes`, `receipt_identity_immutable`, `origin_stream_no_regression`, `checkpoint_no_regression`, `receipt_no_delete`, `origin_stream_no_delete`, `checkpoint_no_delete`, `document_versions_no_replace`, `document_versions_are_written_once`, `document_versions_are_kept`, `document_version_sources_are_written_once`, `document_version_sources_are_kept`, `task_affects_feature_same_project_insert`, `task_affects_feature_same_project_update`, `task_insert_keeps_affected_feature`, `feature_insert_keeps_affected_task`, `feature_project_keeps_affected_task`, `cleanup_record_same_office_insert`, `cleanup_record_same_office_update`, `placement_requires_agent_insert`, `placement_requires_agent_update`, `task_project_keeps_affected_feature`, `document_versions_match_document_project`, `document_project_keeps_versions`, `document_version_sources_same_project`, `task_document_versions_same_project_insert`, `task_document_versions_same_project_update`, `task_insert_keeps_document_versions`, `task_project_keeps_document_versions`, `release_tasks_same_project_insert`, `release_tasks_same_project_update`, `task_insert_keeps_release_tasks`, `task_project_keeps_release_tasks`, `comments_reject_deleted_task`, `checklist_steps_reject_deleted_task`, `watchers_reject_deleted_task`, `owner_questions_reject_deleted_task`, `waiting_spans_reject_deleted_task`, `feature_replacement_no_cycle_insert`, `feature_replacement_no_cycle_update`, `deployment_rollback_no_cycle_insert`, `deployment_rollback_no_cycle_update`, `hire_request_employee_is_agent_insert`, `hire_request_employee_is_agent_update`, `hire_request_terminal_immutable`, `office_codes_immutable`, `office_codes_no_delete`, `reference_counters_monotonic`, `office_task_counters_monotonic`

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
