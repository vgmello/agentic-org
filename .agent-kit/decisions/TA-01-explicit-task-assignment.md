# TA-01: selected task assignment domain

- Status: ACTIVE, revision 1; source baseline `4dd45f5c` (2026-11-15).
- Scope: [stable task contract](../tasks/v1-task-assignment-contract.md).
- Architecture owner: Codex lead; actual author/planner session
  `01a1dee0-f635-7b11-8050-b8d7113b45f7`, observed Sol Xhigh.
- User direction: move to an independent feature when blocked. Runtime work is
  parked; assignment does not depend on native authentication or numeric hashing.

## Premises and intent

| Premise | Authority/evidence | State / recheck trigger |
| --- | --- | --- |
| TA-P1: accountable team, assigned team and employee are separate; every nullable pair is valid | Selected document 12 revision 4 | VERIFIED requirement. Recheck on owning contract change. |
| TA-P2: complete pair, exact revision and one appended old/new assignment change are required | Document 12; document 05 assignment payload; stable contract | VERIFIED requirement. No implicit patch or equality shortcut. |
| TA-P3: requested non-null targets must be active and same-org; no home-team constraint; historical IDs stay untouched | Document 12 | VERIFIED requirement. Recheck if lookup semantics change. |
| TA-P4: legacy assignment is a different contract | `org-work/src/tasks.rs::assign`, `org-proto` TaskAssignParams, Rust LSP/source inspection | VERIFIED observation: individual-only, absence unassigns, per-aspect conflict behavior. Do not repurpose it. |
| TA-P5: authorization, writer/fencing and durable task/event/receipt acceptance belong to the accepting service | Document 05 accepted-write contract; document 08 attribution | VERIFIED requirement. A pure plan cannot prove these properties. |
| TA-P6: pure deterministic behavior needs no shared port change | `org-work` existing package/serde dependencies; additive module and tests | VERIFIED feasible boundary; falsify if implementation needs I/O, a persistence trait or legacy interface change. |

## Decision and alternatives

Add `org_work::selected_assignment`, leaving legacy types, commands, ports and
behavior untouched. One pure `plan_assignment` receives an immutable snapshot
(organization/task IDs, current revision/pair), a complete desired command and
separately typed desired team/employee facts (ID, organization, active).

The strict command codec requires `expected_revision`, `assigned_team_id` and
`assignee_id`, rejects unknown/duplicate keys and malformed shapes, and preserves
explicit nulls. IDs are nonempty opaque strings; do not trim, normalize or require
UUID syntax. Old assignments are preserved without current membership checks.

Validate task/org identity and positive signed-SQLite revisions, exact expected
revision and checked increment. Validate each requested non-null target against
its exact active same-org fact; null ignores surplus facts and never infers an
assignment. Empty desired IDs are invalid rather than a clearing alias. Choose
and document deterministic diagnostic precedence; it is not authorization policy.

Return a plan with private fields/read-only accessors, task/org identity, next
revision and exactly the serializable document-05 `assignment_changed` from/to
payload. Both nullable keys are always present. Equal-pair accepted commands still
validate targets and produce one change. No input is mutated, even on failure.
No owner, project/scope, number, reporter, permission or path is changed by this API.

The service must authenticate/authorize, resolve trusted facts within its accepted
operation, replay same-operation receipts before invoking the function, enforce
writer/fencing and commit snapshot/full attributed event/receipt recoverably before
ACK. A plan is neither a complete event nor a hash, authorization or persistence
receipt. UI/service integration remains outstanding in the broad v1 goal.

Reject extending the incompatible legacy path, partial-patch semantics, implicit
team inference, fresh membership checks on old facts, equality shortcuts and new
storage traits. No dependencies are added: project-crate Ce stays 1 (`org-proto`),
or 4 including existing external normal dependencies. Native codec and service
concerns do not enter this module. Additive rollback removes the unused new API;
there is no persisted-data migration in this change.

Falsification tests: all current/desired pair combinations; independent clearing;
unchanged-pair acceptance and target rejection; arbitrary historical references;
each invalid target fact; opaque identity; positive/signed-max/overflow/stale
revisions; strict missing/null/unknown/duplicate/type JSON cases; exact change
payload; unchanged inputs on failure. Existing work tests must remain green.

## Review and readiness

Fresh independent Sol Xhigh architecture review session
`01a1dee6-6788-7fb0-bdb9-a94d8b6bcc4a` returned **READY** for revision 1 at the
exact baseline. Actual model/effort were verified from rollout metadata. It found
the boundary faithful and feasible, while preserving all downstream obligations.
Detailed planning and implementation may proceed. Another detailed-plan review is
unnecessary if it introduces no material boundary/interface/security changes.
Return material changes to architecture review; code acceptance remains separate.
