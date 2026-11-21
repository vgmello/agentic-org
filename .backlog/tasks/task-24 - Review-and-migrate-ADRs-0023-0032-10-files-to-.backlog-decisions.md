---
id: TASK-24
title: Review and migrate ADRs 0023-0032 (10 files) to .backlog/decisions/
status: Done
assignee: []
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:47:00Z'
labels:
  - docs
  - adr
  - backlog-migration
milestone: m-5
dependencies:
  - TASK-19
documentation:
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 24000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Review ADRs 0023 through 0032 individually against the current codebase and audit findings (including removing superseded approval prose from ADR-0027 vs ADR-0029 and reconciling ADR-0032 offline/token notes) and migrate each into .backlog/decisions/ via `backlog decision create`.

Files in scope (10):
- docs/adr/0023-an-agents-ask-starts-a-turn.md
- docs/adr/0024-a-channel-question-is-watched-for-its-answer.md
- docs/adr/0025-agents-start-sub-agents.md
- docs/adr/0026-development-processes-are-content.md
- docs/adr/0027-a-lead-approves-a-hire.md
- docs/adr/0028-an-agent-is-always-available.md
- docs/adr/0029-only-who-may-hire-hires.md
- docs/adr/0030-an-agent-works-while-it-has-work.md
- docs/adr/0031-any-employee-changes-tasks.md
- docs/adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Each of ADRs 0023-0032 is reviewed individually for accuracy, supersession status, and open contradictions.
- [x] #2 All 10 decisions are created in .backlog/decisions/ via `backlog decision create` with clean non-contradictory text and accurate statuses.
- [x] #3 A mapping table records ADR-0023..0032 -> Backlog decision IDs.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Read and review ADRs 0023-0032 individually against current codebase, TASK-19 updates, doc-1 (Vision), supersession status (ADR-0027 vs ADR-0029), and ADR-0032 offline/token notes.
2. Create 10 Backlog decisions (decision-23..decision-32) under .backlog/decisions/ via backlog decision create with accurate statuses, clean non-contradictory text, full technical substance, and Review note (2026-11-21) callouts.
3. Verify with backlog decision list and backlog doctor, record ADR-0023..0032 -> decision-23..32 mapping in TASK-24 notes, check ACs #1-#3, finalize Done, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed individual review and migration of ADRs 0023-0032 into .backlog/decisions/.

| Source Path | Decision ID | Status | Key Review Notes |
| --- | --- | --- | --- |
| docs/adr/0023-an-agents-ask-starts-a-turn.md | decision-23 | accepted | Pending in TASK-47 (C8); extended by ADR-0028/0030; TASK-2 decoupled permits from HQ reachability. |
| docs/adr/0024-a-channel-question-is-watched-for-its-answer.md | decision-24 | accepted | Pending in TASK-48 (C9); 50-excerpt accuracy gate (WATCH-11) required first. |
| docs/adr/0025-agents-start-sub-agents.md | decision-25 | accepted | Pending in TASK-42 (C3); amended 2026-11-19 Decision 6 persists background sub-agents across turns (doc-27 §4 needs alignment). |
| docs/adr/0026-development-processes-are-content.md | decision-26 | accepted | Aligned with doc-1; pending in TASK-55 (docs tree/index) and TASK-56 (L12-L14 process packages/DSS). |
| docs/adr/0027-a-lead-approves-a-hire.md | decision-27 | superseded | Partially superseded by ADR-0029 (TASK-19 removed approval/escalation prose); surviving may_hire decisions 3, 4, 7 + HIRE-4 implemented with ADR-0029 in TASK-41. |
| docs/adr/0028-an-agent-is-always-available.md | decision-28 | accepted | Aligned with doc-1; pending in TASK-47 (C10); extended by ADR-0030. |
| docs/adr/0029-only-who-may-hire-hires.md | decision-29 | accepted | Supersedes ADR-0027 approval/escalation; pending in TASK-41 (C2). |
| docs/adr/0030-an-agent-works-while-it-has-work.md | decision-30 | accepted | Pending in TASK-42/47; flagged drift in Decisions 2-3 citing HQ reachability for turn permits (decoupled in TASK-2). |
| docs/adr/0031-any-employee-changes-tasks.md | decision-31 | accepted | Aligned with doc-1; pending in TASK-49, TASK-50, TASK-67, plus schema/security prep in TASK-6, TASK-8, TASK-10. |
| docs/adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md | decision-32 | accepted | Pending in TASK-42, TASK-54, TASK-11 (env scrub live check, CA/token key separation); Codex centralized refresh gated on live token turn + vendor refresh path; Linux unverified. |
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed ADRs 0023-0032 individually against current implementation, TASK-19 updates (clean ADR-0027 vs ADR-0029 supersession), TASK-2 offline permit remediation, audit tasks (TASK-6..11), and doc-1 (Vision), and migrated all 10 into .backlog/decisions/ (decision-23 through decision-32) with accurate statuses, non-contradictory text, and Review note (2026-11-21) callouts. Verified with backlog doctor.
<!-- SECTION:FINAL_SUMMARY:END -->
