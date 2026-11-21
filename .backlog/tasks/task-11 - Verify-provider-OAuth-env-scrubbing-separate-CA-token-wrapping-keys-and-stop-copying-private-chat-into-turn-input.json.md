---
id: TASK-11
title: >-
  Verify provider OAuth env scrubbing, separate CA/token wrapping keys, and stop
  copying private chat into turn/input.json
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - security
  - runtime
  - p1
milestone: m-3
dependencies:
  - TASK-2
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md
  - docs/adr/0016-no-application-level-data-encryption.md
  - docs/design/organization-data/11-model-profiles-and-agent-execution.md
priority: high
type: enhancement
ordinal: 11000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit finding H-07: ADR-0032 passes a 1-year CLAUDE_CODE_OAUTH_TOKEN via child environment variables relying on unverified CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1 behavior (and Codex refresh relies on unverified experimentalApi), Org CA keys and provider tokens share a single per-office wrapping key, and supervision/turns/<id>/input.json copies private chat in plaintext outside durable/messaging/ with no retention or purge policy.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Empirical verification (automated test or documented spike on target Claude Code and Codex CLI builds) proves child tool subprocesses cannot read provider OAuth credentials, or authentication is moved to an out-of-process proxy before C2 wiring.
- [ ] #2 Org CA key wrapping and provider OAuth token wrapping use distinct wrapping keys.
- [ ] #3 Turn execution inputs reference protected chat messages or enforce explicit redaction/purge rather than retaining permanent unredacted plaintext shadow copies outside durable/messaging/.
<!-- AC:END -->
