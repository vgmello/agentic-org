---
id: TASK-112
title: >-
  Agent Decision Promotion (v1b): 2-phase agent-os note promote to shared
  project ADRs, docs-and-decisions skill & Agent Profile shared decisions
  section
status: To Do
assignee: []
created_date: '2026-11-24T17:07:00Z'
updated_date: '2026-11-25T00:15:00Z'
labels:
  - backend
  - rust
  - cli
  - ax
  - ui
  - docs
  - decisions
  - privacy
milestone: m-0
dependencies:
  - TASK-111
  - TASK-55
  - TASK-68
documentation:
  - docs/design/organization-data/private-notes-schema.sql
  - .backlog/docs/specs/doc-52 - Spec-Knowledge-Decisions-and-Tech-Debt-UX.md
  - .backlog/docs/specs/doc-45 - Spec-Hiring-Profiles-and-Team-Moves-UX.md
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - >-
    .backlog/docs/design/doc-98 -
    Design-Two-Tier-Agent-Decision-Capture-and-Promotion.md
priority: medium
type: feature
ordinal: 127000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers Tier 2 bridging for Agent Decision Capture once Project Docs (TASK-55 / TASK-68) land: lets an agent or person promote a private working decision into a canonical shared project/feature ADR (documents.kind='decision' in org Git) in one idempotent, crash-safe command (agent-os note promote) with zero private provenance leaked into shared Git, populates the Shared Project Decisions section on Agent Profiles (/members/[memberId]/decisions), and ships the docs-and-decisions built-in AX skill.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 ADEC-P1 2-Phase Crash-Safe Promotion & Privacy Firewall: Extends private_note_command_receipts with command='promote', nullable shared_document_id (mutually exclusive with shared_comment_id, unique partial index, one pending publication guard), and private-side promoted_document_id on private_notes (visible only to author + people). Implements begin_promote/finish_promote on PrivateNotesStore (Sqlite + Memory) and office RPC private_notes.promote: creates a canonical shared documents(kind='decision') record via TASK-55 with zero-ceremony defaults (inferred --project, auto-slugged --path under the active process's decision section via sections[].kinds, caller owner, ADR body prefilled with ## Context <- why, ## Options considered <- alternatives, ## Decision <- body_markdown, plus public task relates_to Why-link if task_id present). Enforces ADR-0031 & PN-05: shared Git NEVER records private_note_id or private provenance, the private original stays private, and out-of-scope projects fail closed (other_project, doc-86:466).
- [ ] #2 ADEC-P2 CLI & Task/Profile UI Wiring: Adds agent-os note promote --id <id> --if-match <rev> [--project <ref>] [--feature <id>] [--title <text>] [--path <path>] [--key ADR-xxxx] [--status draft|current] (+~9 B in grouped 4 KiB catalogue) and matching HTTP route + UI 'Promote to project decision...' action for own private decisions in PrivateNotesPane.svelte and Agent Profile Decisions tab (showing 'Promoted -> <key/title>' badge privately to avoid duplicate ADRs). Populates the 'Project decisions (Shared ADRs)' section on /members/[memberId]/decisions from live TASK-55 documents(kind='decision') authored/owned by the member.
- [ ] #3 ADEC-P3 Built-in docs-and-decisions Skill & Closure Hook: Ships the concise on-demand docs-and-decisions skill in crates/org-process/src/skills.rs covering doc create --kind decision, note promote, Why links, and other_project boundaries, and wires tasks-finalize's pre-done check to promote only cross-task/architectural working decisions (verified by org_command::parse drift checks). Updates doc-52 and doc-96.
<!-- AC:END -->
