---
name: architecture-decisions
description: Preserve architecture premises and rationale, obtain independent architecture review, and re-evaluate designs when implementation contradicts their assumptions.
---

The lead owns architecture and its decision history. Use this before detailed
planning of large/complex work and whenever implementation challenges the design.
Large/complex includes multiple dependent slices or subsystems, shared interfaces,
cross-repo coordination, material uncertainty, or security/data/concurrency risk.
Review the suitability of an existing design when it can be reused; do not invent
a replacement merely because the task is large. Routine bounded work can reuse
applicable active decisions after the lead checks their scope and currency.

## Preserve the reasons, not a transcript

Use the project's existing ADR location if established; otherwise create
`.agents/agent-kit/decisions/<ADR-ID>-<slug>.md` from `.agents/agent-kit/templates/ARCHITECTURE_DECISION.md`.
This is the canonical versioned record. Keep `.agents/agent-kit/DECISIONS.md` a short index of governing
constraints, status and links. Contracts, task checkpoints and maps link to the
record; they do not copy its rationale. Commit accepted decisions with the work.

Separate requirements from observed facts and assumptions. Give each premise an
ID, source/authority, evidence revision/date, verification state and recheck trigger.
Record what the chosen approach satisfies, rejected alternatives and why, known
limits, acceptance/falsification checks, owner and review outcome. An unsupported
model assertion is UNVERIFIED, not a fact. Missing evidence does not itself prove
the claim false. Reviewers read applicable original premises and rationale before
judging a replacement and check them against current evidence.
The lead may schedule a bounded disposable feasibility probe when essential
evidence is missing; record its result before declaring architecture READY.

For large/complex work, use plan-review with the active profile’s `architecture_review` model/effort,
a fresh independent context and explicit `PLAN REVIEW`, stage `ARCHITECTURE`. Supply the decision
record/revision, request, constraints, relevant source and proposed validation.
Record READY / CHANGES_REQUIRED / INCOMPLETE on that revision before detailed task
breakdown. Architectural READY permits planning; it is not implementation acceptance.
The lead then produces contracts tied to the reviewed decision revision.

## When implementation contradicts the design

1. The worker pauses affected work and returns the decision/premise ID, expected
   versus observed behavior, reproduction/source evidence, impacted slices and
   current revision. Do not fabricate a missing capability, weaken acceptance
   criteria, silently redesign, or keep repairing against a disproven premise.
2. The lead records the blocker in task state and the decision's challenge history,
   invalidates affected plan readiness, and pauses dependent workers/dispatch.
   Unaffected independent work can continue under its existing contract.
3. Return to the original architecture author/session when available. Otherwise
   assign the profile’s architecture-review model/effort to a staff-engineer to recover the decision record and evidence,
   recording the ownership transfer. Do not pretend a new session has old context.
4. Distinguish an unfinished prerequisite or local implementation defect from an
   invalid architectural assumption. A planned missing dependency may need ordering
   or completion, not a new design. If the design is unsound or infeasible, mark it
   CHALLENGED and propose a replacement. Identify exactly which premises remain
   VERIFIED, are DISPROVED, or remain UNVERIFIED, and why. Preserve valid requirements
   and rationale: failure of a chosen library does not remove tenant-isolation or
   durability requirements. Never relax a user requirement to rescue a design;
   return a genuine requirement conflict to its authorized decision owner.
5. Link a replacement record to its predecessor/revision and explicitly carry
   forward valid premise IDs. Explain rejected premises with evidence, tradeoffs,
   migration/rollback implications and new checks. Obtain independent architecture
   review using the profile’s required model/effort of a material replacement before replanning. A reviewer may
   recheck a focused correction in the same review session; a new reviewer gets
   the relevant records, not a whole transcript. Review independence never means
   ignoring governing constraints. Do not repeatedly restart review to force READY;
   task-routing's review budget applies, and unresolved work stays BLOCKED.
6. After review, the lead activates the accepted decision and marks its predecessor
   SUPERSEDED with a backlink; preserve the history in Git and linked records.
   Update affected plans, dependency order, task assessments/model routing, checks
   and worker handoffs. Inspect/revalidate any already-written dependent code.
   Resume only against the reviewed decision revision and ready revised contracts.
   If evidence supports retaining the design, record that resolution explicitly.

Use project-memory for discovery pointers. Accepted memory about a decision must
include the canonical decision file and relevant current implementation files in
`sources` for scoped recall, so a challenge/revision changes its evidence hash and
triggers revalidation. Summaries never replace the decision
record; reconcile superseded/conflicting accepted records after a replacement.
External evidence still needs its own recheck even when file hashes match.
