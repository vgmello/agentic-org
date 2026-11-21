---
id: decision-21
title: >-
  ADR-0021: The office store: one journal of file changes, a projection rebuilt
  from files
date: '2026-11-21T00:46:00Z'
status: implemented
---

> **Review note (2026-11-21):** **Implemented** (B1a/B1b landed: `crates/org-store`, `org-office/src/store/sqlite/`, legacy store deleted, `engine_boundary.rs` active). **Minor drift noted in `TASK-19`:** Decision 9 cites "the store's two tables", whereas `SCHEMA-COVERAGE.md` now documents 4 `store_*` adapter tables alongside verbatim `schema.sql`. Remaining follow-ups: B1c latency/engine-independence guards (`TASK-33`), `org-office` wire cutover (`TASK-14`), restricting SQLite re-exports (`TASK-13`), removing wrapped Org CA key from journal Entry 1 (`TASK-10`), and rebuilding projects/messaging/work families (`TASK-36`, `TASK-45`, `TASK-49`).

# ADR-0021 · The office store: one journal of file changes, a projection rebuilt from files

Status: **accepted** 2026-11-18, answering the owner's request to define the store's new
architecture ("without unnecessary coupling", "without over-engineering"); accepted when
the Track B lead took B1a. Implementation in progress: increments B1a to B1c of the
[implementation plan](../implementation-plan.md). The design is the
[office store](../design/office-store.md). This record states the choices that
[ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md),
[ADR-0006](0006-selected-task-acceptance.md),
[ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md) and
[ADR-0018](0018-storage-engine-independence.md) leave open.

## Context

- Files are the source and the database is a projection that can be deleted
  ([non-negotiables](../architecture-non-negotiables.md) 1 and 2). ADR-0014 decision 6
  makes a journal under `durable/` the commit point and lets files follow.
- Today's store (`crates/org-office/src/store.rs` and `store/`, 14,694 lines outside
  tests) keeps the old tables as the source in two plain SQLite databases (SQLCipher
  until A1, ADR-0016). Its acceptance
  journal records SQL row images with a projection-schema digest, writes one file per
  transaction, asks the host for its custody checkpoint before every write and every
  read, confirms after every write, and re-reads every materialized file on every read.
- ADR-0018 requires one seam for the engine, a second implementation and a suite per
  contract. ADR-0014 requires a committed class that never waits for the drive.

## Decision

1. **One acceptance journal per office, for every record family, is the commit point.**
   An entry is one accepted operation: its receipt, the origin and feed positions it
   took, and the exact bytes of every file it creates, replaces or removes. Entries never
   contain projection rows, table names or engine terms.
2. **The projection is a pure function of the files.** Each family has one projector,
   from a file's path and bytes to rows; the write path and rebuild call the same one.
   Values spanning records are views or queries.
3. **The journal is append-only segments** (`segments/<first-ordinal>.log` under the
   journal incarnation), each entry framed with its length and hashed as stored
   (`raw-bytes/1`), chained to its predecessor and to an immutable anchor. A batch of
   entries is one write. A flusher thread flushes the segment for a group.
4. **Order.** The writer decides on the projection at head, applies each operation's
   files to the batch's projection transaction, appends the batch, commits the
   projection, publishes the change and answers the committed class. The flusher fully
   flushes, answers the durable class, confirms the flushed head to the host's writer
   custody and then writes the files, atomically and flushed, to their final state.
5. **Visible at commit, acknowledged by class, released after flush.** A durable change
   is readable from its projection commit, before its flush. Nothing leaves the office
   (replication, Git publication) before its entry is flushed. Task numbers keep
   ADR-0019's durable mark.
6. **A failed flush is fatal.** The office never retries a failed flush in the same
   process, answers no waiting durable request as success, and exits; a failed append
   fails its batch and is truncated before the next one.
7. **Reads touch only the projection.** No file read, no host call, no writer. The host
   custody is checked at open and confirmed once per flush.
8. **Crates.** One new crate, `org-store`, holds the engine-neutral foundation and the
   in-memory engine for tests. The SQLite engine is `org-office/src/store/sqlite/`, the
   only office module that names `rusqlite`. `Engine` and `CorePort` (one implementation
   per engine) and `Projector` (one per family, generic over the engine) are the seam. Every family has the same shape: records and
   codec, a read port and a write port, a projector, one file per operation, a testkit
   with a memory implementation and a conformance macro, and one SQL module.
9. **The projection schema is the contract's `schema.sql`** plus the store's two tables.
   Its digest is stored in the projection; a different digest deletes and rebuilds it.
   There is no migration code.
10. **The old store is deleted whole** in B1b. Calls only the old tables served answer
   `unsupported` until their family is rebuilt (work B4, messaging B6, process ownership
   C3).
11. **The change stream** is an in-memory ring of committed changes with feed positions
    recorded in the journal; a position older than the ring answers `resync`.

## Rejected

- The files as the commit point: a change spanning several files is not atomic, and a
  durable change would need a flush per file.
- The projection as the commit point: a cache must be deletable.
- SQL row images in the journal, as today: they bind the journal to one engine and one
  schema, against ADR-0018.
- One file per journal entry: in an indicative harness, 16 µs median per entry
  against 0.65 µs for an append (580 µs against 228 µs with a flush), and a flush per
  entry.
- A separate SQLite adapter crate: it would depend on `org-office` for the families that
  live there.
- A database, writer or crate per family: cross-family reads would lose one snapshot,
  and atomic changes would need coordination.
- Replacing the old store family by family: two writers and two copies of organization
  data until B6, and adapters written only to be deleted.
- A host confirmation per write: a host round trip on every acknowledgement.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| STORE-1 | An operation killed before its journal append leaves no trace; killed after it, it is wholly present after reopening, with its receipt. No decision runs again after its entry is appended. |
| STORE-2 | Deleting `runtime/<office-id>/` and reopening yields a projection whose canonical dump equals the one before; the rebuild-equivalence harness passes for every family on every engine. |
| STORE-3 | No journal entry names a table, column or engine; changing the projection DDL changes its digest, rebuilds it, and changes no journal or file byte. |
| STORE-4 | A committed operation is answered while a flush is held back; a durable one is not answered before the flush that covers it; no outbox consumer and no file sees an entry before its flush. |
| STORE-5 | Reads still answer with `durable/` made unreadable and the host channel closed (they read no file and call no host), and see every change acknowledged before they began. |
| STORE-6 | Every materialized file is at the data contract's path in its exact bytes, written atomically; bytes the office did not write stop materialization and name the path. |
| STORE-7 | Adding a family (projects, B3) adds its own module and one SQL module and changes no file of `org-store`. |
| STORE-8 | A flush error injected on a segment, a materialized file or a directory makes the office answer no waiting durable request as success and exit without retrying the flush, and the reopened office holds exactly what the journal on the drive holds; a failed or short append fails its whole batch, leaves no acknowledged operation, and is truncated before the next batch. |

## Consequences

- One write path and one rebuild serve every record family; a new family copies a shape.
- A second engine implements the projection only; the journal and files carry every
  guarantee and do not change.
- Since B1b the running office is on this store: HQ creation is the journal's first
  entry, the organization's files are under `durable/state/`, the projection under
  `runtime/<office-id>/` is rebuilt when deleted, and the host confirms the journal head
  once per flush ([office store](../design/office-store.md) section 11). The old store,
  its two databases and the SQL outside `src/store/sqlite/` are deleted;
  `crates/org-office/tests/engine_boundary.rs` keeps it so (ENG-1 for the office).
- Calls only the old tables served answer `unsupported` until their family is on the
  store: work in B4; operation lookup and the other organization commands in B2;
  messaging, status lines and `subscribe` in B6. The office's native directory reads
  and `team.create` answer from the store.
- An organization created before B1b has no journal; its office is refused, not
  migrated, and the organization is created again.
- The data contract states the journal this record decides: document 16 revision 4,
  document 15 revision 10, document 06 revision 2.10 and document 08 revision 2.10
  (set revision 2.16), with ADR-0006 and ADR-0014 decision 8 worded to match.
