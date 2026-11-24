---
id: TASK-106
title: >-
  v2: Storage Write-Throughput Hardening — Decouple org-store Flusher,
  Append-Only Messaging Segments & Bounded WAL Checkpoint Valve
status: To Do
assignee: []
created_date: '2026-11-24T04:52:00Z'
labels:
  - v2
  - backend
  - rust
  - storage
  - performance
  - messaging
  - docs
milestone: m-1
dependencies:
  - TASK-52
  - TASK-97
documentation:
  - >-
    .backlog/docs/organization-data/doc-90 -
    Organization-Data-15-—-Messaging-and-Chat.md
  - >-
    .backlog/docs/organization-data/doc-91 -
    Organization-Data-16-—-Recovery-and-Office-Continuity.md
  - >-
    .backlog/decisions/decision-14 -
    ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md
priority: medium
type: enhancement
ordinal: 121000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Addresses the real storage write-latency and amplification bottlenecks identified in the 2026-11-24 Principal Systems Architect review without splitting tasks out of office.db (which breaks 9 outgoing FKs, 5 views, 15 triggers, and gives 0% write parallelism): (1) decouple org-store's fast journal group-flush from slow per-file F_FULLFSYNC materialization; (2) replace messaging's O(M²) full received/snapshot.json rewrite with append-only received/segments/*.jsonl per doc-90:50; (3) add a bounded WAL checkpoint pressure valve in SqliteEngine. Note: if production telemetry ever proves >5,000+ sustained writes/sec single-writer saturation, decouple protected/messaging.db into its own standalone Store + acceptance journal (eliminating commit_gate) — never split tasks.db from office.db.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Decouples crates/org-store/src/flusher.rs:87-148 so fast journal group-flush + host custody confirm (unblocking Ack::Durable callers) runs independently from background Files::materialize() per-file F_FULLFSYNC (files.rs:244), and releases Core::flow during journal.append disk I/O (store.rs:276-289, B1c/F8).
- [ ] #2 Migrates messaging upsert_participant_snapshot (crates/org-office/src/messaging/features.rs:1165-1201) and upsert_states_batch (store/open/messaging_ops.rs:613-651) from full received/snapshot.json rewrites to append-only received/segments/<first-seq>.jsonl with periodic compaction (matching existing messaging/projector.rs:227-239, doc-90:50, and doc-91:316).
- [ ] #3 Adds a bounded WAL checkpoint pressure valve (e.g., triggering bounded RESTART/TRUNCATE when -wal exceeds 64 MiB under sustained read load) to SqliteEngine (crates/org-office/src/store/sqlite/mod.rs:174, 660) with regression tests proving WAL size stays bounded under concurrent readers.
- [ ] #4 Verifies dual-engine parity, Delete+Rebuild, crash/restart recovery, and write-burst latency benchmarks (proving per-file materialization bursts no longer stall Ack::Durable journal flushes); updates doc-90 and doc-91 as needed.
<!-- AC:END -->
