---
id: TASK-85.3
title: >-
  AX 3/4: agent-os CLI AX upgrades (4 KiB catalogue grouping, help examples,
  session attach, pagination, msg ask flags, unchecked startup)
status: To Do
assignee: []
created_date: '2026-11-22T14:37:00Z'
updated_date: '2026-11-24T03:17:00Z'
labels:
  - ax
  - backend
  - rust
  - cli
  - testing
  - docs
milestone: m-0
dependencies:
  - TASK-85.1
  - TASK-85.2
  - TASK-83
documentation:
  - .backlog/docs/design/doc-28 - Design-agent-os-Command-Line.md
  - .backlog/docs/specs/doc-59 - REQ-08-agent-os-CLI-and-Kernel-Primitives.md
  - .backlog/docs/design/doc-27 - Design-Agent-Instructions-Assembly.md
  - .backlog/docs/doc-3 - Feature-Checklist.md
  - >-
    .backlog/decisions/decision-14 -
    ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md
  - >-
    .backlog/docs/specs/doc-96 -
    Spec-Agent-Experience-AX-—-agent-os-Onboarding-and-Skills.md
parent_task_id: TASK-85
priority: high
type: feature
ordinal: 91000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit of crates/org-command, crates/org-agent-cli, and crates/org-office-sessions confirmed concrete AX friction points and P0/P1 defects that slow agents down, waste context tokens, or cause silent bugs: help::catalogue() is at 4,054/4,096 B (42 B headroom) while instructions::render_command_catalogue() forks its own 5,256 B renderer and testkit::check_catalogue hard-asserts 1 line/command; session attach runs writes before rejecting non-JSON formats, lets --stdin hijack its JSONL pipe, drops unknown top-level keys, exits 2 without hint on bind errors, and omits data/meta in error_envelope(); msg send/ask strip recipient_presence and woke_agents; msg_dispatch drops non-UUIDv7 idempotency keys and org-command omits retry_with.idempotency-key on Timeout; msg_dispatch falsely stamps Ack::Durable while durable ops under-report Ack::Committed; 8 of 16 Kind::List commands ignore --limit/--cursor and task list adds --max-results 500; boolean flags trap callers on member set-hiring (with illegal hint) and process create|update; msg reply/msg list silently drop conflicting flags while msg ask lacks doc-28 §3.3 scalar flags; help::flags_hint omits non-flag structured inputs and office.rs:218 strips ApiError metadata; table::table() validates 180 JSON examples on every startup instead of CommandTable::unchecked; and we lack live multi-runtime AX evals across claude, codex, and agy.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Solves launch-catalogue scaling within CATALOGUE_LIMIT = 4096 across help.rs, testkit::check_catalogue (testkit.rs:706-725), and instructions.rs:92-102 (deleting the 5,256 B duplicate renderer so standing instructions call table.catalogue()), unblocking TASK-90, TASK-92.1, TASK-98.4, and TASK-99; renders Command.example in --help; switches table::table() to CommandTable::unchecked (keeping CommandTable::new in tests); fixes all session attach P0 framing/safety bugs (reject non-JSON format, --stdin, and unknown top-level keys before dispatch; use AttachSources; reject unknown CLI flags; exit 1 with hint on bind error; conform error_envelope() to envelope_schema(); surface attach in session help); reconciles msg ask meta.status vs data.status and restores stripped recipient_presence & woke_agents on msg send/ask; uses derive_uuid_v7 on msg_dispatch/dispatch/hiring_dispatch and adds ErrorCode::Timeout to retry_with.idempotency-key (org-command/src/lib.rs:134); fails closed on --durable in msg_dispatch/non-durable git ops and reports Ack::Durable truthfully on always-durable commands; surfaces structured --json fields in help::flags_hint, propagates ApiError.retry_after_ms/field/rule (+ RateLimited hint) in office.rs:218-229, and fixes task_dispatch.rs:203 decode error to Unavailable.
- [ ] #2 Enforces --limit (<= 200) and --cursor / next_cursor pagination across all 8 currently unbounded Kind::List commands (directory list/who, channel list, task history, task labels, task-note list, process list, skill list, task list), removes --max-results 500 on task list, and adds task list total, --count, --exclude-status, --unassigned, and --modified-file filters; ensures pre-normalized aliases (who, task note) behave consistently across --help, --schema, and session attach; replaces broken boolean flags on member set-hiring (--allow/--revoke + fix illegal hint at hiring_dispatch.rs:334) and process create|update (--non-overridable) without changing doc-28 §2.1 value-free boolean parsing; rejects conflicting flag combos on msg reply/msg list with InvalidRequest and adds doc-28 §3.3 single-question scalar flags (--prompt, --question-type, --option, --default-value) to msg ask.
- [ ] #3 Updates doc-28 (§2.3 presence, §2.5 grouped catalogue, §2.7 Timeout retry_with, task list filters, ask status/scalar flags, budget/task-note/worktree alignment), doc-27 §1 catalogue wording, doc-59 (REQ-08 if needed), doc-96, and doc-3 (FC-168 evidence); cargo test and org_command::testkit contract checks pass. (Final holistic skill copy pass and live multi-runtime AX evals are split cleanly into TASK-85.4 once the full v1 CLI surface lands.)
<!-- AC:END -->
