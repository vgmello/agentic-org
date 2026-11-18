# Schema coverage and integrity review

The primary design documents define two SQL artifacts. Load `schema.sql` once into the shared office projection, including model profiles and execution. Load `private-notes-schema.sql` into a separate durable private database. The shared side has 90 tables and 11 views; the private side has 4 tables. These are draft definitions, not application migrations.

## Record inventory

| Source | SQL artifact | Tables and views |
| --- | --- | --- |
| [01](01-organization-structure.md) | [schema.sql](schema.sql) | `organizations`, `repositories`, `roles`, `teams`, `offices`, `structure_documents`, `local_repository_locations`, `repository_caches` |
| [02](02-project-structure.md) | [schema.sql](schema.sql) | `projects`, `project_contributing_teams`, `project_repositories`, `releases`, `release_repository_commits`, `milestones`, `documents`, `milestone_progress`, `release_progress` |
| [03](03-features-and-work.md) | [schema.sql](schema.sql) | `features`, `tasks`, `task_repositories`, `record_refs`, `tombstones`, `feature_slugs` |
| [04](04-agent-structure.md) | [schema.sql](schema.sql) | `employees`, `agent_placements`, `task_workspaces` |
| [05](05-operational-data.md) | [schema.sql](schema.sql) | `comments`, `checklist_steps`, `watchers`, `owner_questions`, `waiting_spans`, `task_events` |
| [06](06-identity-and-numbering.md) | [schema.sql](schema.sql) | `reference_counters`, `reference_allocations`, `office_codes`, `office_task_counters` |
| [07](07-release-deployments.md) | [schema.sql](schema.sql) | `deployments`, `deployment_repository_commits` |
| [08](08-record-contracts.md) | [schema.sql](schema.sql) | `links`, `entity_relationships` |
| [09](09-release-content-and-flags.md) | [schema.sql](schema.sql) | `release_features`, `release_tasks`, `feature_flags`, `release_flag_settings`, `deployment_flag_settings` |
| [10](10-usage-and-cost-accounting.md) | [schema.sql](schema.sql) | `model_prices`, `usage_records`, `usage_stream_checkpoints`, `usage_stream_coverage`, `effective_usage_records`, `usage_task_totals`, `usage_agent_totals`, `usage_project_totals` |
| [11](11-model-profiles-and-agent-execution.md) | [schema.sql](schema.sql) | `provider_connections`, `model_profiles`, `model_profile_connections`, `agent_execution_configs`, `office_connection_bindings`, `office_connection_state`, `office_connection_models`, `connection_quota_state`, `connection_turn_permits`, `agent_execution_turns`, `agent_process_instances`, `agent_execution_attempts`, `supervisor_command_receipts`, `model_profile_model_union` |
| [12](12-task-assignment-and-history.md) | Task semantics | Uses `tasks` from 03 and `task_events` from 05; no additional tables |
| [05: private operational data](05-operational-data.md#private-notes-attached-to-tasks) | [private-notes-schema.sql](private-notes-schema.sql) | `private_note_authors`, `private_note_tasks`, `private_task_notes`, `private_note_command_receipts` |
| [15](15-messaging-and-chat.md) | [schema.sql](schema.sql) | `conversations`, `conversation_memberships`, `messages`, `message_recipient_states`, `conversation_preferences`, `conversation_read_markers`, `message_attachments`, `message_reactions` |
| [03: task details](03-features-and-work.md#detailed-task-payload-and-feature-lifecycle) | [schema.sql](schema.sql) | `task_details` |
| [16](16-recovery-and-office-continuity.md) | [schema.sql](schema.sql) | `mutation_receipts`, `office_origin_streams`, `office_recovery_checkpoints`, `recovery_snapshot_transfers`, `process_cleanup_records` |
| [17](17-budgets-and-controls.md) | [schema.sql](schema.sql) | `budgets`, `budget_policy_versions`, `budget_periods`, `budget_events`, `budget_notices`, `budget_holds` |
| [18](18-development-processes-and-project-documentation.md) | [schema.sql](schema.sql) | `development_processes`, `development_process_roles`, `development_process_skills`, `development_process_skill_roles`, `development_process_kinds`, `development_process_folders`, `development_process_tabs`, `documentation_sections`, `documentation_media`, `document_versions`, `document_version_sources`, `document_code_links`, `task_document_versions`, `document_version_progress`; adds `projects.development_process_id` and `documents.summary`, `path`, `version`, `issued_revision` to document 02's tables |

[Document 14](14-model-providers-and-connection-pools.md) presents the provider/pool subset of document 11 in the common data-structure format: seven existing tables and `model_profile_model_union`. Its field diagrams and SQL subset are checked against `schema.sql`; it adds no tables.

Document 04 owns `agent_placements`, including `placement_generation`; document 11 defines the execution relationships and fencing constraints against that same consolidated schema. No private-note body, author association, receipt or note count belongs in the shared database.

## Implementation status

This tracks the draft schema against the code, so that drift is visible. Update the row in the same change that implements or reshapes a table. States: **Implemented** (the code has this table with these columns), **Different shape in code** (the code holds the same facts in a different table or columns), **Absent** (no code).

As of 2026-11-18 none of the 90 tables or 11 views is implemented as drafted. 16 have a counterpart of a different shape in the code and 85 are absent. The comparison is by table name and by reading the current in-code schemas (`org.db` version 3, `office.db` version 8); it was not run.

| Record | Kind | State | Note |
| --- | --- | --- | --- |
| `agent_execution_attempts` | table | Absent |  |
| `agent_execution_configs` | table | Absent |  |
| `agent_execution_turns` | table | Absent |  |
| `agent_placements` | table | Absent |  |
| `agent_process_instances` | table | Absent |  |
| `budget_events` | table | Absent |  |
| `budget_holds` | table | Absent |  |
| `budget_notices` | table | Absent |  |
| `budget_periods` | table | Absent |  |
| `budget_policy_versions` | table | Absent |  |
| `budgets` | table | Absent |  |
| `checklist_steps` | table | Absent |  |
| `comments` | table | Absent |  |
| `connection_quota_state` | table | Absent |  |
| `connection_turn_permits` | table | Absent |  |
| `conversation_memberships` | table | Absent |  |
| `conversation_preferences` | table | Absent |  |
| `conversation_read_markers` | table | Absent |  |
| `conversations` | table | Different shape in code | Closest current table: `msg_threads` |
| `deployment_flag_settings` | table | Absent |  |
| `deployment_repository_commits` | table | Absent |  |
| `deployments` | table | Absent |  |
| `document_code_links` | table | Absent |  |
| `document_version_sources` | table | Absent |  |
| `document_versions` | table | Absent |  |
| `documentation_media` | table | Absent |  |
| `documentation_sections` | table | Absent |  |
| `documents` | table | Absent |  |
| `employees` | table | Different shape in code | Closest current table: `members`, `memberships` |
| `feature_flags` | table | Absent |  |
| `features` | table | Absent |  |
| `links` | table | Absent |  |
| `local_repository_locations` | table | Absent |  |
| `message_attachments` | table | Absent |  |
| `message_reactions` | table | Absent |  |
| `message_recipient_states` | table | Different shape in code | Closest current table: `msg_state`, `msg_unread` |
| `messages` | table | Different shape in code | Closest current table: `msg_headers`, `msg_bodies` |
| `milestones` | table | Absent |  |
| `model_prices` | table | Absent |  |
| `model_profile_connections` | table | Absent |  |
| `model_profiles` | table | Absent |  |
| `mutation_receipts` | table | Different shape in code | Closest current table: `commands`, `work_change_results`, `selected_receipts` |
| `office_codes` | table | Absent |  |
| `office_connection_bindings` | table | Absent |  |
| `office_connection_models` | table | Absent |  |
| `office_connection_state` | table | Absent |  |
| `office_origin_streams` | table | Different shape in code | Closest current table: `work_origin_counter`, `work_origin_log` |
| `office_recovery_checkpoints` | table | Different shape in code | Closest current table: `org_replica`, `org_replica_base` |
| `office_task_counters` | table | Absent |  |
| `offices` | table | Absent |  |
| `organizations` | table | Different shape in code | Same name in `org.db`, different columns |
| `owner_questions` | table | Absent |  |
| `process_cleanup_records` | table | Different shape in code | Closest current table: `owned_processes` |
| `development_process_folders` | table | Absent |  |
| `development_process_kinds` | table | Absent |  |
| `development_process_roles` | table | Absent |  |
| `development_process_skill_roles` | table | Absent |  |
| `development_process_skills` | table | Absent |  |
| `development_process_tabs` | table | Absent |  |
| `development_processes` | table | Absent |  |
| `project_contributing_teams` | table | Different shape in code | Closest current table: `project_teams` |
| `project_repositories` | table | Absent |  |
| `projects` | table | Different shape in code | Same name in `org.db`, different columns |
| `provider_connections` | table | Absent |  |
| `record_refs` | table | Absent |  |
| `recovery_snapshot_transfers` | table | Different shape in code | Closest current table: `org_snapshot_stage`, `org_snapshot_install` |
| `reference_allocations` | table | Absent |  |
| `reference_counters` | table | Different shape in code | Closest current table: `work_number_counters` |
| `release_features` | table | Absent |  |
| `release_flag_settings` | table | Absent |  |
| `release_repository_commits` | table | Absent |  |
| `release_tasks` | table | Absent |  |
| `releases` | table | Absent |  |
| `repositories` | table | Absent |  |
| `repository_caches` | table | Absent |  |
| `roles` | table | Absent |  |
| `structure_documents` | table | Absent |  |
| `supervisor_command_receipts` | table | Absent |  |
| `task_details` | table | Absent |  |
| `task_document_versions` | table | Absent |  |
| `task_events` | table | Different shape in code | Closest current table: `work_events`, `selected_events` |
| `task_repositories` | table | Absent |  |
| `task_workspaces` | table | Absent |  |
| `tasks` | table | Different shape in code | Closest current table: `work_items` |
| `teams` | table | Different shape in code | Same name in `org.db`, different columns |
| `tombstones` | table | Absent |  |
| `usage_records` | table | Absent |  |
| `usage_stream_checkpoints` | table | Absent |  |
| `waiting_spans` | table | Absent |  |
| `watchers` | table | Absent |  |
| `document_version_progress` | view | Absent |  |
| `effective_usage_records` | view | Absent |  |
| `entity_relationships` | view | Absent |  |
| `feature_slugs` | view | Absent |  |
| `milestone_progress` | view | Absent |  |
| `model_profile_model_union` | view | Absent |  |
| `release_progress` | view | Absent |  |
| `usage_agent_totals` | view | Absent |  |
| `usage_project_totals` | view | Absent |  |
| `usage_stream_coverage` | view | Absent |  |
| `usage_task_totals` | view | Absent |  |

## Relationships and enforcement

| Invariant | Enforcement / boundary |
| --- | --- |
| Organization scope, stable identities, task/project/team ownership | Composite keys/FKs and checks; shared source records and tombstones keep identities that past records refer to |
| Task owner versus assigned team/person | Independent references; project/task accountable team still matches; active membership and authorization are command-service checks |
| Team tree, employee reporting and task dependency cycles | SQL insertion/update guards; reporting chain is separate from team hierarchy |
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
work-acceptance journal ordinal/anchor in document 16 is not an event-source stream
sequence. See ADR-0006 for its frozen mapping.

Organization Markdown/YAML, task event JSON, accounting streams/checkpoints, authority journals, and execution transaction envelopes/input/checkpoints/tool receipts have explicit serialization/storage contracts in documents 01, 05, 06, 10 and 11. They are not missing tables merely because their durable source is a file. Accounts, credentials and the link from an account to its person record are host data; private mirrors do not create credentials. Private notes and their command receipts are authoritative in the separate private database defined by document 05; document 13 points to that section.

## Verification and limits

`python3 -B docs/design/organization-data/test_schema.py` checks structural document/SQL equality, document 11's execution definitions in the consolidated schema, every diagram field against its owning database, this inventory's coverage, FK integrity and regression cases for the relationships above. Private tests use in-memory SQLite to check structure; they do not implement authenticated privacy.

Required service behavior still includes actor binding/access control, active-record validation, atomic revisioned mutations and idempotency, projection validation, allocation arbitration, generated-profile membership rules, permit capacity/freshness, durable journals and authorized backup recovery. These are specified contracts rather than facts established by passing DDL tests. The application is not implementation-complete.

The ledger deliberately rounds each event half-up to integer micro-USD. Partitioning consumption across events can change the rounded total; sums report recorded estimates rather than exact provider bills. This precision policy is deliberate.
