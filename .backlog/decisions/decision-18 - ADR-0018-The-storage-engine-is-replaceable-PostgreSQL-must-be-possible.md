---
id: decision-18
title: 'ADR-0018: The storage engine is replaceable; PostgreSQL must be possible'
date: '2026-11-21T00:45:00Z'
status: accepted
---

> **Review note (2026-11-21):** Partially implemented: `org-office` moved onto `org-store` (`Engine`/`CorePort`) with `crates/org-office/tests/engine_boundary.rs` (B1b). **Open compliance gaps:** bringing `org-host` into ADR-0018 compliance and restricting `org-office` SQLite re-exports (`TASK-13`), reducing SQLite-specific trigger overhead (`TASK-18`), and completing latency/engine-independence guards (`TASK-33`, B1c). No PostgreSQL adapter is built in v1.

# ADR-0018 · The storage engine is replaceable; PostgreSQL must be possible

Status: **accepted by the owner 2026-11-17**: "we will need to
support PostgreSQL; we need to be able to abstract what we need there." Implementation
pending. No PostgreSQL adapter is built now. Builds on
[ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md) and
[ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md) decision 8.

## Context

- SQL exists in two crates, `org-office` and `org-host`. The domain crates
  (`org-work`, `org-mailbox`, `org-sync`) contain none; they define the storage
  operations they need, and the office store implements them.
- In `org-office`, 23 of 26 files that use SQLite are under the private `store` folder,
  about 14,700 lines split by area. Two files outside it use SQL directly:
  `organization/bootstrap.rs` and `organization/admission.rs`. A third, `messaging.rs`,
  uses SQL only in its tests.
- The office uses one concrete store type. Nothing selects an engine at start-up.
- Contracts named SQLite mechanisms: `BEGIN IMMEDIATE` in documents 06 and 11 until this
  review reworded them, and a draft schema written in SQLite's dialect with generated
  columns and triggers.
- Two storage contracts already have a suite that does not depend on the engine:
  replication, and messaging (`crates/org-mailbox/src/testkit/conformance.rs`, which
  runs against an in-memory store and against SQLite).
- The single writer does two jobs. It fits SQLite's one-writer engine, and it keeps
  sequence allocation, deduplication and outbox writes atomic.

## Decision

1. **Only store adapters know an engine.** No SQL, driver type, pragma or engine name
   appears in a domain crate, a wire contract or an application's composition beyond
   choosing the adapter. The two files outside the store folder move behind it.
2. **Invariants are stated without an engine.** The rule is "one accepting writer per
   stream, with sequence allocation, deduplication and outbox writes committed
   together", not "one database connection". An adapter may use one connection or
   many. Contract text that names an SQLite statement is reworded as the guarantee it
   provides.
3. **Durability is a declared class.** As ADR-0014 decision 8 requires, an operation
   states committed or durable and the adapter honours it.
4. **The store is chosen at composition.** The office and the host each construct
   their store through one seam, so a second implementation can be selected by
   configuration without changing callers.
5. **Every storage contract has an engine-independent test suite.** The replication
   and messaging suites already work this way. The work, organization and host stores
   get the same, and a new engine is accepted only when it passes them all.
6. **The draft schema is the reference shape, not the only dialect.** The combined
   schema stays the executable SQLite reference. Constraints it enforces with
   SQLite-specific features are also listed in words in schema coverage, so that
   another engine can enforce them its own way.
7. **Portable values.** IDs are UUIDv7, written as text in files and on the wire; an
   adapter stores them as text or in the engine's native UUID type. Times are UTC text
   or integers in the units the contracts state, and money and tokens are integers. No
   contract depends on a type only one engine has.
8. **Encryption, backup and file layout are adapter properties.** With ADR-0016 there
   is no database encryption to carry across. Where the data lives is the storage
   contract's `durable/` and `runtime/` split; a server-based engine keeps its files
   elsewhere and still honours which records are sources and which are caches.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| ENG-1 | A search of the workspace finds SQL and driver types only inside the store adapters of `org-office` and `org-host`. |
| ENG-2 | The office and host start with their store selected through one seam, proven by running the suites of ENG-3 against a second, in-memory implementation. |
| ENG-3 | Engine-independent suites exist for work, messaging, organization, replication and host storage, and the SQLite adapter passes them. |
| ENG-4 | Documents 06 and 11 and the ADRs state numbering, permit and journal guarantees without naming an SQLite statement. |
| ENG-5 | Schema coverage lists every constraint the SQLite schema enforces with a trigger or generated column. |

## Consequences

- A later PostgreSQL adapter is an implementation of existing contracts, checked by
  existing suites, with domain rules, wire contract and UI untouched.
- There is a cost now: two files to move, one seam to add and three suites to write,
  for an engine that is not being built yet. Doing it alongside the storage move
  (implementation plan B1) touches the same code once.
