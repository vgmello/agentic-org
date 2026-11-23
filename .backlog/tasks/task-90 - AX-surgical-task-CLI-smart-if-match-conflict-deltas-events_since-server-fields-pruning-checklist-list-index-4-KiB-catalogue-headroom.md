---
id: TASK-90
title: >-
  AX surgical task CLI & smart --if-match conflict deltas (events_since, server
  --fields pruning, checklist list/--index, 4 KiB catalogue headroom)
status: To Do
assignee: []
created_date: '2026-11-23T03:22:00Z'
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
- [ ] #1 Recovers launch-catalogue headroom under CATALOGUE_LIMIT = 4096 across org-agent-cli tables so new AX/draft/search commands pass org_command contract checks cleanly; updates org_command::Request to carry requested --fields roots and converts optional TASK_INSPECT_OUTPUT sections to Field::optional so shape::conform succeeds when tasks.inspect prunes unrequested sections (body_markdown, comments, watchers, dependencies, history) server-side.
- [ ] #2 Adds agent-os checklist list --task <ref> [--section acceptance|dod|work|all] [--unchecked], supports --section and repeatable --text on checklist add, and supports 1-based --index (including batch check) on checklist check|edit|remove|move when expected_revision (--if-match) is supplied (refusing ordinal mutations without expected_revision while keeping UUIDv7 --step-id working); CLI checklist mutations return compact mutated-step + per-section progress receipts; task update supports field-level PatchField patching on task_details and append_body (mutually exclusive with full body replacement); EVENT_FIELDS exposes change.
- [ ] #3 Stale expected_revision < current_revision on task/ordinal-checklist mutations returns ErrorCode::Conflict with a bounded ConflictDetail (<= 20 events, <= 8 KiB serialized JSON, events_truncated, omitted_events, changed_sections, reread_recommended, compact current_summary — never inlining body_markdown or comment bodies), with enriched task_updated and checklist_step_* event deltas; updates org_proto::ApiError, org_command::Refusal, envelope_schema(), org_agent_cli map_client_error (populating both retry_with.if-match and error.conflict), and agentic-org-web HTTP 409 responses.
- [ ] #4 Preserves strict ADR-0031 security ordering (require_active_employee runs before task lookup/conflict building so unauthorized callers receive not_authorized with zero conflict metadata, and private task notes never appear in events_since) and verifies rebuild-after-child-ops conflict deltas across Memory and SQLite engines, org-command checks, CLI/session attach, and HTTP 409 tests; updates doc-28, doc-77, doc-79, doc-49 (TRI-6), doc-3, and records a concise ADR.
<!-- AC:END -->
