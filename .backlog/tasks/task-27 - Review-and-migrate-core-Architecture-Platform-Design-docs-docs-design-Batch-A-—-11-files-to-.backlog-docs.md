---
id: TASK-27
title: >-
  Review and migrate core Architecture & Platform Design docs (docs/design/,
  Batch A — 11 files) to .backlog/docs/
status: Done
assignee: []
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T00:49:00Z'
labels:
  - docs
  - architecture
  - backlog-migration
milestone: m-5
dependencies:
  - TASK-19
documentation:
  - docs/design/documentation-authority.md
  - docs/architecture/architecture-and-data-audit.html
priority: medium
type: docs
ordinal: 27000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Individually review each core architecture/platform design document in docs/design/ against current v1 code and the documentation authority map, then migrate them into .backlog/docs/design/ via `backlog doc create`.

Files in scope (11):
- docs/design/accounts-sandbox-and-extensions.md
- docs/design/documentation-authority.md
- docs/design/executable-migration.md
- docs/design/linux-host.md
- docs/design/office-host-boundary.md
- docs/design/office-store.md
- docs/design/offices-hq-and-satellites.md
- docs/design/project-structure.md
- docs/design/rust-browser-bff-and-static-sveltekit.md
- docs/design/standalone-server.md
- docs/design/web-api-v1.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 All 11 architecture/platform design documents are individually reviewed for drift against implemented crates, binaries, and ADRs.
- [x] #2 All 11 documents are migrated into .backlog/docs/design/ via `backlog doc create` with reviewed updates applied.
- [x] #3 Old path -> Backlog doc ID mapping is recorded.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Read and review all 11 core Architecture & Platform Design docs in docs/design/ (Batch A) against current crates, binaries, ADRs (decision-1..32), TASK-19 synchronization, audit tasks (TASK-5..TASK-18), and doc-1 (Vision).
2. Migrate all 11 documents into .backlog/docs/design/ via backlog doc create -p design and backlog doc update --content --tags -t, preserving full technical substance and prepending concise Review note (2026-11-21) callouts for verified drift, open tasks, or v2 deferrals.
3. Verify with backlog doc list and backlog doctor, record old path -> doc-XX mapping in TASK-27 notes, check ACs #1-#3, finalize Done, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Completed individual review and migration of all 11 core Architecture & Platform Design docs (docs/design/, Batch A) into .backlog/docs/design/.

| Source Path | Doc ID | Title | Key Review Notes |
| --- | --- | --- | --- |
| docs/design/accounts-sandbox-and-extensions.md | doc-61 | Design: Dedicated Accounts, Agent Sandbox and Extensions | Proposed (Owner D1-D28, 2026-11-20); tracked in TASK-57, TASK-58, TASK-59, TASK-61; ADR-0007/0011 v2 deferral updated in TASK-19 (TASK-72). |
| docs/design/documentation-authority.md | doc-62 | Design: Documentation Authority Map | Legacy docs/ links cut over in TASK-32; noted ADR-0030 (decision-30) companion reference for agent turn starts. |
| docs/design/executable-migration.md | doc-63 | Design: Executables and Entry Points | Synced in TASK-19; flagged remaining Linux setup row wording, unreached-libraries paragraph drift (org-release/org-platform confinement), B2 Q8 bridge (TASK-5), org-spool/binary thinning (TASK-15). |
| docs/design/linux-host.md | doc-64 | Design: Linux and Container Hosting | X1a-X5 landed; flagged stale §3 kernel 6.2 floor (amended to 5.9+ seccomp + optional Landlock) and stale §6 Code today paragraph; X6/X7/image tracked in TASK-62, TASK-63, TASK-64. |
| docs/design/office-host-boundary.md | doc-65 | Design: Host and Office Worker Boundary | Foundation live; flagged v2 deferral of escrow/recovery (TASK-72), wrapped CA key removal from journal Entry 1 (TASK-10), bridge/channels (TASK-5, TASK-16), ADR-0018/leaks (TASK-13), wire cutover (TASK-14), UUIDv7/gateway renames (TASK-15). |
| docs/design/office-store.md | doc-66 | Design: Office Store Architecture | B1a/B1b/B2.1 landed; open work in TASK-6..8, TASK-10, TASK-13, TASK-14, TASK-18, TASK-33 (B1c), TASK-36/42/45/49; coordinate §13.5 schema.sql include_str! with TASK-26/TASK-32. |
| docs/design/offices-hq-and-satellites.md | doc-67 | Design: Offices, HQ and Satellites | Flagged Placement HQ-permit drift (TASK-2 decoupled offline permits), v2 escrow/promotion deferral (TASK-72) vs v1 Git import/re-enroll (TASK-39, TASK-52), stale org-sync/old-tables code note (removed in B1b). |
| docs/design/project-structure.md | doc-68 | Design: v1 Project Structure | Synced in TASK-19; flagged unreached-libraries table listing live org-platform/org-store and omitting org-platform-host; open work in TASK-12, TASK-15, TASK-17, TASK-72. |
| docs/design/rust-browser-bff-and-static-sveltekit.md | doc-69 | Design: Rust Browser BFF and Static SvelteKit Architecture | Core live; open work in TASK-5 (bridge), TASK-12 (productionImportGuard), TASK-16 (multiplexed SSE/office channels), TASK-17 (UI module layout). |
| docs/design/standalone-server.md | doc-70 | Design: Standalone Agentic Org Server | Flagged v2 deferral of CA escrow/recovery gate (TASK-72) and major drift in "What the code does today" (predates B1b org-store, A1 launch cleanup, A3/A4 web routes, X2-X5 Linux hosting). |
| docs/design/web-api-v1.md | doc-71 | Design: Web HTTP API v1 | 9 host routes live (flagged bottom paragraph omitting POST /organizations and GET /operations/host/{id}); open work in TASK-5, 15, 16, 17, 34, 38, 49, 50; recovery/cancel deferred (TASK-72, FC-188). |
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Reviewed all 11 core Architecture & Platform Design docs (docs/design/, Batch A) individually against current crates, binaries, ADRs (decision-1..32), TASK-19 synchronization, audit tasks (TASK-5..TASK-18), and doc-1 (Vision), and migrated all 11 into .backlog/docs/design/ (doc-61 through doc-71) via backlog doc create/update with full technical substance preserved and Review note (2026-11-21) callouts. Verified with backlog doctor.
<!-- SECTION:FINAL_SUMMARY:END -->
