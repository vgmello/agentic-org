---
id: TASK-93
title: >-
  v2: First-Class Research Workflows — hybrid local/org/code & opt-in web
  research with resumable research_id session checkpoints and Lethal-Trifecta
  air-gap
status: To Do
assignee: []
created_date: '2026-11-23T03:40:00Z'
labels:
  - v2
  - backend
  - cli
  - agents
  - research
  - security
  - ax
milestone: m-1
dependencies:
  - TASK-55
  - TASK-57
  - TASK-90
  - TASK-91
documentation:
  - .backlog/docs/design/doc-29 - Design-Agent-Sessions-and-Runtime-Isolation.md
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/design/doc-27 - Design-Agent-Instructions-Assembly.md
  - .backlog/docs/specs/doc-52 - Spec-Knowledge-Decisions-and-Tech-Debt-UX.md
  - >-
    .backlog/docs/organization-data/doc-87 -
    Organization-Data-10-—-Usage-and-Cost-Accounting.md
  - >-
    .backlog/docs/organization-data/doc-92 -
    Organization-Data-17-—-Budgets-and-Controls.md
  - >-
    .backlog/decisions/decision-25 -
    ADR-0025-An-agent-can-start-sub-agents-inside-its-turn.md
  - >-
    .backlog/decisions/decision-28 -
    ADR-0028-An-agent-is-always-available-a-persons-message-and-a-task-assignment-also-start-it.md
  - >-
    .backlog/decisions/decision-31 -
    ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md
priority: medium
type: feature
ordinal: 102000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Introduces token-efficient First-Class Research Workflows in v2 using a clean two-tier hybrid with zero new core database tables: Tier 1 keeps fast turn-scoped local lookups inside ADR-0025 sub-agents + TASK-91 Unified Search (never adding runner spawn) and adds opt-in office-brokered agent-os research web for external facts; Tier 2 routes deep/async org, codebase, or web investigations as tracked tasks (tasks.type = research) publishing durable canonical documents (documents.kind = report) linked back via relates_to. Crucially, to prevent blowing up caller context windows and avoid redundant cold-start re-reads on follow-ups, every research run returns a stable research_id tied to its saved runtime session checkpoint on disk so follow-up questions with --research-id restore warm context within a configurable retention TTL (gracefully falling back to a fresh session seeded with the saved report if expired). Enforces a strict Lethal-Trifecta air-gap separating offline local/code research from opt-in web egress.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Tier 2 Research Tasks & Durable Reports: agent-os research request/wait create and await tracked tasks (tasks.type="research", never spawning a runtime sub-agent per ADR-0025); adds relates_to (task -> document) link support in crates/org-office/src/tasks/; completing project-scoped research publishes a canonical documents row (kind="report", valid owner_employee_id, Research Report template in doc-52) under projects/<id>/docs/reports/ linked back via relates_to (with task-body/comment fallback for projectless team research), indexed by TASK-91 and rebuildable across Memory and SQLite (ADR-0018).
- [ ] #2 Resumable Context-Isolated Sessions (Token Efficiency): every research run returns a stable research_id mapped to its persisted runtime session checkpoint (claude --resume / agy --conversation / codex thread resume); follow-up calls passing --research-id <id> within the configurable warm TTL (e.g. 1 hour) resume that exact researcher session with full warm context, omitting --research-id starts a brand-new session, and expired/cleaned/moved checkpoints transparently fall back to a fresh session pre-seeded with the saved report and citations.
- [ ] #3 Local/Org/Code Research Air-Gap: runs 100% offline with zero web egress (WebSearch/WebFetch disallowed; os-sandbox TASK-57 network isolation); distinguishes ADR-0025 in-turn sub-agents (narrow ADR-0025 d9 amendment if adding a read-only researcher sub-agent) from dedicated Tier-2 research agents running PermissionPreset::ReadOnly (with explicit CommandPrefix("agent-os ") allow rule), scoped strictly to authorized project docs (doc-86) and task_repositories (doc-35), with zero access to other employees private notes (ADR-0031), unjoined protected chat, or unauthorized budgets.
- [ ] #4 Opt-In Office-Brokered Web Research: disabled by default and opt-in per agent via owner-only agent_execution_configs.tools_config_json (schema.sql:1661), failing closed offline/unconfigured behind a replaceable WebResearchProvider port (explicit ADR/dependency for outbound HTTPS since Cargo.toml:94 forbids HTTP clients); refuses --repos when --mode web; enforces query length cap, pre-egress text_has_secret screening (documenting 64-hex handling), post-DNS SSRF IP pinning on every redirect hop, search-only default, per-turn/per-day caps, owner-visible egress audit logs, and <<<DATA ... DATA>>> untrusted-content fencing with canonical URL + retrieved_at UTC metadata.
- [ ] #5 CLI, Short-Hold & Cost/Budget Attribution: adds research request, research web, and research wait (capped at 90s short hold per Non-Negotiable #18) within CATALOGUE_LIMIT = 4096 (TASK-90); fixes crates/org-office-sessions/src/lifecycle.rs:1536 to thread active task/project/team/task_type_snapshot scope into usage_records (enforcing doc-92 per-task and per-agent budgets across researcher turns and ADR-0025 sub-agents), emits usage_records for research web (agent or one_off), and updates doc-29, doc-28, doc-27 (on-demand research skill), doc-52, doc-3, doc-62, plus a new ADR.
<!-- AC:END -->
