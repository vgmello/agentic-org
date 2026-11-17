# Task state: v1-f4-gateway-sessions

- Goal/acceptance: `.agent-kit/tasks/v1-f4-gateway-sessions-contract.md` (stable). Internal F4 foundation for U1.
- Metrics journal: `.agent-kit/logs/task-metrics/f3e395633310482895d4311f657eda69.json` (slot-1), registered
  before detailed planning. Read-only F4 inventory work started during the F3c gates (recorded honestly).
- Status: IN_PROGRESS. GW-01 r2 architecture READY (conditions C1-C3 applied); detailed plan = slice contract (no further plan review: no new risk beyond the reviewed r2).
- Owner: Claude lead (hybrid). Final reviewer: fresh Opus High code-reviewer. Sol review via kit_codex.
- Routing: `.agent-kit/logs/v1-f4-sessions/routing-1.json`. Planner/architecture review/worker are Opus High;
  Sol High review (xhigh optional); final Opus High. Reason: security-critical auth/cookies, cross-crate wire,
  prior failed compile ace8b29d, open ambiguity.
- Writer: one fullstack-engineer (Opus High) owns host+web+UI adapter end to end (GW-01 D5). The author
  session ID is recorded at dispatch.
- Worktree: /Users/vgmello/projects/agentic-org-claude-worktrees/slot-1, branch worktree-f4-sessions,
  base e5152b4e (F3c candidate). Rebase onto the accepted F3c merge in v1-integration before the F4 review.
- Concurrent writers: F3c Sol fixer in /Users/vgmello/projects/agentic-org-delivery-worker (separate task/checkout)
  plus at most this one F4 writer = 2. The lead does not write product source while both run.
- Architecture: `.agent-kit/decisions/GW-01-gateway-browser-sessions.md` r1 PROPOSED; extends native-host-boundary N4/N5.
- Evidence: `.agent-kit/logs/v1-f4-sessions/{closure-inventory-1,contract-draft-1}.md`.
- Next step: architecture READY → detailed plan (Opus High) → dispatch writer. Early checkpoint: all-target
  compile of host+web, then a real host-spawns-web session flow test.
- Integration order: F3c accepted+merged → rebase F4 → full checks → Sol cycle → original author → fresh final →
  merge --no-ff into v1-integration, push only that branch.

## Execution budget
- Maximum active implementation workers: 1 (F4), plus the separate F3c Sol fixer.
- Maximum elapsed time before reassessment: 240 minutes after dispatch.
- Worker turn limit: 60 turns. Return at the compile checkpoint and on any P-5/P-7 premise failure.
- Repair attempts before escalation: 1 automatic hook repair.
- Escalation owner: the lead. Stop on premise failure (pause and return evidence), or on any need for task-service
  types or new dependencies.
