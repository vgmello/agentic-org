---
id: TASK-35
title: >-
  A5 & B2.4: Person CLI sign-in, selected-office addressing, and organization
  CLI rows
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
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
- [ ] #1 agentic-org sign-in prints a browser approval link, issues host grants without terminal passwords, and enforces explicit or saved instance+office context (SES-8).
- [ ] #2 Organization CLI rows (org inspect, team list|inspect|create|update|move|archive, person inspect|add|update|move|deactivate, agent inspect) pass K1 contract tests and CLI-4/5/6 against a real office.
- [ ] #3 agent-os addresses selected offices by UUIDv7 socket path and refuses unauthenticated or unknown-office invocations immediately.
- [ ] #4 Real-host FC-146 checks pass: delete runtime/<office-id>/ after team.create rebuilds identically and replays command_id, and journal corruption/rollback/closed custody fail closed.
<!-- AC:END -->
