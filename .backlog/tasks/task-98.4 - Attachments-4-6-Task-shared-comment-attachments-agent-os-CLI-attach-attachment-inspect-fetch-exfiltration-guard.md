---
id: TASK-98.4
title: >-
  Attachments 4/6: Task & shared comment attachments + agent-os CLI (--attach,
  attachment inspect/fetch, exfiltration guard)
status: To Do
assignee: []
created_date: '2026-11-24T01:57:00Z'
labels:
  - backend
  - rust
  - tasks
  - cli
  - attachments
  - ax
  - security
  - docs
milestone: m-0
dependencies:
  - TASK-98.1
  - TASK-85.3
  - TASK-87
  - TASK-90
documentation:
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
  - >-
    .backlog/docs/organization-data/doc-77 -
    Organization-Data-03-—-Features-and-Work.md
  - >-
    .backlog/docs/organization-data/doc-79 -
    Organization-Data-05-—-Operational-Data-and-Private-Notes.md
parent_task_id: TASK-98
priority: high
type: feature
ordinal: 110000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Adds image attachments to tasks (tasks, bugs, feature requests) and shared task comments persisted in task.md/comments/<id>.md front matter, and enables agents to send and fetch authorized images via agent-os CLI with strict two-layer exfiltration protection.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Persists attachments[] metadata in task.md (max 20) and comments/<id>.md (max 10) front matter, projects into attachments table across SqliteEngine and MemoryEngine (ADR-0018, lossless Delete+Rebuild), wires tasks.create, tasks.update (add_attachments/remove_attachments with --if-match & ConflictDetail changed_sections), tasks.inspect (prunable via --fields per TASK-90), and tasks.comment (including wake notifications carrying comment attachment_ids).
- [ ] #2 Adds repeatable --attach <path> and --attachment-id <id> flags on agent-os msg send|reply|ask and task create|update|comment (--add-attachment/--remove-attachment) plus 2 compact verbs (attachment inspect, attachment fetch [--output <path>]) within CATALOGUE_LIMIT = 4096 (building on TASK-85.3), with zero latency overhead when --attach is omitted (ADR-0014).
- [ ] #3 Implements two-layer --attach exfiltration defense: org-office-server NEVER opens agent-supplied paths (org-agent-cli reads under the agent OS user and streams 512 KiB chunks); org-agent-cli enforces canonicalized allowed-root confinement (workspace, own private_home, own active worktrees, session-scoped TMPDIR — never global /tmp), O_NOFOLLOW, S_ISREG, <= 10 MiB bounded read, and magic-byte pre-check, backed by negative tests (/etc/passwd, ../ traversal, escaping symlinks, durable/blobs/, durable/private/task-notes.db, another agent's private_home).
- [ ] #4 Documents --attach and attachment fetch in built-in skills (crates/org-process/src/skills.rs, verified by verify_command_references), doc-28 (§2.11 slow-path note), doc-77, doc-79, and doc-96.
<!-- AC:END -->
