---
id: TASK-90
title: >-
  AX surgical task CLI & smart --if-match conflict deltas (events_since, server
  --fields pruning, checklist list/--index, 4 KiB catalogue headroom)
status: To Do
assignee: []
created_date: '2026-11-23T03:22:00Z'
updated_date: '2026-11-23T04:54:00Z'
labels:
  - ax
  - backend
  - rust
  - cli
  - tasks
milestone: m-0
dependencies:
  - TASK-87
  - TASK-88
  - TASK-85.3
documentation:
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - >-
    .backlog/docs/organization-data/doc-77 -
    Organization-Data-03-—-Features-and-Work.md
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
  - .backlog/docs/specs/doc-49 - Spec-Work-Items-and-Tasks-UX.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
  - >-
    .backlog/docs/research/doc-95 -
    Research-Backlog.md-vs-Agentic-Org-Gap-Analysis.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
priority: high
type: feature
ordinal: 96000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Prevents agents from polluting their context windows by re-reading entire task documents on every read, checklist tick, or optimistic-concurrency conflict. Solves the 4 KiB org-agent-cli launch catalogue ceiling (currently ~4,044/4,096 bytes), adds server-side --fields pruning on tasks.inspect, adds surgical checklist list and 1-based --section/--index patching with mandatory --if-match, upgrades task_details to field-level PatchField mutations plus append_body, and returns bounded hybrid ConflictDetail payloads (events_since <= 20 / 8 KiB + compact current_summary + reread_recommended) on stale --if-match revisions.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Builds on the progressive 4 KiB catalogue headroom delivered by TASK-85.3 so new surgical checklist/task commands pass org_command contract checks cleanly; updates org_command::Request to carry requested --fields roots and converts optional TASK_INSPECT_OUTPUT sections to Field::optional so shape::conform succeeds when tasks.inspect prunes unrequested sections (body_markdown, comments, watchers, dependencies, history) server-side.
- [ ] #2 Adds agent-os checklist list --task <ref> [--section acceptance|dod|work|all] [--unchecked], supports --section and repeatable --text on checklist add, sets if_match: IfMatch::Optional (NOT IfMatch::Required, preserving UUIDv7 --step-id mode) on checklist check|edit|remove|move (task_table.rs:772, 790, 808, 826) while enforcing --if-match on 1-based --section/--index mutations (including batch check) in task_dispatch.rs + org-office, adds --optional paired with --required on checklist edit (AX-AUDIT-10), returns compact mutated-step + per-section progress receipts, adds work_type/effort/severity to TASK_FIELDS and --work-type to task update (AX-AUDIT-19), upgrades task update to field-level PatchField + append_body (mutually exclusive with full body replacement), and exposes change on EVENT_FIELDS.
- [ ] #3 Stale expected_revision < current_revision on task/ordinal-checklist mutations returns ErrorCode::Conflict with a bounded ConflictDetail (<= 20 events, <= 8 KiB serialized JSON, events_truncated, omitted_events, changed_sections, reread_recommended, compact current_summary — never inlining body_markdown or comment bodies), with enriched task_updated and checklist_step_* event deltas; updates org_proto::ApiError, org_command::Refusal, envelope_schema(), generic ConflictDetail handling in crates/org-agent-cli/src/office.rs:218-229 (map_client_error populating both retry_with.if-match and error.conflict so non-task domains can opt in cleanly), and agentic-org-web HTTP 409 responses.
- [ ] #4 Preserves strict ADR-0031 security ordering (require_active_employee runs before task lookup/conflict building so unauthorized callers receive not_authorized with zero conflict metadata, and private task notes never appear in events_since) and verifies rebuild-after-child-ops conflict deltas across Memory and SQLite engines, org-command checks, CLI/session attach, and HTTP 409 tests; updates doc-28, doc-77, doc-79, doc-49 (TRI-6), doc-96, doc-3, and records a concise ADR.
<!-- AC:END -->
