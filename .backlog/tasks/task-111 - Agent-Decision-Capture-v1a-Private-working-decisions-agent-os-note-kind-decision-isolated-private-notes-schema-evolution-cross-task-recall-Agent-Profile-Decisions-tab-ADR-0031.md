---
id: TASK-111
title: >-
  Agent Decision Capture (v1a): Private working decisions (agent-os note --kind
  decision), isolated private-notes-schema evolution, cross-task recall & Agent
  Profile Decisions tab (ADR-0031)
status: To Do
assignee: []
created_date: '2026-11-24T17:07:00Z'
labels:
  - backend
  - rust
  - cli
  - ax
  - ui
  - ux
  - agents
  - decisions
  - privacy
milestone: m-0
dependencies:
  - TASK-50
  - TASK-67
  - TASK-85.3
documentation:
  - docs/design/organization-data/private-notes-schema.sql
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
  - >-
    .backlog/decisions/decision-31 -
    ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md
  - .backlog/docs/specs/doc-45 - Spec-Hiring-Profiles-and-Team-Moves-UX.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
priority: high
type: feature
ordinal: 126000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Delivers Tier 1 of Agentic Org's visibility-truthful Two-Level Decision Capture architecture without duplicating shared ADRs (documents.kind='decision', TASK-55) or violating ADR-0031 / doc-79:530 (zero mutable visibility flags). Evolves the shipped HQ-only isolated private store (private-notes-schema.sql) in place (pre-v1 clean replacement per doc-7 #14: private_notes in <durable-root>/private/notes.db) to capture lightweight private working decisions (kind IN ('note','plan','decision'), nullable task_id for non-task turns, optional auto-inferred project_id, auto-derived title, optional why & alternatives, normalized private_note_tags with task_labels grammar, and author-kind/tag indexes). Generalizes the agent-os CLI from task-note to agent-os note (keeping task-note and task note as pre-normalized AX-8 aliases; get -> inspect; 1 required content flag --text/--input-file/--stdin for low Turn-15 friction; auto-bound --task; cross-task paginated note list with server-side --fields pruning; +0 net resources in the 4 KiB CATALOGUE_LIMIT) and adds a read-only people-facing Decisions tab on Agent Profiles (/members/[memberId]/decisions) cleanly separating private working decisions from shared project ADRs.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 ADEC-1 Schema & Dual-Engine Store: Evolves docs/design/organization-data/private-notes-schema.sql (clean pre-v1 private_notes + private_note_tags tables in <durable-root>/private/notes.db; nullable task_id with SQLite MATCH SIMPLE FK to private_note_tasks; nullable project_id; kind IN ('note','plan','decision') author-editable on update; title auto-derived from first non-empty line <=120 chars when omitted and required non-null for active decisions <=200 chars; optional why & alternatives <=2000 chars; normalized private_note_tags <=8 tags per note matching [a-z0-9._:/-] grammar; private_notes_by_author_kind and private_note_tags_by_tag indexes; atomic tombstone clearing title/why/alternatives/body_markdown to NULL and deleting private_note_tags per PN-06; tasks.notes.publish rejects task_id IS NULL). Updates PrivateNotesStore (Sqlite + Memory + SatelliteRefused) in lockstep with ENG-3 conformance, doc-79 (Doc 05), doc-74, test_schema.py (preserving 18-doc invariant & zero private_% in schema.sql), engine_boundary.rs, and records an ADR extending ADR-0031.
- [ ] #2 ADEC-2 Strict ADR-0031 Privacy Isolation: Private notes/decisions are readable only by their author and active people, and writable/deletable only by their author; other agents receive empty lists / NotFound regardless of team leadership or task reassignment. Zero private titles, why, alternatives, bodies, tags, or IDs appear in schema.sql, office.db, durable/state/, Git, shared Activity (LIVE-2), LiveEvents, other agents' turn context, or Rust Debug logs (redacted on PrivateNoteRecord/wire structs). Satellites forward authenticated RPCs to HQ when online and fail closed (Unavailable) when offline.
- [ ] #3 ADEC-3 Low-Friction CLI & Turn-40 Recall: Upgrades agent-os task-note -> agent-os note (add|list|inspect|update|remove|publish) with task-note and task note pre-normalized aliases (AX-8) and 0 net new resources in the 4 KiB catalogue. note add requires only --text (or --input-file/--stdin), accepts optional --kind note|plan|decision, --title, --why, --alternatives, repeatable --tag, --task (auto-bound from active run, nullable for non-task turns, --standalone override), --project (auto-inferred); omits --private/--shared and --author on write. note list supports cross-task filtering (--task, --project, --kind, --tag, --since, people-only --author), --count, --limit/--cursor, and server-side --fields pruning (excluding full body_markdown by default/when unrequested) for <1 KiB Turn-40 recall. Adds runnable Command.example (AX-7) and matching agentic-org person CLI parity.
- [ ] #4 ADEC-4 Agent Profile Decisions Tab & Task Pane Upgrade: Adds GET /api/v1/organizations/{org}/members/{member_id}/decisions and private-notes routes in agentic-org-web, plus a new Decisions tab on Agent Profiles (/members/[memberId]/decisions, updating doc-45 §5 and tabs.ts; keeping Notes and memory untouched per doc-45:124 and Activity free of private data). Displays two clearly separated, badged sections: (1) Working decisions (Private) — read-only with lock badge ('Visible to people and <agent>; hidden from other agents'), showing Title, Why, Alternatives, Tags, linked Task pill, and tag/project/task filters; (2) Project decisions (Shared ADRs) — honest empty/placeholder state until TASK-55/TASK-112 lands. Upgrades PrivateNotesPane.svelte (TASK-67) with Decision/Plan/Note badges, Why/Alternatives/Tags display, and kind filter.
- [ ] #5 ADEC-5 Anti-Spam Tier-1/Tier-3 AX Guidance: Adds the 2-sentence visibility & anti-spam rule (+~140 net bytes inside the 10 KiB cap) to standing instructions (crates/org-office-sessions/src/instructions.rs) and the concrete 4-Signal Decision Bar (non-obvious trade-off, shortcut/scope cut --tag shortcut, unverified assumption/timeout default --tag assumption, costly-to-reverse cross-turn constraint) + explicit 'Do NOT record' list (0-3 decisions/task) + compact Turn-40 recall recipe to tasks-execute and pre-done review check to tasks-finalize in crates/org-process/src/skills.rs, verified by org_command::parse drift guards.
<!-- AC:END -->
