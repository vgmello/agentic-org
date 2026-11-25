---
id: TASK-55
title: >-
  L7 & L11: Project documentation tree/index, canonical features, knowledge
  library, releases, and flags
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-25T00:16:00Z'
labels:
  - backend
  - rust
  - docs
  - late-v1
milestone: m-0
dependencies:
  - TASK-49
  - TASK-85.3
  - TASK-91
documentation:
  - docs/implementation-plan.md
  - docs/design/organization-data/03-features-and-work.md
  - docs/design/organization-data/07-release-deployments.md
  - docs/design/organization-data/09-release-content-and-flags.md
  - >-
    docs/design/organization-data/18-development-processes-and-project-documentation.md
  - .backlog/docs/specs/doc-52 - Spec-Knowledge-Decisions-and-Tech-Debt-UX.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - >-
    .backlog/docs/design/doc-99 -
    Design-Dynamic-Documentation-Processes-and-Unified-Search.md
priority: medium
type: feature
ordinal: 55000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Build the project documentation tree under projects/<id>/docs/ with office-maintained index.yaml and agent-os doc commands scoped to an agent's projects (L11, FC-180), plus canonical feature definitions, knowledge kinds/templates/search, shortcut provenance, releases, changelogs, deployment records, and flag definitions/snapshots (L7, FC-058..070).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 projects/<id>/docs/ tree, media assets, index.yaml regeneration, and agent-os doc index|ls|show|new|edit|move|put pass PKG-11/12/14 and PPD-4/5/9 (FC-180).
- [ ] #2 Canonical features, knowledge library/search, Why links, releases, changelogs, deployments, and flag snapshots pass persistence and API tests (FC-058..070).
- [ ] #3 Agents cannot read or write documentation trees outside their assigned projects.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (L7 & L11):
1. Project Documentation Tree (L11, FC-180): extend crates/org-docs and crates/org-office/src/projects/ to manage projects/<id>/docs/, media assets, office-maintained index.yaml regeneration, and project-membership-scoped read/write boundaries (agents cannot read/write docs outside assigned projects).
2. agent-os doc CLI: add agent-os doc index|ls|show|new|edit|move|put in crates/org-agent-cli + apps/agent-os + org-office-server/src/serve/host/operations.json (keep K1 catalogue <= 4096 bytes).
3. Canonical Features, Knowledge, Releases & Flags (L7, FC-058..070): add org-proto types, Memory + SQLite store ports in crates/org-office, office RPCs, and /api/v1 routes in apps/agentic-org-web for canonical features, knowledge kinds/templates/search, shortcut/Why provenance, releases, changelogs, deployment records, and flag definitions/snapshots.
4. Cross-feature wiring follow-up from TASK-53: once release->project ownership exists, wire PM release-budget authorization into crates/org-office/src/office/budgets.rs (currently owner-only for release budgets until release catalog lands).
5. Verification: cargo test -p org-docs -p org-office -p org-office-server -p org-agent-cli -p agentic-org-web; python3 -B -m unittest docs/design/organization-data/test_schema.py; PKG-11/12/14, PPD-4/5/9, FC-058..070, FC-180.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Principal Designer & Search Architect audit (2026-11-24): (1) Fix live PKG-1 violation in apps/agentic-org-ui/src/lib/modules/docs/present.ts:91-92 (remove hardcoded product-dss/technical-dss branches; resolve default section dynamically from overview.sections[].kinds); (2) Harmonize agent-os doc CLI verbs to doc-96 §2.1 / doc-32 §8 canonical names (doc inspect|create|update instead of show|new|edit; update doc-52 AUTH-5) within TASK-85.3 4 KiB catalogue headroom; (3) Upgrade DocsTab.svelte to desktop split tree+reader (>=1024px, stacked list->detail with back link <1024px) showing index.yaml freshness and calm 'Indexed for agents (doc index)' indicator; (4) Replace @lucide/svelte imports in DocsTab/DocumentPage with Heroicons (SHELL-32) and replace amber tone.owner on Unissued changes / stale links with sky info / red danger tones (SHELL-31); (5) Register project documents/versions into TASK-91 SearchRead with strict project-scoped visibility for agents (doc-86).

2026-11-24 Agent Decision Capture AX alignment (TASK-111/TASK-112): Ensure agent-os doc create --kind decision provides zero-ceremony defaults so agents never hand-author YAML front matter or look up UUIDs — default --project from active binding, derive --path from the active process's decision section (overview.sections[].kinds) + slugified title, default owner_employee_id to bound caller, auto-derive 1-line summary if omitted, and support --link-task <ref> for public Why-links.
<!-- SECTION:NOTES:END -->
