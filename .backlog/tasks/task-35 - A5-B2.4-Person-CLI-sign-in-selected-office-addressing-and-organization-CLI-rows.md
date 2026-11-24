---
id: TASK-35
title: >-
  A5 & B2.4: Person CLI sign-in, selected-office addressing, and organization
  CLI rows
status: Done
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T09:34:00Z'
labels:
  - backend
  - rust
  - cli
  - m1
milestone: m-0
dependencies:
  - TASK-15
  - TASK-34
documentation:
  - docs/implementation-plan.md
  - docs/design/agent-os-command-line.md
  - .agent-kit/slices/b2-organization-teams-people-plan.md
priority: high
type: feature
ordinal: 35000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Deliver person-started CLI sign-in approved in the browser (FC-173 / SIGN-40 / SES-8) over host.sock with saved instance+office context, update org-client/agent-os to address selected UUIDv7 office sockets, add B2.4 organization rows (org inspect, team *, person *, agent inspect) with --dry-run, --idempotency-key, --if-match, and --durable, and restore B1b's two real-host custody/rebuild checks (FC-146).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 agentic-org sign-in prints a browser approval link, issues host grants without terminal passwords, and enforces explicit or saved instance+office context (SES-8).
- [x] #2 Organization CLI rows (org inspect, team list|inspect|create|update|move|archive, person inspect|add|update|move|deactivate, agent inspect) pass K1 contract tests and CLI-4/5/6 against a real office.
- [x] #3 agent-os addresses selected offices by UUIDv7 socket path and refuses unauthenticated or unknown-office invocations immediately.
- [x] #4 Real-host FC-146 checks pass: delete runtime/<office-id>/ after team.create rebuilds identically and replays command_id, and journal corruption/rollback/closed custody fail closed.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Selected-office addressing & org CLI rows (B2.4, CLI-4/5/6): update org-client/org-agent-cli/agent-os to resolve selected UUIDv7 office sockets (AGENT_OS_OFFICE/AGENT_OS_OFFICE_HOME/AGENT_OS_SOCKET) and refuse unauthenticated or unknown-office calls immediately; add complete B2.4 organization rows (org inspect, team list|inspect|create|update|move|archive, person inspect|add|update|move|deactivate, agent inspect, plus project list|inspect|create) to org-agent-cli with --dry-run, --idempotency-key, --if-match, and --durable; verify K1 contract checks and CLI-4/5/6 against a real office. 2. Browser-approved CLI sign-in & saved context (A5, FC-173, SIGN-40, SES-8): add host approval-link ops (cli.approval.create / poll / decide) in org-host-control + org-host (10-min expiry, one-time digest-only secret, approve/decline/expire, binds terminal session without password), web routes (/api/v1/cli-approvals/{id}/decide) in agentic-org-web, and agentic-org sign-in + context save/select in org-host/agentic-org. 3. Real-host FC-146 custody & rebuild checks: prove through real host + office that (a) deleting runtime/<office-id>/ after team.create rebuilds identically and replays command_id, and (b) flipped journal byte at/below confirmed head, older segment rollback behind host checkpoint, and closed custody channel fail closed. 4. Update doc-3 (FC-173, FC-146, FC-168) and pass fresh two-pass Rust review.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered A5 & B2.4 (FC-173, FC-168, FC-146): browser-approved CLI sign-in over host.sock (cli.approval.create/poll, browser.cli_approval_decide, POST /api/v1/cli-approvals/decide, digest-only 10-min single-use connection-bound tokens, SES-8 revocation on drop, owner-only 0o600 non-secret cli-context.json), selected UUIDv7 office addressing with fail-closed hello verification in org-client/agent-os, 16-row B2.4/B3 org & project org-command table in org-agent-cli with CLI-4/5/6 and --durable, and real-process E2E proofs through real org-host + agentic-org-web + agentic-org-office covering browser-approved sign-in, runtime deletion rebuild + exact command_id replay + post-rebuild operation_reused, and journal rollback/corruption/closed-custody fail-closed. Passed full cargo fmt/clippy/test --workspace, 3/3 real-process E2E tests, and separate fresh final Rust review (VERDICT: APPROVE).
<!-- SECTION:FINAL_SUMMARY:END -->
