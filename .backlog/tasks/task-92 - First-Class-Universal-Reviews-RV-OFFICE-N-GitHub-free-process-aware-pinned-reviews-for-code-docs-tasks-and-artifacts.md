---
id: TASK-92
title: >-
  First-Class Universal Reviews (RV-<OFFICE><N>): GitHub-free, process-aware,
  pinned reviews for code, docs, tasks, and artifacts
status: To Do
assignee: []
created_date: '2026-11-23T03:40:00Z'
labels:
  - backend
  - ui
  - cli
  - reviews
  - ax
  - blog
milestone: m-0
dependencies:
  - TASK-51
  - TASK-90
documentation:
  - .backlog/docs/design/doc-35 - Design-Projects-Git-and-GitHub.md
  - >-
    .backlog/docs/design/doc-32 -
    Design-Development-Processes-Process-Packages.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - >-
    .backlog/docs/organization-data/doc-74 -
    Organization-Data-—-Schema-Coverage-and-Integrity-Review.md
  - >-
    .backlog/docs/organization-data/doc-80 -
    Organization-Data-06-—-Identity-and-Numbering.md
  - .backlog/docs/specs/doc-52 - Spec-Knowledge-Decisions-and-Tech-Debt-UX.md
  - >-
    .backlog/decisions/decision-26 -
    ADR-0026-A-development-process-is-content-the-product-runs-no-methodology.md
  - >-
    .backlog/decisions/decision-28 -
    ADR-0028-An-agent-is-always-available-a-persons-message-and-a-task-assignment-also-start-it.md
  - >-
    .backlog/decisions/decision-31 -
    ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md
priority: high
type: feature
ordinal: 98000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Makes reviews a first-class, GitHub-free, methodology-agnostic product primitive (RV-<OFFICE_CODE><N> — avoiding R- release key collision) across code worktrees, tasks, documents, processes, and arbitrary artifacts without creating fake Kanban tasks. Supports both internal process reviews and routing to dedicated hired reviewer/architect agents or humans, pinning every round to an exact immutable commit OID or revision/hash with line/section-anchored finding threads, verdicts, ADR-0028 wakeups, human Inbox surfacing, warm resumable reviewer sessions on follow-up rounds (with --fresh for independent final reviews), and token-efficient CLI/diff reads.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Universal reviews persist as first-class records under durable/state/reviews/<id>/ (zero-padded rounds/{round:06}.json and events/{revision:010}.json, single-writer office per review) with offline-safe RV-<OFFICE_CODE><N> counters at durable/local/offices/<office-id>/review-counter.json, projecting into 7 STRICT SQLite + Memory tables passing ADR-0018 parity, delete+rebuild, and test_schema.py.
- [ ] #2 Code reviews run 100% offline against the office org-git bare cache (refusing dirty worktrees, pinning immutable base..head commit OIDs, bounded diff_commits / read_blob_lines with --stat-first token efficiency and inter-round diffs), while task/document/process/artifact reviews pin immutable revisions/hashes and flag stale pins whenever targets change.
- [ ] #3 Reviewers submit anchored findings (general, file_line, doc_section, checklist_item) with append-only replies and round-pinned verdicts (approve, changes_requested, comment, incomplete; atomic batch findings in review verdict), enforcing independence (reviewer != author when independent=true), ADR-0026 open-slug kinds and advisory guidance_refs/criteria (zero hardcoded methodology engine), and single-agent routing (explicit, claimable pool, or deterministic --auto-assign).
- [ ] #4 ADR-0028 wakes (ReviewRequested, ReviewVerdict, deduplicated ReviewReply, respecting 12/hr agent limit, person exemption, and advisory >=5-round escalation), warm resumable reviewer session affinity on follow-up rounds (plus --fresh independent override), ReviewInboxSource (OpenTarget::Review) with live SSE, 9-verb agent-os review CLI within CATALOGUE_LIMIT = 4096, and Browser Review Workbench (/reviews, /reviews/[ref], Inbox, Task widget) work end-to-end.
- [ ] #5 Updates schema.sql, test_schema.py, Organization-Data-19 (or doc-77/79 + doc-72/73/74/80), doc-28, doc-35, doc-52, doc-3, and records a new ADR documenting Universal Reviews, single-writer routing, started_by_kind mapping, and ADR-0026 compliance.
<!-- AC:END -->
