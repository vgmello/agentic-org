# Schema coverage and integrity review

The thirteen primary design documents define two SQL artifacts. Load `schema.sql` once into the shared office projection, including model profiles and execution. Load `private-notes-schema.sql` into a separate durable private database. The shared side has 55 tables and 8 views; the private side has 4 tables. These are draft definitions, not application migrations.

## Record inventory

| Source | SQL artifact | Tables and views |
| --- | --- | --- |
| [01](01-organization-structure.md) | [schema.sql](schema.sql) | `organizations`, `repositories`, `roles`, `teams`, `offices`, `structure_documents`, `local_repository_locations`, `repository_caches` |
| [02](02-project-structure.md) | [schema.sql](schema.sql) | `projects`, `project_contributing_teams`, `project_repositories`, `releases`, `release_repository_commits`, `milestones`, `documents` |
| [03](03-features-and-work.md) | [schema.sql](schema.sql) | `features`, `tasks`, `task_repositories`, `record_refs`, `tombstones`, `feature_slugs` |
| [04](04-agent-structure.md) | [schema.sql](schema.sql) | `employees`, `agent_placements`, `task_workspaces` |
| [05](05-operational-data.md) | [schema.sql](schema.sql) | `comments`, `checklist_steps`, `watchers`, `owner_questions`, `waiting_spans`, `task_events` |
| [06](06-identity-and-numbering.md) | [schema.sql](schema.sql) | `reference_counters`, `reference_allocations` |
| [07](07-release-deployments.md) | [schema.sql](schema.sql) | `deployments`, `deployment_repository_commits` |
| [08](08-record-contracts.md) | [schema.sql](schema.sql) | `links`, `entity_relationships` |
| [09](09-release-content-and-flags.md) | [schema.sql](schema.sql) | `release_features`, `release_tasks`, `feature_flags`, `release_flag_settings`, `deployment_flag_settings` |
| [10](10-usage-and-cost-accounting.md) | [schema.sql](schema.sql) | `model_prices`, `usage_records`, `usage_stream_checkpoints`, `usage_stream_coverage`, `effective_usage_records`, `usage_task_totals`, `usage_agent_totals`, `usage_project_totals` |
| [11](11-model-profiles-and-agent-execution.md) | [schema.sql](schema.sql) | `provider_connections`, `model_profiles`, `model_profile_connections`, `agent_execution_configs`, `office_connection_bindings`, `office_connection_state`, `office_connection_models`, `connection_quota_state`, `connection_turn_permits`, `agent_execution_turns`, `agent_process_instances`, `agent_execution_attempts`, `supervisor_command_receipts`, `model_profile_model_union` |
| [12](12-task-assignment-and-history.md) | Task semantics | Uses `tasks` from 03 and `task_events` from 05; no additional tables |
| [05: private operational data](05-operational-data.md#private-notes-attached-to-tasks) | [private-notes-schema.sql](private-notes-schema.sql) | `private_note_authors`, `private_note_tasks`, `private_task_notes`, `private_note_command_receipts` |

[Document 14](14-model-providers-and-connection-pools.md) presents the provider/pool subset of document 11 in the common data-structure format: seven existing tables and `model_profile_model_union`. Its field diagrams and SQL subset are checked against `schema.sql`; it adds no tables.

Document 04 owns `agent_placements`, including `placement_generation`; document 11 defines the execution relationships and fencing constraints against that same consolidated schema. No private-note body, author association, receipt or note count belongs in the shared database.

## Relationships and enforcement

| Invariant | Enforcement / boundary |
| --- | --- |
| Organization scope, stable identities, task/project/team ownership | Composite keys/FKs and checks; shared source records and tombstones retain historical identity |
| Task owner versus assigned team/person | Independent references; project/task accountable team still matches; active membership and authorization are command-service checks |
| Team tree, employee reporting and task dependency cycles | SQL insertion/update guards; reporting chain is separate from team hierarchy |
| Release/deployment repository history | Stable repository FK; capture validates project eligibility, while later project detachment leaves history valid |
| Flag types and used identity | Both setting-side and definition-side validation, including deferred import order; used identity deletion/replacement guarded |
| Immutable task/usage events and historical prices | Revision/stream identity and mutation guards; corrections append new facts; recursive triggers required on every write connection |
| Usage stream checkpoints | Immutable stream identity, monotonic coverage and sealed-state checks; replacement/deletion guarded |
| Subscription uniqueness | Canonical provider/subscription key and replacement guards; external identity verification is adapter/service work |
| Private note attachment/author and receipts | Same-organization private mirror FKs, immutable attachment/author, expected revisions, deletion tombstones and durable command receipts; authenticated reads/writes remain service work |
| Execution state and recoverability | SQL references/checks plus journal files with distinct full-envelope checksums; ADR-0002 platform flush guarantees |

All write connections must set `foreign_keys=ON` and `recursive_triggers=ON`. The latter makes SQL replacement run deletion guards; bypassing either connection setting invalidates the contract. SQL columns that contain timestamps, file paths or canonical identifiers still need format/path validation at the service/import boundary.

## Durable data outside tables

Organization Markdown/YAML, task event JSON, accounting streams/checkpoints, authority journals, and execution transaction envelopes/input/checkpoints/tool receipts have explicit serialization/storage contracts in documents 01, 05, 06, 10 and 11. They are not missing tables merely because their durable source is a file. Credentials and authenticated-subject identities remain in the existing authentication subsystem; private mirrors do not create credentials. Private notes and their command receipts are authoritative in the separate encrypted database defined by document 05. Document 13 is a compatibility link to that canonical operational-data section.

## Verification and limits

`python3 -B docs/design/organization-data/test_schema.py` checks structural document/SQL equality, document 11's execution definitions in the consolidated schema, every diagram field against its owning database, this inventory's coverage, FK integrity and regression cases for the relationships above. Private tests use in-memory SQLite to check structure; they do not implement SQLCipher or authenticated privacy.

Required service behavior still includes actor binding/access control, active-record validation, atomic revisioned mutations and idempotency, historical import validation, allocation arbitration, generated-profile membership rules, permit capacity/freshness, durable journals and authorized backup recovery. These are specified contracts rather than facts established by passing DDL tests. The application is not implementation-complete.

The ledger deliberately rounds each event half-up to integer micro-USD. Partitioning consumption across events can change the rounded total; sums report recorded estimates rather than exact provider bills. This precision policy is explicit and was not silently changed by the integrity fixes.
