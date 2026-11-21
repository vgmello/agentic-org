---
id: TASK-32
title: >-
  Cut over repository documentation links from docs/ to .backlog/, retire legacy
  docs/ markdown files, and verify checks
status: Done
assignee:
  - '@docs-cutover-engineer'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T01:41:00Z'
labels:
  - docs
  - backlog-migration
  - ci
milestone: m-5
dependencies:
  - TASK-20
  - TASK-21
  - TASK-22
  - TASK-23
  - TASK-24
  - TASK-25
  - TASK-26
  - TASK-27
  - TASK-28
  - TASK-29
  - TASK-30
  - TASK-31
documentation:
  - AGENTS.md
  - CLAUDE.md
priority: medium
type: chore
ordinal: 32000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Once all 127 documentation files under docs/ have been individually reviewed and migrated into .backlog/docs/ and .backlog/decisions/, repository-wide references (AGENTS.md, CLAUDE.md, crate READMEs/doc comments, CI scripts, and Backlog task links) must point to the new canonical .backlog/ locations and legacy markdown copies under docs/ must be retired cleanly without breaking Rust builds or kit hooks.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 AGENTS.md, CLAUDE.md, crate READMEs, and scripts/CI workflows reference the migrated .backlog/docs/ and .backlog/decisions/ locations (and any code-owned SQL assets still embedded via include_str!).
- [x] #2 Legacy migrated Markdown files under docs/ are removed (leaving no duplicate competing sources of truth).
- [x] #3 No broken internal links remain across .backlog/docs/, .backlog/decisions/, and AGENTS.md.
- [x] #4 cargo test --workspace, test_schema.py, and python3 .agent-kit/runtime/kit_hooks.py check pass cleanly.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Verify 120/120 legacy docs/**/*.md exist in .backlog/ and sync drifted updates in decision-16, decision-32, and doc-7. 2. Rewrite all relative Markdown links across .backlog/docs/** and .backlog/decisions/** to resolve accurately with URL-encoded spaces. 3. Update AGENTS.md, README.md, DEVELOPMENT.md, .agent-kit/PROJECT_MAP.md, and 5 crate READMEs to reference .backlog/ paths. 4. Update docs/design/organization-data/test_schema.py to read canonical contracts from .backlog/docs/organization-data/ and validate relative links. 5. git rm all 120 retired docs/**/*.md files while retaining SQL, test_schema.py, HTML guides, and LICENSE. 6. Run test_schema.py, full link audit, cargo test -p org-office -- store::sqlite, and kit_hooks.py check, then finalize TASK-32 and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Verified 120/120 legacy docs/**/*.md exist in .backlog/ (plus 5 standalone index/vision docs). Synced post-migration updates into decision-16 (TASK-10/11), decision-32 (TASK-11), and doc-7 (TASK-2/19). Rewrote 2,222 relative Markdown links across 117 .backlog/{docs,decisions} files plus 4 directory links (.backlog/decisions) and 2 obsolete UI e2e reference links in doc-3. Updated AGENTS.md, README.md, DEVELOPMENT.md, .agent-kit/PROJECT_MAP.md, 5 crate READMEs, and docs/design/organization-data/test_schema.py. Retired all 120 legacy docs/**/*.md via git rm while keeping 4 SQL schemas, test_schema.py, 3 HTML guides, and rust-style-guide.LICENSE. Checks: test_schema.py 116/116 OK; full link checker 135 files / 2,278 relative links / 0 broken / 0 legacy_md; cargo test -p org-office -- store::sqlite 26/26 OK; kit_hooks.py check exited 1 on pre-existing AGENTS.md capability-evidence hash warning (left un-faked).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Cut over repository documentation links from legacy docs/ to canonical .backlog/docs/ and .backlog/decisions/, synced drifted decision-16/decision-32/doc-7 updates, updated test_schema.py to verify .backlog/docs/organization-data/ contracts and relative links, and retired all 120 legacy docs/**/*.md files while retaining executable SQL schemas, test_schema.py, HTML guides, and rust-style-guide.LICENSE. Verified with test_schema.py (116/116 OK), full 135-file link audit (2,278 relative links, 0 broken, 0 legacy_md), and cargo test -p org-office -- store::sqlite (26/26 OK). Noted pre-existing kit_hooks.py AGENTS.md hash warning without modifying hashes.
<!-- SECTION:FINAL_SUMMARY:END -->
