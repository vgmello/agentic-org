---
id: TASK-17
title: >-
  Migrate legacy UI folders into src/lib/modules/, move domain dialogs out of
  components/, and align proto.ts with org-proto
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - ui
  - architecture
  - p2
milestone: m-4
dependencies:
  - TASK-12
  - TASK-14
references:
  - apps/agentic-org-ui/src/lib/proto.ts
  - apps/agentic-org-ui/eslint-suppressions.json
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/design/project-structure.md
  - docs/architecture-rules.md
priority: medium
type: enhancement
ordinal: 17000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit finding M-11: Legacy apps/agentic-org-ui/src/lib/{org,work,office,offices}/ coexist beside src/lib/modules/, 16 domain dialogs/sheets sit in shared src/lib/components/, platform/http/events.ts is missing, eslint-suppressions.json has grown to 210 lines, and hand-mirrored proto.ts still retains deleted memberships: Membership[] shapes locked in by mocked tests.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Legacy src/lib/{org,work,office,offices}/ modules are migrated into src/lib/modules/ (including modules/work/) per project-structure.md §4.
- [ ] #2 Domain-specific dialogs and sheets are moved out of shared src/lib/components/ into their owning feature modules.
- [ ] #3 TypeScript wire contracts in proto.ts match current org-proto shapes (deleting legacy memberships: Membership[]) and are protected by shared test vectors or codegen.
- [ ] #4 platform/http/events.ts implements the SSE live-event client, and UI type check, lint, unit tests, and Playwright checks pass.
<!-- AC:END -->
