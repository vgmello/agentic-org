---
id: TASK-43
title: 'C4: Session inspection and read-only conversation log viewer (REQ-05)'
status: In Progress
assignee:
  - '@lead'
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-22T05:48:00Z'
labels:
  - backend
  - ui
  - agents
  - m1
milestone: m-0
dependencies:
  - TASK-42
documentation:
  - docs/implementation-plan.md
  - docs/specs/req-05-virtual-console-conversation-log-viewer.md
priority: medium
type: feature
ordinal: 43000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement office-side parsing and redaction of Claude and Codex conversation files (including sub-agent transcripts nested under their starting turn per SUB-6) and connect the browser read-only conversation log viewer with tool-call and file-change toggles (FC-150).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Authorized users can open an agent's live or past session in the browser and toggle tool calls and file changes.
- [x] #2 Sub-agent transcripts render under the turn that launched them (SUB-6) and secrets/credentials are redacted before leaving the office.
- [ ] #3 REQ-05 acceptance criteria pass and FEATURES row FC-150 is updated.
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
C4 backend complete & Neon Pass 3 VERDICT: APPROVE (crates/org-office-sessions/src/transcript/{mod,redact,claude,codex,agy,tests}.rs, org-proto member.session_list/member.session_transcript, org-office-server serve.rs+operations.json, agentic-org-web /agents/{id}/{sessions,transcript,sessions/{sid}/transcript}, doc-3 FC-150/FC-172). Remaining: browser UI console viewer (AC #1/#3).
<!-- SECTION:NOTES:END -->
