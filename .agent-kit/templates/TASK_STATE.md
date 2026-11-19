# Task state: <id>

Copy to `.agent-kit/tasks/<id>.md`. One lead owns this record; update at handoff,
interruption, budget exhaustion and acceptance. Link the slice contract and evidence
instead of repeating them. Aim for 6,000 characters; preserve intent, constraints,
unresolved failures and the next step. Archive completed records under
`.agent-kit/tasks/archive/` and load only the current task when resuming.

- Goal and observable acceptance criteria:
- Stable request/acceptance contract and metrics journal path; registration before planning or explicitly late:
- Status: PLANNED / IN_PROGRESS / BLOCKED / INCOMPLETE / REVIEW / ACCEPTED
- Owner and reviewer:
- Per-slice model/effort assignments and reasons; original author session ID:
- Project role/skill packet and preflight per worker; capability gaps/stale evidence:
- End-to-end owner, delegate scopes and task-wide active writers; focused review requirements:
- Repository/worktree paths, branches, and exact base commits:
- Current revisions and uncommitted work:
- Index backend/root, refresh fingerprint and relevant bounded lookup evidence:
- Current plan and next executable step:
- Canonical plan/child slice links, plan-review outcome or reason unnecessary, and ready/blocked dependencies:
- Architecture decision/review revision, challenged premise IDs, owner/escalation and paused dependencies; accepted revision required to resume:
- Blockers, failed checks, and unresolved review findings:
- Verification configuration, latest receipt paths, and review package paths:
- Sol phase/round and run folder; origin acceptance and fresh final review SHA:
- Collected review components, whole-task usage/cost coverage, final outcome and quality follow-up window:
- Integration order and final combined checks:
- Proposed decision/map updates; accepted ledger references:
- Memory proposal/accepted-record paths or explicit reason no durable update is useful:

## Execution budget

- Maximum active implementation workers: <start with 1; default ceiling 2>
- Maximum elapsed time before reassessment: <minutes agreed for this task>
- Worker turn/time limit: <Claude default 40 turns; Codex uses an explicit task time/turn agreement, not Claude maxTurns>
- Repair attempts before escalation: <default 1 automatic hook repair>
- Escalation owner and stop condition:

These task budgets are an operating agreement, not a scheduler or billing cap.
At a limit, record INCOMPLETE, exact remaining work, failures, and the next step.
Do not weaken acceptance criteria or present a partial task as approved.
