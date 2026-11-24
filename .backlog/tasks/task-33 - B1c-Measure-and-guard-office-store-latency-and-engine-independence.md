---
id: TASK-33
title: 'B1c: Measure and guard office store latency and engine independence'
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T19:04:00Z'
labels:
  - backend
  - rust
  - performance
  - m1
milestone: m-0
dependencies:
  - TASK-1
documentation:
  - docs/implementation-plan.md
  - docs/design/office-store.md
  - docs/adr/0014-acknowledgement-classes-and-agent-os-latency.md
  - docs/adr/0018-storage-engine-independence.md
priority: medium
type: enhancement
ordinal: 33000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Measure office store latency on Linux CI and reference Mac, fix the flush interval from measurements, guard regressions in CI, surface persistent SQLite checkpoint failures instead of dropping errors, and close ADR-0018 ENG-4/ENG-5 and receipt retention/pruning follow-ups (FC-160, FC-165).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Office store benchmark target runs on Linux CI and reference Mac with numbers recorded in ADR-0014 and a CI regression check enabled.
- [x] #2 ACK-2, ACK-3, ACK-5, ACK-6, and ACK-7 pass for organization operations and the flush-bound load test is verified.
- [x] #3 Repeated SQLite checkpoint failures are surfaced rather than silently dropped, and receipt retention/pruning bounds are documented or implemented.
- [x] #4 ENG-4 and ENG-5 wording and trigger/generated-column coverage checks pass across docs and schema coverage.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered B1c office store latency benchmark & CI guard (crates/org-office/examples/store_latency.rs + .github/workflows/ci.yml on macos-15 & ubuntu-26.04; Apple M2 release measurements recorded in doc-66 & doc-3 FC-160: snapshot read p50 0.077ms / p95 0.080ms, committed write p50 0.128ms / p95 3.666ms, durable org write p50 65.114ms, flush_interval=50ms, continuous load ACK-5 PASS), engine-independent ACK-2..ACK-7 suite on SQLite+Memory (crates/org-office/src/store/open/tests/ack.rs), persistent SQLite WAL checkpoint failure/block surfacing (Engine::upkeep_error -> Status::upkeep_warning with real held-reader WAL tests), ReceiptRetentionPolicy bounds & boundary tests (30 days / 50,000 receipts), and executable ENG-4 (Docs 06/11 + ADRs 0006/0014/0019/0021) + live SQLite catalog ENG-5 (112 triggers + 6 generated columns in doc-74 & test_schema.py, 118/118 pass). Separate Fresh Final Rust Reviewer (d72ad9d3) returned VERDICT: APPROVE.
<!-- SECTION:FINAL_SUMMARY:END -->
