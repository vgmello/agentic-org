# Organization data: status, changelog and host-owned records

Companion to the [organization-data index](README.md), split out to keep the index short. It has the same authority as the index.

## Outside this set: what the host owns

This set describes an organization's data. Some records it refers to belong to the host and are defined in the [standalone design](../standalone-server.md) and the host's own database, not here:

| Host-owned record | What this set uses from it |
| --- | --- |
| Instance | Its immutable ID, as `instance_id` in assignment fingerprints. An instance can host several organizations. |
| Account | A signed-in person. Accounts are created only through one-time registration links. `agentic-org setup` prints the first one, and the account registered through it is the server's administrator: exactly one account holds that role at any time, and it can hand the role to another account in the browser. Further links are issued in the browser, from People, by the administrator or the organization's owner, each for an existing person record. A link works once, expires and is stored only as a digest; a forgotten password is reset through a link issued the same way (for the administrator, by `agentic-org admin reset` on the server machine), and command-line sign-in is approved in the browser through a one-time link. Only the administrator creates organizations. |
| Account to person link | In each organization an account can open, the account is tied to exactly one person record (an employee of kind `person`); never to an agent. The link is host data and grants no authority by itself ([document 04](04-agent-structure.md#accounts-people-and-the-organization-owner)). |
| Organization ownership | The account that created the organization owns it until that owner hands ownership to another person who has an account; an organization has exactly one owner. Ownership is a permission of the owning account, not an employee field. The owner acts in the organization as their own person record; `owner` in a record that addresses someone means that person at the time it is read. |
| Session | A signed-in browser or CLI session ([ADR-0013](../../adr/0013-stored-browser-sessions.md)). Never referenced by organization data. |
| Office secrets key and writer custody | Held by the host per office ([ADR-0016](../../adr/0016-no-application-level-data-encryption.md), [document 16](16-recovery-and-office-continuity.md)). Never stored under the organization data root. |

Nothing in an employee record grants sign-in, ownership or administration.

## Status and changelog

Every numbered document carries one status line. The vocabulary is: **draft** (still being shaped), **selected** (the contract to build to), **implemented** (the code matches). All numbered documents are selected. What the code actually holds is tracked per table in [schema coverage](SCHEMA-COVERAGE.md#implementation-status). A document's own revision number changes when its contract changes.

| Date | Set revision | Change |
| --- | --- | --- |
| 2026-11-18 | 2.19 | The owner's REQ-06 and REQ-07 and the decisions that followed. Document 18 added: development processes (a primitive separate from organization processes) with their roles, skills, document kinds, folders and tabs; a project's choice of one; the project documentation tree and its index; issued document versions, code links, the tasks that realize a version and the `task_releases` view. `projects` gains `development_process_id`; `documents` gains `summary`, `path`, `version` and `issued_revision` and an extensible `kind` for tree documents; project-wide decisions, designs and reports move into the tree. `employees` gains `may_hire` and a hire request is a record, `hire_requests` ([ADR-0027](../../adr/0027-a-lead-approves-a-hire.md)). A turn records what started it. |
| 2026-11-18 | 2.16 | The office store ([ADR-0021](../../adr/0021-office-store-architecture.md)): one acceptance journal per office for every family, entries of file changes in segments hashed as stored, host checkpoint per flush (16 rev 4); a message is acknowledged at its journal entry and its file follows (15 rev 10); no separate numbering journal (06 rev 2.10); journal entries use `raw-bytes/1` (08 rev 2.10); the storage tree shows where the journal is. |
| 2026-11-18 | 2.17 | What reaches an agent's inbox ([ADR-0024](../../adr/0024-a-channel-question-is-watched-for-its-answer.md)): `message_recipient_states` gains `delivered_because` and `related_message_ids_json`; a message's metadata carries `mentions[]`, read from `@handle` in its text; `here` is not allowed as a handle. |
| 2026-11-18 | 2.18 | An ask carries one to eight questions (`questions[]`), answered together; an answer holds one entry for each question. Owner request. |
| 2026-11-17 | 2.15 | Every actor is a person or an agent, never `owner`; accounts are tied to persons; creating an organization makes its top-level team, the creator's person record and the HR lead; a project creates its team; agents are onboarded only through HR; areas are removed; older material removed. |

The SQL is a structural draft. Implementing the schema, task mutation/replication, private task-note storage/access controls, memory consolidation and analytics retention is separate work. Run `python3 docs/design/organization-data/test_schema.py` for executable draft-schema checks.

Recovery sources are defined in [document 16](16-recovery-and-office-continuity.md); office caches are disposable only after durable sources and safe process reconciliation are accounted for.
