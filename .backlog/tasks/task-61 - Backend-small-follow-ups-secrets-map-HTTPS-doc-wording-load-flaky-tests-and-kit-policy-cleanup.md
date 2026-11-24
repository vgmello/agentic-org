---
id: TASK-61
title: >-
  Backend small follow-ups: secrets map, HTTPS doc wording, load-flaky tests,
  and kit policy cleanup
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T14:30:00Z'
labels:
  - backend
  - docs
  - hygiene
milestone: m-0
dependencies:
  - TASK-1
documentation:
  - docs/workstreams/backend.md
  - docs/design/accounts-sandbox-and-extensions.md
priority: low
type: chore
ordinal: 61000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Address small non-feature follow-ups tracked in docs/workstreams/backend.md: produce the secrets map (document + visual custody reference once secrets settle), update docs/design/office-host-boundary.md HTTPS wording, investigate/stabilize load-flaky tests (a_creation_retried_after_a_crash_point_keeps_its_ca_and_its_one_entry and an_admission_that_could_not_be_built_is_asked_for_again), clean up stale kit manifest hash and v1-integration entry in .agent-kit/policy/review-sandbox.json, and reconcile model-routing.json with owner policy.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Secrets map documents every secret, holder, storage location, and readers.
- [ ] #2 office-host-boundary.md reflects implemented X7 HTTPS support and kit policy/sandbox config stale entries are reconciled with owner confirmation.
- [ ] #3 Investigated load-flaky receipt/admission tests have root-cause fixes or deterministic regression tests.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Hygiene Blueprint (TASK-61):
1. Secrets map: author canonical secrets map doc (every secret, holder, storage location, wrapping key/domain, and readers — including TASK-53 protected DBs, TASK-54 provider bindings, and TASK-79 usage-credential.v1.json).
2. Docs & kit cleanup: update docs/design/office-host-boundary.md to reflect implemented X7 HTTPS support; reconcile stale kit manifest hash, v1-integration ref in .agent-kit/policy/review-sandbox.json, and model-routing.json with owner policy.
3. Flaky test root-cause fixes: stabilize a_creation_retried_after_a_crash_point_keeps_its_ca_and_its_one_entry (crates/org-office) and an_admission_that_could_not_be_built_is_asked_for_again (crates/org-host) under heavy parallel load.
<!-- SECTION:PLAN:END -->
