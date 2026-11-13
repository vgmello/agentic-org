# P5 charter: org_process::one_branch (Rory)

Scope: docs/design/processes.md §4.2 (the start check, the rule switch, own work, the guarded
moves), A54, A55, A63, A64, A72, and Appendix C's "One branch" line. These are pure functions in
crates/org-process: no SQL, git, network or clock, and no new dependencies. Records, git copies,
ancestry, roles and bindings are inputs the office (server-dev or River, A67) computes and passes
in. Tier A. Each slice is at most 250 non-test code lines.

1. **`rule_on(set: Option<&EffectiveSet>, approved: impl Fn(&TurnedOff) -> bool) -> bool`**
   (A64, A72).
   - The rule is off only when `branch-and-merge` is turned off in the target task's project set
     and the caller says that `off` is approved (its blob was approved by a non-author, §5).
   - It stays on in every other case: no set (can't compute, PROC-17), a missing, clashed or
     invalid key, a conflict on the key, a turned-off key whose `off` is unapproved, or an
     approved `off` of another key.
   - The key name is a single constant, so U26's split (`one-open-branch`) is a one-line change.

2. **`check_start(records, target, exception, rule_on) -> Result<Start, OneBranchOpen>`** (§4.2
   Start, A55).
   - `records` are the agent's task branch records, each with its task, branch, state and
     `stage?`.
   - A `reserved` or `open` record for another task blocks. A `closing` record also blocks
     (fail closed: its move hasn't been answered yet). `merged` and `archived` records don't
     block, and the target task's own record never blocks.
   - Docs, process and retro branches aren't task records, so they never block (A55).
   - When the rule is off, the start is allowed. When an exception is held for the target task
     and its assignee, the start is allowed and `Start` says the exception is used up.
   - `OneBranchOpen { branch, task, stage }` names the first blocking record in a fixed order.
     Until the merge flow supplies a stage, `stage` is none, and the message says "isn't merged
     yet" (A63).
   - The record type is plain input data: the office maps its rows to it, and org-process stores
     nothing.

3. **`own_work(base_sha, tips, uncommitted) -> OwnWork { None, Merged, Some }`** (§4.2 Guarded
   moves).
   - `tips` holds every copy: the `work.git` branch copy, the HEAD copy, the recorded `own_tip`
     and the remote head. Each is `Missing`, `Unreadable`, or a sha the office has judged with
     `descends_from_base` and `in_integration`.
   - Own work is `Some` when:
     - any tip descends from the base, differs from it, and isn't in the integration branch;
     - or there are uncommitted changes;
     - or any copy is missing or unreadable.
   - `Merged` applies only when `own_tip` is in the integration branch and no copy has work
     beyond it.
   - Otherwise it is `None`, which covers a tip at `base_sha` or a tip in the integration branch.

4. **`guarded_move(caller, own_work) -> Move`**, where `caller` is Assignee, Lead (not the
   assignee) or Owner (decision 11).
   - With no own work, the move goes through and the record is archived. When `own_work` says
     `Merged`, it is merged instead.
   - With own work:
     - the Assignee gets `OneBranchOpen`;
     - a Lead or the Owner can make the move, which archives the branch and frees the agent;
     - anyone else gets `NotAuthorized`.
   - Bindings and `forwarded_to_owner` (decision 15) are decided by the office before this call,
     so they aren't part of this function.

**Slices:** P5a covers items 1 and 2, and P5b covers items 3 and 4. I send one request to Tomas
per slice, each with evidence and a guard-to-test table (mutants on the changed lines).
Ambiguities are read fail-closed and listed in the request. The `closing` record blocking (item
2) is one of them.

**Open for ada:**
- **Item 2:** should a `closing` record block a start? I read it as yes, fail closed.
- **Item 1:** should a conflict on `branch-and-merge` leave the rule on even when the version
  that applies meanwhile is an approved `off` (a sealed team's)? I read it as yes, because only
  an `off` that "survives resolution" counts, and a conflict isn't settled.
