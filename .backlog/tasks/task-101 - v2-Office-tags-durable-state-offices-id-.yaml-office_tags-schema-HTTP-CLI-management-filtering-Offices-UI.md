---
id: TASK-101
title: >-
  v2: Office tags (durable state/offices/<id>.yaml, office_tags schema, HTTP/CLI
  management, filtering & Offices UI)
status: To Do
assignee: []
created_date: '2026-11-24T03:27:00Z'
labels:
  - feature
  - backend
  - rust
  - ui
  - cli
  - offices
  - data-contract
  - docs
milestone: m-1
dependencies:
  - TASK-52
  - TASK-85.3
documentation:
  - >-
    .backlog/docs/organization-data/doc-75 -
    Organization-Data-01-—-Organization-Structure.md
  - >-
    .backlog/docs/organization-data/doc-74 -
    Organization-Data-—-Schema-Coverage-and-Integrity-Review.md
  - .backlog/docs/specs/doc-44 - Spec-Offices-Teams-People-and-Org-Chart-UX.md
  - .backlog/docs/design/doc-71 - Design-Web-HTTP-API-v1.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
priority: medium
type: feature
ordinal: 116000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Enables organizations in v2 to assign normalized, human- and agent-readable tags to offices (e.g., macos, linux, gpu, xcode, us-east, build-runner) so owners, operators, and agents can organize, filter, and inspect offices by environment, hardware, region, or capability — laying a clean foundation for future tag-aware placement and routing. Tags are descriptive metadata persisted durably in HQ-owned state/offices/<office-id>.yaml (replicated automatically to satellites via TASK-52) and NEVER act as security or authorization grants.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Extends durable/state/offices/<office-id>.yaml (Office record in crates/org-office/src/organization/records.rs) with normalized, deduplicated tags[] (bounded e.g. <= 32 tags/office, lowercase slug/tag charset <= 48 chars) and adds STRICT table office_tags(organization_id, office_id, tag) + index (organization_id, tag, office_id) to docs/design/organization-data/schema.sql (documented inside existing doc-75 + doc-74 ENG-5, preserving test_schema.py exact 18-doc invariant), projected across both Memory and SQLite engines with lossless Delete+Rebuild and automatic HQ->satellite replication (TASK-52).
- [ ] #2 Adds HQ-authoritative office tag mutation RPC (owner/authorized admin only, Ack::Durable, optimistic --if-match revision check, additive/subtractive add_tags/remove_tags or replace semantics) and exposes tags + tag filtering on office list/inspect RPCs and Web HTTP API (/api/v1/organizations/{org}/offices, doc-71); ensures tags are never used as authorization grants (Non-Negotiable #9).
- [ ] #3 Exposes office tags in CLI (agent-os / agentic-org office list --tag <tag>, office inspect, and owner-only office tag update within TASK-85.3 4 KiB CATALOGUE_LIMIT headroom) and in the Browser Offices UI (apps/agentic-org-ui/src/lib/modules/offices/: display tag chips on office rows/detail, filter Offices list by tag, and add/remove tags in office settings/detail per doc-44).
- [ ] #4 Updates doc-75 (Mermaid ER + SQL + YAML contract), doc-74, doc-44, doc-71, doc-28, and doc-3; dual-engine Memory/SQLite + Delete+Rebuild, test_schema.py, engine_boundary.rs, cargo fmt/clippy/test, and UI checks pass.
<!-- AC:END -->
