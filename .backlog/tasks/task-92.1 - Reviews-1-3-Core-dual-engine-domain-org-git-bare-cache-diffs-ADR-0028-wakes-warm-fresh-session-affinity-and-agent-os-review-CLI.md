---
id: TASK-92.1
title: >-
  Reviews 1/3: Core dual-engine domain, org-git bare-cache diffs, ADR-0028
  wakes, warm/fresh session affinity, and agent-os review CLI
status: To Do
assignee: []
created_date: '2026-11-23T03:40:00Z'
updated_date: '2026-11-23T03:41:00Z'
labels:
  - backend
  - rust
  - cli
  - reviews
  - ax
milestone: m-0
dependencies:
  - TASK-51
  - TASK-90
documentation:
  - .backlog/docs/design/doc-35 - Design-Projects-Git-and-GitHub.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - >-
    .backlog/docs/organization-data/doc-74 -
    Organization-Data-—-Schema-Coverage-and-Integrity-Review.md
  - >-
    .backlog/docs/organization-data/doc-80 -
    Organization-Data-06-—-Identity-and-Numbering.md
  - >-
    .backlog/decisions/decision-26 -
    ADR-0026-A-development-process-is-content-the-product-runs-no-methodology.md
  - >-
    .backlog/decisions/decision-28 -
    ADR-0028-An-agent-is-always-available-a-persons-message-and-a-task-assignment-also-start-it.md
  - >-
    .backlog/decisions/decision-31 -
    ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md
parent_task_id: TASK-92
priority: high
type: feature
ordinal: 99000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implements the backend domain, dual-engine SQLite/Memory storage, hardened org-git bare-cache diff helpers, ADR-0028 review wakeups with warm/fresh reviewer session affinity, and the 9-verb token-efficient agent-os review CLI.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Adds 7 STRICT tables + immutability triggers in schema.sql (office_review_counters, reviews with generated RV-<OFFICE_CODE><N> key, review_rounds, review_reviewers, review_findings, review_finding_replies, review_events) plus Organization Data contract & doc-74 ENG-5/test_schema.py sync (explicitly deciding record_refs/links review membership), backed by durable/state/reviews/<id>/ (zero-padded rounds/{round:06}.json, events/{revision:010}.json, single-writer office per review) and durable/local/offices/<office-id>/review-counter.json owned exclusively by ReviewFiles::new(office_id) with dual-engine Memory/SQLite + delete+rebuild parity.
- [ ] #2 Adds hardened read-only helpers to crates/org-git (resolve_worktree_head, diff_commits with --stat/path filter/inter-round diffs/binary skip/256 KiB cap, read_blob_lines) running under RepoLocks with AllowedProtocol::None, GitCredential::None, validate_commit_oid, and -- <path> guards; composed with ReviewService in org-office-server (preserving zero org-git dep in org-office) using runtime git-state.json worktree lookup, refusing dirty worktrees, and computing pin_stale on inspect/list.
- [ ] #3 Implements anchored findings (general, file_line with anchor_snippet_sha256, doc_section, checklist_item), append-only thread replies, round-pinned verdicts (approve, changes_requested, comment, incomplete; atomic batch findings[] in review verdict), independence enforcement (reviewer != author when independent=true), open-slug kinds/categories + advisory guidance_refs/criteria (ADR-0026), and explicit/pool/single-agent --auto-assign routing.
- [ ] #4 Integrates ReviewRequested, ReviewVerdict, and deduplicated ReviewReply wakes with SessionManager (explicitly resolving agent_execution_turns.started_by_kind CHECK in schema.sql:1797, honoring 12/hr agent limit, person exemption, pause/budget/stall holds, advisory >=5-round escalation), supports resuming a reviewer saved session on follow-up rounds with an explicit --fresh independent override, enforces require_active_employee + project scoping before lookup/diffing (zero private-note leaks per ADR-0031), and ships all 9 agent-os review verbs (request, list, inspect, diff, route, finding, verdict, resubmit, close) within CATALOGUE_LIMIT = 4096 with server-side --fields pruning.
- [ ] #5 Updates doc-28, doc-35, doc-72/73/74/80, doc-3, test_schema.py, and adds a new ADR; cargo fmt/clippy/test, engine_boundary, and org_command contract checks pass.
- [ ] #6 Review session orchestration supports two-stage convergence: warm session resume on Round 2..N resubmissions (passing inter-round diff plus fix-verification and regression/unexamined-area checks) and explicit --fresh empty-session final validation pass once iterative findings are resolved, recording session_mode (warm | fresh) on each review round.
<!-- AC:END -->
