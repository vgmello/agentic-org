---
id: TASK-10
title: >-
  Isolate private DMs and owner-only budgets from plaintext office.db and remove
  wrapped Org CA key from acceptance journal Entry 1
status: To Do
assignee: []
created_date: '2026-11-20T23:24:00Z'
labels:
  - security
  - architecture
  - runtime
  - p1
milestone: m-3
dependencies: []
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/adr/0016-no-application-level-data-encryption.md
  - docs/design/organization-data/15-messaging-and-chat.md
  - docs/design/organization-data/16-recovery-and-office-continuity.md
  - docs/design/organization-data/17-budgets-and-controls.md
priority: high
type: enhancement
ordinal: 10000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit findings H-05, H-06: Under ADR-0016, runtime/<office-id>/office.db is unencrypted SQLite that mixes public org/task tables with private human DMs, owner-only agent budgets/spend, and inline supervisor_command_receipts.response_json—collapsing the isolation model established by task-notes.db. Separately, Doc 16:229-243 commits the wrapped Org CA private key (ca-key.v1.json) into Entry 1 of durable/recovery/ alongside public state/ files, forcing permanent special-case outbox exclusions and retaining key ciphertext in recovery segments indefinitely.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Protected messaging (private DMs) and owner-only budget projections are isolated from shared plaintext office.db behind explicit access-controlled boundaries matching private/task-notes.db.
- [ ] #2 Inline supervisor_command_receipts.response_json is replaced with access-checked result_ref references.
- [ ] #3 Wrapped Org CA private key material is written exclusively to local security storage outside durable/recovery/ acceptance journal segments.
- [ ] #4 ADR-0016, Doc 15, Doc 16, Doc 17, and affected org-office/org-store tests reflect and enforce the updated boundaries.
<!-- AC:END -->
