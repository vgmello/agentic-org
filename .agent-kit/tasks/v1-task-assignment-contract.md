# Explicit task assignment pair: domain implementation

The owner's full goal remains selected v1 in vertical UI/backend/database slices.
Latest direction: if a feature is blocked, move to another feature. The blocked
native-runtime work is preserved on its branch/stash; this task implements the
independent Codex-owned assignment domain behavior selected by document 12.
Review locally and merge finished work into `v1-integration`, without a PR wait.

Implement the complete assignment-pair command shape and deterministic domain
decision in `crates/org-work`, preserving legacy command/store behavior. The
selected task's accountable team, current assigned team and individual assignee
are separate facts. This domain delivery does not claim the API/storage/UI slice
ships; those remaining layers retain their actual ownership and requirements.

Acceptance:

1. Represent all four assignment states: team-only, individual-only, both and
   unassigned. Commands supply both desired fields explicitly, with null to clear,
   and an expected task revision. Missing pair keys, malformed shapes and unknown
   command fields fail decoding rather than silently clearing/retaining a field.
2. Plan a complete replacement against the exact current revision. Validate
   positive revisions in the signed SQLite integer domain and refuse increment
   overflow. Stale expected revision conflicts without a partial result or input
   mutation. An accepted plan carries exactly the next revision and both old/new
   pairs in the document 05 `assignment_changed` payload shape.
3. Every desired non-null assigned team/employee must resolve to an active target
   with the exact requested ID in the same organization. Missing, wrong-ID,
   inactive or other-organization targets fail. Historical old assignment IDs
   remain opaque and are not checked against current membership. Do not infer
   an assigned team or require an employee's home team to equal the assigned team.
4. Task and organization identity remain explicit; preserve opaque existing IDs.
   Assignment cannot change accountable ownership, project/scope, readable number,
   reporter, permissions or file location. The pure decision has no I/O, clock,
   random ID generation, storage mutation or new persistence trait.
5. Do not add a no-op shortcut: the selected command appends an assignment change
   when accepted, including if its complete desired pair equals the current pair.
   Validate the requested non-null targets on each new command. Receipt retries
   belong to the accepting service, which must replay its original result first.
6. Document caller obligations: authorization, trusted target facts, accepting
   writer/fencing and recoverable atomic commit of task, full attributed event
   and idempotency receipt before ACK. This plan is not acceptance, persistence,
   a complete history event or a hash/integrity proof. Numeric canonicalization
   and the blocked runtime are not prerequisites for this independent behavior.
7. Meaningful public-API tests cover the four states, clearing one while keeping
   the other, unchanged-pair acceptance, cross-home-team independence, stale and
   invalid revisions, all invalid target facts, historical inactive references,
   strict JSON pair presence and exact assignment event payload. Existing work
   behavior and tests continue to pass.
8. Run required workspace and kit checks. Obtain fresh local independent review,
   original-author validation and separate fresh final review for the exact
   commit, then merge into `v1-integration`. Preserve outstanding vertical layers
   in the broad v1 checkpoint; do not label all v1 or task assignment shipped.
