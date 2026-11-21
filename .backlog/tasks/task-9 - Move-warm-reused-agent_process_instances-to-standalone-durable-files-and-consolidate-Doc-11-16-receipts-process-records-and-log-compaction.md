---
id: TASK-9
title: >-
  Move warm reused agent_process_instances to standalone durable files and
  consolidate Doc 11/16 receipts, process records, and log compaction
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - architecture
  - runtime
  - data-model
  - p1
milestone: m-3
dependencies:
  - TASK-2
references:
  - docs/design/organization-data/schema.sql
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/organization-data/11-model-profiles-and-agent-execution.md
  - docs/design/organization-data/15-messaging-and-chat.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
  - docs/design/organization-data/17-budgets-and-controls.md
priority: high
type: enhancement
ordinal: 9000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings H-09, M-04, M-05: Doc 11 embeds multi-turn reused agent_process_instances inside a single turn's turn.json, causing split-brain file ownership when Turn 2 mutates or stops Turn 1's process (violating Non-Negotiable #3). Additionally, Doc 11 and Doc 16 duplicate receipts (supervisor_command_receipts vs mutation_receipts) and process ownership records (agent_process_instances vs process_cleanup_records), Doc 15 creates 1 JSON file per (employee, message) risking inode bloat, Doc 17 duplicates policy across budget.md and versions/*.yaml, and append-only streams lack compaction/pruning windows.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 agent_process_instances has its own canonical durable file path (supervision/processes/<process-instance-id>.json) referenced by turns and attempts, preserving single-writer ownership.
- [ ] #2 Duplicate Doc 11/16 receipt and process-cleanup models are consolidated onto canonical mutation_receipts (using access-checked result_ref) and process lifecycle records.
- [ ] #3 Per-participant messaging receipt storage avoids unbounded 1-file-per-(employee, message) inode growth, budget policy duplication is eliminated, and archival/compaction windows are defined for expired receipts and historical ledgers.
- [ ] #4 Docs 11, 15, 16, 17, schema.sql, and test_schema.py agree with zero drift.
<!-- AC:END -->
