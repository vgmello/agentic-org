---
id: TASK-17
title: >-
  Migrate legacy UI folders into src/lib/modules/, move domain dialogs out of
  components/, and align proto.ts with org-proto
status: Done
assignee:
  - '@ui-subagent'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T03:58:00Z'
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
- [x] #1 Legacy src/lib/{org,work,office,offices}/ modules are migrated into src/lib/modules/ (including modules/work/) per project-structure.md §4.
- [x] #2 Domain-specific dialogs and sheets are moved out of shared src/lib/components/ into their owning feature modules.
- [x] #3 TypeScript wire contracts in proto.ts match current org-proto shapes (deleting legacy memberships: Membership[]) and are protected by shared test vectors or codegen.
- [x] #4 platform/http/events.ts implements the SSE live-event client, and UI type check, lint, unit tests, and Playwright checks pass.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Migrated src/lib/{org,work,office,offices}/ into src/lib/modules/{organization,work,offices}/ (and wire-shapes.test.ts to src/lib/), moved 17 domain dialogs/sheets/panels out of src/lib/components/ into owning feature modules, moved toAppError to src/lib/proto.ts (eliminating agent-setup -> organization -> offices -> employees -> members cycle), deleted Membership / OrgSnapshot.memberships / retired team.add_member & team.remove_member across proto/model/reducer/actions/sample/mock/components/fakes/tests, implemented POST /api/v1/events/stream SSE client in src/lib/platform/http/events.ts (+10 unit tests in events.test.ts), updated e2e/gate-manifest.json and eslint-suppressions.json. Verified format:check, lint, check, test (143 files / 2415 tests), build (productionImportGuard error mode + check-build-assets), and Playwright production-unavailable (6/6).

Review repair pass (6/6 findings resolved): (1) Rewrote all remaining 72 toAppError imports across src/ and e2e/ to $lib/proto and removed toAppError re-exports from modules/offices/office-api.ts and e2e/fakes/office-api.ts (verified 0 cycles across all 23 modules and 0 office-api imports in components/employees/members/organization/platform/shell); (2) Fixed splitSseLines CRLF handling on empty chunks; (3) Rejected outbound 'unknown' feed before dispatch and narrowed Subscription['feed']; (4) Hardened SSE parser with event: filtering (empty or 'frame' only), UTF-8 byte-length accounting, and bounded readErrorPayload; (5) Preserved WireDecodeError/EventStreamError/SseLimitError and handler exceptions even when aborted; (6) Updated stale office/mock.ts comments in e2e/mock-organization.ts. Re-verified format:check (773/0), lint (0/0), check (0/0), test (Test Files 143 passed (143); Tests 2417 passed (2417); 9 tests in events.test.ts), build (4 CSS, 156 JS), and Playwright production-unavailable (6/6).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Completed TASK-17 on branch refactor/task-17-ui-modules-and-sse: migrated legacy src/lib/{org,work,office,offices}/ into src/lib/modules/, relocated 17 domain components out of shared src/lib/components/, aligned proto.ts and UI organization state with single-home-team org-proto contracts (removing Membership, OrgSnapshot.memberships, team.add_member, team.remove_member), and added typed SSE live-event client at src/lib/platform/http/events.ts with unit tests. All UI gates (format:check, lint, check, 2415 unit tests, build, and Playwright production-unavailable 6/6) pass.
<!-- SECTION:FINAL_SUMMARY:END -->
