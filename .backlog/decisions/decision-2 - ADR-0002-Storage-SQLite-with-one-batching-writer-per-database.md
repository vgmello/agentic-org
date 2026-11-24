---
id: decision-2
title: 'ADR-0002: Storage: SQLite with one batching writer per database'
date: '2026-11-21T00:43:00Z'
status: accepted
---

> **Review note (2026-11-21):** Decision 1's original SQLCipher requirement is superseded by ADR-0016 (`decision-16`); journal/projection split is governed by ADR-0021 (`decision-21`), storage-engine independence by ADR-0018 (`decision-18`), and private-note access rules were reconciled with ADR-0031 (`decision-31`) in `TASK-19`. Open v1 remediation: `org-host` ADR-0018 compliance & `org-office` SQLite re-export restriction (`TASK-13`), SQLite trigger overhead (`TASK-18`), missing hot-path/ADR-0031 indexes & SQL integrity holes (`TASK-6`, `TASK-7`, `TASK-8`), isolating private DMs/owner budgets from plaintext `office.db` (`TASK-10`), and isolated private task notes (`TASK-50`).

# ADR-0002 · Storage: SQLite with one batching writer per database

Status: accepted; decision 1's SQLCipher requirement is superseded by
[ADR-0016](decision-16%20-%20ADR-0016-No-application-level-encryption-of-organization-data.md) (the workspace no longer builds
SQLCipher). Refines [ADR-0001](decision-1%20-%20ADR-0001-The-app-is-a-client-offices-serve-organization-data.md); the office
acceptance journal and projection split are governed by
[ADR-0021](decision-21%20-%20ADR-0021-The-office-store-one-journal-of-file-changes-a-projection-rebuilt-from-files.md).
Where data lives on disk is owned by the
[organization-data storage contract](../docs/organization-data/doc-72%20-%20Organization-Data-—-README.md#shared-storage-contract):
one root, with sources under `durable/` and the database projection under
`runtime/<office-id>/`. The database engine is replaceable behind store adapters
([ADR-0018](decision-18%20-%20ADR-0018-The-storage-engine-is-replaceable-PostgreSQL-must-be-possible.md)). Organization data is not
encrypted by the application ([ADR-0016](decision-16%20-%20ADR-0016-No-application-level-encryption-of-organization-data.md)).

## Context

- Many agents talk to an office at once, and their writes must not fail or stall
  each other.
- An office must acknowledge a write only when its declared durability holds.
- Turso, a Rust rewrite of SQLite with concurrent writes, was considered. Its
  concurrency is experimental and its Rust API is async-only and pre-1.0, so it is
  not used.

## Decisions

1. **Engine: SQLite through `rusqlite`.** Only the authorized storage service opens
   its database; browser and agent clients use its domain operations (ADR-0001).
   The separate private-note store enforces its own reading rule (the author and
   people, never another agent, [ADR-0031](decision-31%20-%20ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md)) rather
   than reusing broad shared-task reads.
2. **Concurrency: one writer per database.** The server keeps one write connection
   on one thread and sends every write to it through a channel. Reads use their own
   connections (WAL) with a `busy_timeout`. Any number of client connections can
   write at the same time without contending for the write lock. Separate offices
   each have their own database and never share a file.
3. **The writer batches.** It runs everything queued in one transaction, each
   request inside its own `SAVEPOINT`, so an ordinary failure (a broken rule, a
   constraint) rolls back only that request. It commits once and only then reports
   each request's result. After each failed request the writer checks whether the
   transaction is still open (`is_autocommit()`). SQLite can end it on its own (for
   example some disk-full, I/O, out-of-memory or busy errors, an interrupt,
   `ON CONFLICT ROLLBACK`, `RAISE(ROLLBACK)`), which also undoes requests that
   already succeeded. If it is gone, the failed request gets its error, and every
   other request in the batch, including those that had succeeded, is re-run in a
   new transaction. If the commit itself fails, the writer rolls back if the
   transaction is still open, and every request that had succeeded gets the error.
   Requests that had already failed keep their own error.
4. **Durability follows the operation's declared class.** WAL is used. Each
   operation is either committed-class or durable-class, as
   [ADR-0014](decision-14%20-%20ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md) lists. A
   committed-class request is acknowledged once its transaction commits, and the
   writer forces a full flush within a bounded interval. A durable-class request
   is acknowledged only after a full flush; on macOS that means `fullfsync`,
   because a plain fsync does not flush the drive's cache. A batch that contains a
   durable request flushes before that reply is sent.
5. **Private persistence adapters.** SQL and `rusqlite` types stay inside the
   office's and the host's store adapters. Domain callers use typed operations. The
   shared projection and the separate private-note database keep distinct access
   and durability boundaries; the combined draft schema does not combine their
   writers or permissions.
6. **Secrets are not in the database in the clear.** An office's private keys are
   stored wrapped by the host-held office secrets key (ADR-0016). Credentials keep
   their own owner-only stores.

## Canonical sources and acknowledgement

| Data | Durable authority | Role of SQLite |
| --- | --- | --- |
| Organization/team/employee/project definitions, task and feature records | Organization Git content mirrored as durable files; acknowledged unpublished changes also need the durable journal | Indexed shared projection |
| Published task activity | Immutable `work-history/tasks/<task-id>/<revision>.json` plus the journal for acknowledged unpublished events | Ordered history projection |
| Usage observations and corrections | Durable accounting archive and checkpoints; immutable used price versions in organization Git | Derived totals and event indexes |
| Chat, recipient state and attachments | Protected messaging sources under document 15 | Authorized shared projection, not the sole recoverable copy |
| Budget policies, extensions, notices and holds | Protected budget sources and recoverable mutation receipts under document 17 | Evaluation/read projection; retained decisions are not regenerated from spend |
| Number allocations, command receipts, placement generations and process evidence | Durable authority, recovery and supervisor journals under documents 06, 11 and 16 | Rebuildable indexes where specified; recreating a cache grants no launch authority |
| Private task notes | Separate durable private database, protected mirrors and receipts, and retention policy under document 05 | Canonical private store, not a disposable shared cache |
| Credentials and authentication state | Protected host and office credential stores | Not part of the organization-data schema |

Every write connection enables `foreign_keys` and `recursive_triggers`. An
accepted change has one recoverable commit boundary covering its domain change,
required journal entry and receipt or outcome. Independent file and SQL writes are
not atomic merely because they share an operation ID. Document 16 requires
evidence before acknowledgement; document 11 specifies complete-envelope
checksums, temporary-file publication, file and directory flushes and gap
detection. Journals get the same power-loss guarantees as the database, including
the macOS full flush for durable-class entries.

A rebuild inventories verified sources and fails explicitly for missing coverage.
Git alone cannot restore chat, usage, budget decisions, private notes or execution
authority. Existing processes are reconciled or fenced before a replacement
launch. A task handoff does not authorize deleting its workspace. Code that is
local-only or unpushed needs the protected recovery checkpoints of document 16
before its runtime cache can be discarded.

## Not covered here

- Organization Git content and runtime checkouts are ordinary files, protected by
  disk encryption and file permissions (ADR-0016). Never put secrets, chat bodies,
  private notes or budget amounts in shared Git to obtain recoverability.
- Automatic backup and restore are implementation work. The data contracts specify
  what must survive.

## Consequences

- Each database has one writer. Readers use WAL. Acknowledgements follow the
  operation's declared class, and failures never manufacture successful receipts.
- The private-note store adds a distinct authorization and recovery boundary.
  Under [ADR-0031](decision-31%20-%20ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md), a private note is read by
  its author and by people who can read the task (read-only for non-authors), and
  never by another agent regardless of assignee, team-lead, or role status; only
  the author edits or deletes a note.
- Recovery tests cover preserved message visibility, accounting gaps, consumed
  budget extensions, independent holds, numbering allocations and live processes,
  as well as successful SQL replay.
- Full source contracts are
  [private notes](../docs/organization-data/doc-79%20-%20Organization-Data-05-—-Operational-Data-and-Private-Notes.md#private-notes-attached-to-tasks),
  [accounting](../docs/organization-data/doc-87%20-%20Organization-Data-10-—-Usage-and-Cost-Accounting.md),
  [execution](../docs/organization-data/doc-88%20-%20Organization-Data-11-—-Model-Profiles-and-Agent-Execution.md),
  [messaging](../docs/organization-data/doc-90%20-%20Organization-Data-15-—-Messaging-and-Chat.md),
  [recovery](../docs/organization-data/doc-91%20-%20Organization-Data-16-—-Recovery-and-Office-Continuity.md) and
  [budgets](../docs/organization-data/doc-92%20-%20Organization-Data-17-—-Budgets-and-Controls.md).
