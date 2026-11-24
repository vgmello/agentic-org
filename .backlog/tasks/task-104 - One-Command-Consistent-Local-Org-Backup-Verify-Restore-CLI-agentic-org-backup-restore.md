---
id: TASK-104
title: >-
  One-Command Consistent Local Org Backup, Verify & Restore CLI (agentic-org
  backup / restore)
status: To Do
assignee: []
created_date: '2026-11-24T04:23:00Z'
labels:
  - feature
  - backend
  - rust
  - storage
  - cli
  - security
  - recovery
  - docs
milestone: m-0
dependencies:
  - TASK-39
  - TASK-45
  - TASK-50
  - TASK-53
documentation:
  - .backlog/docs/organization-data/doc-72 - Organization-Data-—-README.md
  - >-
    .backlog/docs/organization-data/doc-91 -
    Organization-Data-16-—-Recovery-and-Office-Continuity.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
priority: medium
type: feature
ordinal: 119000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers a pre-dogfooding local HQ safety net (agentic-org backup / restore) that snapshots an organization's entire non-rebuildable <durable>/ state — including Org Git state/, durable/messaging/, budgets/, accounting/, recovery/, durable/blobs/ (forward-compatible with TASK-98.1), and WAL-mode durable/private/task-notes.db via SQLite VACUUM INTO — into a verifiable, optionally passphrase-encrypted .tar.gz archive using zero new external crates (reusing workspace tar, flate2, argon2, ring AES_256_GCM, zeroize, sha2). Strictly preserves doc-91 same-host writer-custody fencing; cross-machine HQ relocation (TASK-72) and automated backup scheduling (TASK-74 / FC-149) stay in v2.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Creates library crate crates/org-backup (keeping apps/agentic-org thin and org-host decoupled from org-office) implementing deterministic .tar.gz archiving of <org-id>/durable/ (state, messaging, recovery, authority, accounting, budgets, checkpoints, optional blobs, local/offices/<id> excluding enroll-tmp/agents/*/runtime/agents/*/tmp, and private/task-notes.db snapshotted cleanly via SQLite VACUUM INTO into a 0600 temp file), strictly excluding disposable <org-id>/runtime/ caches.
- [ ] #2 Embeds a versioned manifest.json inside each archive recording format version, org_id, office_id, journal_incarnation & head ordinal, created_at, and per-file SHA-256 digests; refuses live backup when office.lock / HostLock is held (failing closed with clear instructions to stop the office first).
- [ ] #3 Supports optional --encrypt and transparent encrypted-archive detection reusing existing workspace argon2 (64 MiB, t=3, p=4) + ring AES_256_GCM (domain AAD 'agentic-org/backup/v1', Zeroizing secrets, TTY or --passphrase-fd/--passphrase-file input only — never CLI args, 0600 atomic temp + full_sync + rename) with zero new workspace dependencies (ADR-0016 d6).
- [ ] #4 Wires 'agentic-org backup <org> --out <path> [--encrypt]', 'agentic-org restore --verify <archive> [--to <dir>]', and 'agentic-org restore <archive> --org <org>': --verify / --to <dir> validates AEAD/SHA-256 manifest, rejects path traversal (..)/absolute paths/symlinks/oversized entries, runs PRAGMA integrity_check on private/task-notes.db, and executes a full disposable org-office projection rebuild into a temp <runtime>/ dir; in-place restore requires stopped office, refuses silently resetting writer custody (failing closed per doc-91:169-171), and prints explicit scope notice for host-local custody and Keychain secrets key (doc-91:295).
- [ ] #5 Adds integration tests covering full org + messaging + budgets + accounting + WAL private/task-notes.db round-trip (plain + encrypted), corrupt-byte/wrong-passphrase/path-traversal/symlink refusal, held-lock refusal, and verified temp-dir projection rebuild; updates doc-91 and doc-3 (FC-147 progress + fix stale FC-053 path to <durable>/private/task-notes.db).
<!-- AC:END -->
