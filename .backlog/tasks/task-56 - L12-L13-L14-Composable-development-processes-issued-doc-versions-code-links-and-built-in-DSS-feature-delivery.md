---
id: TASK-56
title: >-
  L12, L13 & L14: Composable development processes, issued doc versions, code
  links, and built-in DSS/feature-delivery
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T14:30:00Z'
labels:
  - backend
  - rust
  - processes
  - dss
  - late-v1
milestone: m-0
dependencies:
  - TASK-41
  - TASK-42
  - TASK-51
  - TASK-47
  - TASK-55
documentation:
  - docs/implementation-plan.md
  - docs/design/development-processes.md
  - docs/design/desired-state-specifications.md
  - docs/specs/req-06-desired-state-spec-authoring-and-reconciliation.md
  - docs/specs/req-07-composable-process-packages-and-team-blueprints.md
priority: medium
type: feature
ordinal: 56000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Deliver composable development process packages (records, import/download, seed roster, process-declared project tabs, role-bound skills in instructions, agent-os process create|update for project scope; L12, FC-175..179, FC-184, FC-187), issued immutable document versions with diffs and code-link convergence (L13, FC-181..182), and the built-in dss and feature-delivery development processes validated with real agents (L14, FC-183, FC-185).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Development process records, import/download, seed roster derivation, process-declared project tabs, and skill injection pass PKG-1..18 and PPD-1..3 (FC-175..179, FC-184, FC-187).
- [ ] #2 Issued document versions under docs-history/, version diffs, code_links staleness, and task-realizes convergence pass DSS-3/4/7/8 and PPD-6..8/10 (FC-181..182).
- [ ] #3 Built-in feature-delivery and dss processes ship and pass end-to-end real-agent verification (DSS-1..12, FC-183, FC-185).
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (L12, L13 & L14):
1. Composable Process Packages (L12, FC-175..179, FC-184, FC-187): extend crates/org-process (model.rs, resolve.rs, skills.rs, write.rs) and crates/org-office/src/processes/ + office/processes.rs with development process package records, import/download, seed roster derivation, process-declared project tabs, role-bound skill injection in org-office-sessions/src/instructions.rs, and agent-os process create|update for project scope.
2. Issued Document Versions & Code Links (L13, FC-181..182): build immutable issued doc snapshots under docs-history/, version diffs, code_links staleness tracking against repo commits (integrating with crates/org-git / C7), and task-realizes convergence views.
3. Built-in Processes (L14, FC-183, FC-185): ship built-in feature-delivery and dss process packages and verify end-to-end with supervised sessions (DSS-1..12, PKG-1..18, PPD-1..10).
4. Verification: cargo test -p org-process -p org-docs -p org-office -p org-office-sessions -p org-office-server -p agentic-org-web; python3 -B -m unittest docs/design/organization-data/test_schema.py.
<!-- SECTION:PLAN:END -->
