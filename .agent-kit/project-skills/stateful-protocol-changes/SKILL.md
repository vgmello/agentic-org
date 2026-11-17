---
name: stateful-protocol-changes
description: Implement, fix or review session, connection, process and provisioning lifecycles so every exit path, shared resource and refusal is handled once, as a class.
---

Status: trial under PI-01 (H1). Read the slice decision's Invariants table and operation × outcome matrix if present; otherwise derive the relevant
invariant and state it in your review or fix summary. A missing table alone never makes a review incomplete.

**Every exit path.** For each lifecycle operation (sign-in, refresh, sign-out, start, stop, provision step, run, cancel) enumerate the outcomes:
success, transient failure, definitive refusal, timeout, caller abandonment (reply undeliverable), crash/restart. Each cell that changes state needs a
defined result and a test. Build replies before committing state, or roll back when delivery fails. Invalidate local authority before asking a
remote party to end it. Return the same cleanup signal (e.g. a cleared cookie) on every reply of a terminating route.

**Shared resources.** Never close a shared connection, channel or lock because one caller's local deadline expired; close only on proven desync,
protocol error or a stuck owner. Queue wait never consumes write budget; check the deadline before claiming work. Bounded scans make progress across
calls. Background cleanup never blocks the request path and has a bounded queue.

**One owner per limit.** A size, budget, timeout or name shared by two components has one owner along the established dependency direction
(the consumer-owned or shared contract per AGENTS rule 1), and both sides use it.
Every test fake that mirrors a real component has a conformance test (or shared vectors) against the real rule.

**Refusals keep their cause.** Every refusal or error path keeps its full specific cause in the trusted log (escaped, secret-free); external
replies to untrusted callers keep the contract's fixed refusal text.
User-facing help/docs never promise a command or feature that is not true at this commit.

**Fix the class, not the line.** When fixing a finding: state the violated invariant and check the sibling operations and outcomes. Fix a sibling
only when it shows the same defect, proven by a test that fails before the fix; name a sibling outside your authorized scope in the fix summary (`unaddressed_ids` holds review finding IDs only).
Record "invariant / siblings checked / tests added" in the fix summary (Sol: the fix.json `summary`; writers: the receipt). The runner or lead
re-runs the slice's full gate list.

**Reviewing lifecycle code.** Sweep the operation × outcome matrix for each changed lifecycle, and grade the same defect class with the same severity.
