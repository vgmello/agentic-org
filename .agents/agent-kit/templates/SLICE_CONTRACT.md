# Slice contract: <id> — <feature>

## Outcome and acceptance criteria

- User outcome:
- Observable acceptance criteria, including failure/empty/loading states:
- Relevant decisions and project instructions:
- Non-goals:

## Plan and readiness

- Overall plan / parent contract (one canonical plan; link child slices):
- Necessary architecture decisions and rationale, or existing pattern to reuse:
- Canonical architecture decision path/revision and applicable premise IDs:
- Architecture owner/session and independent review outcome using the profile's architecture_review model/effort (before detailed planning for large/complex work):
- Ordered implementation steps, dependencies and integration checkpoints:
- Plan revision and relevant source baseline:
- Independent plan review trigger, or lead's reason it is unnecessary:
- Plan reviewer/session, findings and resolutions (when required):
- Readiness: READY / CHANGES_REQUIRED / INCOMPLETE; unresolved blockers:

The lead owns the overall plan and architecture. Workers refine local steps; return
scope/interface/risk changes to the lead before expanding. Use task-routing's
review threshold and architecture-decisions when a premise fails. That invalidates
affected readiness until the design/dependency issue is resolved and revised work
is ready. Plan readiness is an agent assessment, not final code acceptance.

## Ownership and repositories

- Owner and reviewers:
- Planning model/effort and rationale from the active profile:
- Evidence-backed task assessment path and computed kit_policy.py routing:
- Implementation model/effort per slice; all seven familiarity criteria for the familiar route:
- Project skill catalog revision, semantic risk tags and matched critical areas:
- Per-worker role/skill packet and observed-session preflight acknowledgment:
- Full-stack integration owner; delegate scopes, file ownership and agreed interfaces:
- Why delegation is worthwhile, dependency readiness and task-wide writer budget:
- Escalation trigger and required final review model/effort:
- Original author session ID (preserve context through Sol handoff):
- Exact repository/worktree paths and base refs for the review diff:
- Expected files/modules and shared contracts:
- Relevant indexed symbols/callers, language coverage gaps and scoped memory/capsules:
- Coordination or integration order for migrations/shared schemas:
- Changes requiring a scope/architecture discussion:
- Task-state record and time/concurrency/turn budgets:

## Interfaces and data

Cover only layers present in the project/task; mark the others not applicable.

- Database changes, migration and rollback considerations (or None):
- API request/response, validation, errors, auth and tenancy:
- UI states, accessibility, and design references:

## Verification and handoff

- Commands with working directories (keep `.agents/agent-kit/verification.json` aligned):
- Tests for acceptance criteria and cross-layer behavior:
- Required integration/UI checks beyond the fast hook:
- Selected focused review procedures; extra specialist pass only if justified, with scope/findings/resolution:
- Diff against each agreed base ref; staged, unstaged, and untracked files:
- Current verification receipt and per-repository review package paths:
- Sol run folder, findings, before/after SHAs; original intent validation; fresh final reviewer result:
- Known limitations, failures/skips, and proposed decision/memory updates:

Keep the contract proportional to the work. Update it when evidence changes the
scope; do not expand a small fix into a full design exercise.
