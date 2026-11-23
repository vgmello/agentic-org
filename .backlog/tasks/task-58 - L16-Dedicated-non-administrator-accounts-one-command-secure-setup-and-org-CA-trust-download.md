---
id: TASK-58
title: >-
  L16: Dedicated non-administrator accounts, one-command secure setup, and org
  CA trust download
status: To Do
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T14:30:00Z'
labels:
  - backend
  - rust
  - security
  - linux
  - late-v1
milestone: m-0
dependencies:
  - TASK-57
documentation:
  - docs/implementation-plan.md
  - docs/design/accounts-sandbox-and-extensions.md
  - docs/adr/0015-agent-isolation-modes.md
priority: medium
type: feature
ordinal: 58000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Execute the accounts, launcher, security, secrets, and admin-rights spikes (accounts-sandbox-and-extensions §11), write follow-up ADRs (§10), and build hidden service account + per-office agent accounts (systemd template agentic-org-office@.service + polkit on Linux), --isolation off|accounts default-on setup with closed-list root helper, trusted organization CA install + 'Trust this organization' download (D26), agent graphical session viewing (D27), and ADR-0015 ISO-1..9 / ENC-4 dedicated-user mode (FC-161, FC-191, ASX-1..13).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Section 11 spikes (accounts/launcher, security, isolation setting, secrets, admin rights) are documented and section 10 ADRs are finalized.
- [ ] #2 One-command setup installs service and per-office agent accounts with --isolation accounts default and explicit --isolation off development choice (no silent fallback).
- [ ] #3 Organization CA is installed as trusted with a 'Trust this organization' download and fingerprint display (D26), and ISO-1..9 / ENC-4 / ASX-1..13 pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Backend Implementation Blueprint (L16 — Dedicated Accounts & Org CA Trust):
1. Section 11 Spikes & Section 10 ADRs: complete accounts/launcher, security, isolation setting, secrets, and admin-rights spikes from doc-61 and record ADRs.
2. One-command setup & isolation (crates/org-host, crates/org-platform, crates/org-platform-host, apps/agentic-org):
   - Hidden service account + per-office agent accounts (macOS + Linux agentic-org-office@.service template + polkit rule per D14..D23).
   - Default --isolation accounts with explicit --isolation off development choice (no silent fallback) and closed-list root helper.
   - Disk encryption check-and-warn; single confirmation for uninstall and isolation-mode switches.
3. Org CA trust (D26): install per-org CA as trusted, expose 'Trust this organization' CA download + SHA-256 fingerprint display in agentic-org-web, and wire agent graphical session viewing (D27).
4. Verification: ISO-1..9, ENC-4, ASX-1..13, FC-161, FC-191; cargo test -p org-host -p org-platform -p org-platform-host -p agentic-org-web.
<!-- SECTION:PLAN:END -->
