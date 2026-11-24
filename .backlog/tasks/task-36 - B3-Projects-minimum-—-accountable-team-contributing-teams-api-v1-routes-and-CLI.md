---
id: TASK-36
title: >-
  B3: Projects minimum — accountable team, contributing teams, /api/v1 routes,
  and CLI
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T08:04:00Z'
labels:
  - backend
  - rust
  - m1
milestone: m-0
dependencies:
  - TASK-34
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/02-project-structure.md
priority: high
type: feature
ordinal: 36000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement the projects record family on org-store with HQ numbering and slugs, accountable team (auto-creating a new team under TEAM-1 by default or binding an existing team), contributing teams, /api/v1 project routes, and project list|inspect|create CLI rows (FC-030, FC-038).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Project records, HQ number/slug allocation, default owning-team creation, and contributing teams pass the engine-independent suite on memory and SQLite.
- [x] #2 Creating, listing, and inspecting projects work through /api/v1 and CLI and survive office restart and runtime projection rebuild.
- [x] #3 FEATURES rows FC-030 and FC-038 are updated with verified evidence.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add B3 projects family in crates/org-office/src/projects/ (records: Project, ProjectCounter, ProjectAllocation under state/projects/<id>/project.yaml + state/identity/{counters,allocations}/organization/<org-id>/project; ports: ProjectsRead: OrganizationRead, ProjectsWrite: OrganizationWrite, ProjectsEngine; projector: ProjectFiles; features: CreateProject with HQ P-<n> numbering, canonical slug derivation/validation, default owning team creation under TEAM-1 or binding existing active team, contributing_team_ids; AddContributingTeam; RemoveContributingTeam; plus archive_team guard refusing teams that own active projects).
2. Implement ProjectsRead/ProjectsWrite on SqliteEngine (using existing schema.sql projects, project_contributing_teams, reference_counters, reference_allocations, record_refs; bump ADAPTER_FORMAT) and MemoryEngine (testkit); register ProjectFiles projector in store/open.rs; include projects & contributing teams in OrgSnapshot; run engine-independent test suite on both Memory and SQLite (including restart & runtime projection rebuild).
3. Add org-proto ProjectRecord/ProjectList/ProjectInspect/ProjectCreate/ProjectAddTeam/ProjectRemoveTeam wire support + update org-office-server operations.json & handlers + wire /api/v1/organizations/{org}/projects (GET list, POST create), /api/v1/organizations/{org}/projects/{project} (GET inspect), and /api/v1/organizations/{org}/projects/{add-team,remove-team} (POST) in agentic-org-web.
4. Add org-command CLI rows (project list, project inspect, project create) and agentic-org project CLI + real-process HTTP & restart/rebuild E2E test + update FC-030 and FC-038 in doc-3 + fresh two-pass review.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered B3 projects minimum end-to-end: durable org-office projects family (P-<n> numbering, canonical slug derivation/reservation, atomic default owning-team creation under TEAM-1 or binding existing active team, contributing teams add/remove, archive_team active-project guard, cross-family ID collision checks, Memory==SQLite parity + rebuild), org-office-server project.create/list/inspect/add_team/remove_team, /api/v1/organizations/{org}/projects* web routes, agentic-org project list|inspect|create CLI, real-process E2E with projection rebuild, and updated FC-030/FC-038 in doc-3. Verified via cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, full cargo test --workspace (0 failures), real-process B3 E2E, and Pass 2 fresh read-only Rust review (VERDICT: APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
