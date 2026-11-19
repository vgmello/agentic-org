# POC: DSS process as a workflow

## Question

1. Can the **Desired State Specification (DSS) process** (REQ-06) be expressed with the
   workflow model from **REQ-01** (Markdown action blocks wired by a definition file,
   triggered by agent commands) and **REQ-02** (*In Review* hands the next block to a
   different agent)? What does it look like in [Svelte Flow](https://svelteflow.dev/)?
2. In a real application, **why would events fire, how does an event find its run, how do
   we know the next step actually ran, and who is responsible?** Can conditions like
   `task.status == 'in_review'` be written in the flow while keeping the product
   integration optional?

[DESIGN.md](DESIGN.md) answers question 2 in depth, including how it fits the overall
design and whether the product should add a workflow feature at all.

## What it does

```
workflow/
  dss.flow.yaml        ← flow: agents, triggers, blocks, transitions with CEL conditions
  blocks/*.md          ← blocks: the instructions an agent executes
src/lib/engine.ts      ← pure: validate wiring, type-check CEL, resolve capabilities/fallbacks
src/lib/runtime.ts     ← runner: run/step state, correlation, one-commit moves, reconcile loop
src/lib/host.ts        ← fake host integrations: task board (`task`), project facts (`project`)
scripts/flow.ts        ← CLI over a persisted runner (each call = a fresh process)
src/App.svelte         ← Svelte Flow canvas + simulator (integrations, task board, failures)
```

**Flow** (12 blocks, 7 agent personas):

1. `/project-setup` → *Brand-new project checks* (REQ-01's example) → DSS dialogue.
2. `/dss` → *Product DSS dialogue* → *Publish Product DSS*. Publishing **starts the factory**.
3. `project.standards_defined` (a fact) picks **autonomous** or **interactive** *Technical
   DSS*; autonomous escalates with `decision_open`. Without the `project` integration the
   product owner passes flag `standards_defined` instead.
4. Flag `standards_changed` (a judgment) optionally routes through *Version architecture
   standards*.
5. *Implementation planning* → *Execute task* ↔ *Architecture consultation*.
6. **Level rule** `task.status == 'in_review'` hands the task to the **reviewer** (REQ-02),
   however the task got there. Moving it back to `in_progress` (edge rule with
   `where: event.to == 'in_progress'`) returns it; `approved` needs `task.status == 'done'`.
7. *Reconcile*: `drift_detected` → plan again; `converged` → done; `new_version` loops back.

**Transitions** have `on` (agent outcome or host event), `where` (CEL over the event
payload), `when` (CEL over facts — without `on` it is level-triggered) and an optional
`fallback` used when the host lacks a capability the rule needs.

**Runner**: steps go `ready → dispatched → running → done`. A move and the next step's
creation are one commit; dispatch happens in the reconcile loop, retried after a failure
(backoff) or an expired lease, and escalated after 3 attempts.

## Run it

```sh
bun install
bun run dev        # UI: toggle integrations, start /dss, act as agents, move the task,
                   # lose a dispatch, tick, restart the runner from its snapshot
bun run test       # engine, runner and CLI tests (crash recovery, fallbacks, CEL checks)

# CLI: every call is a new process over the saved state (a restart between each step)
bun run flow --state /tmp/s.json --caps task,project start dss --task T-1
bun run flow --state /tmp/s.json begin R1.1
bun run flow --state /tmp/s.json report R1.1 context_sufficient
bun run flow --state /tmp/s.json status
bun run flow --state /tmp/s.json task T-1 in_review   # host move → reviewer dispatched
```

## Outcome

**Round 1 (definition):**

- The whole DSS process fits as Markdown blocks plus a declarative flow; branching needs only
  conditions, no code per step.
- REQ-02 handoff falls out of the model: each block names its agent, so a move to another
  agent's block *is* the handoff.

**Round 2 (runtime):**

- **CEL works for both sides of the stack.** `@marcbachmann/cel-js` type-checks against
  declared schemas, so typos and type errors fail at load time; Rust has CEL
  implementations for the office side (to be evaluated).
- **Optional integration works per transition.** The same flow file runs with the `task`
  and `project` capabilities (facts and host events) or without them (agent outcomes and
  flags). Capability detection falls out of type-checking: an expression that is invalid
  for the host but valid once a capability is added needs that capability.
- **Level rules are the robust default.** `task.status == 'in_review'` moves the run even
  when the change event is never delivered; edge rules need exactly-once delivery to be
  trusted.
- **"Did step 2 run" is answerable.** Step records plus the reconcile loop recover a crash
  between the move and the dispatch, a lost dispatch and an agent that never starts; the
  tests prove each case. Without a real agent turn runner (FC-101) a dispatch can only hand
  out Markdown, so the last link stays simulated.
- **Should we add it:** yes, as an optional library adopted in stages — see
  [DESIGN.md](DESIGN.md#should-we-add-it) for the trade-offs.

### Gaps before taking it into the product

- The run store is a JSON file or memory; the product needs the office journal (single
  writer, operation receipts).
- No real event source: the product needs a durable cursor over `work_feed`.
- Each run has one subject task; a feature's N tasks need child runs plus a join.
- The real task statuses have no `in_review` (decision open, see DESIGN.md).
- No heartbeat while a step is `running`; only the dispatch-to-start lease is enforced.
- Flow versions: running runs are not pinned to or migrated between flow versions.
- A block has one owning agent; pairing (architect + staff engineer) is only in the Markdown.
- Svelte Flow layout is hand-placed; labels overlap on back-edges.

### Known issues (from the round-2 and round-3 reviews, not fixed unless noted)

Committed as-is while the direction is undecided (see DESIGN.md, *Workflow or agent-driven*).
Fix these before reusing the runner code:

- **Flags stick to the run** (`run.flags = ...` in `applyReport`), so a later loop
  (`new_version`) reuses an old `standards_changed` unless the agent clears it with
  `!standards_changed`. Flags should apply to one report only (not yet done).
- **A duplicate of an accepted report replays `accepted`** (exit 0) even though a fresh
  call would now be rejected. State is not double-applied; only the output is misleading.
- **Concurrent CLI writes are not atomic**: the state file is written via a fixed
  `<file>.tmp` name, so two processes writing the same `--state` can clobber each other.
  Single-writer use only.
- **Dispatch runs before the caller saves state**, so "one commit, then dispatch" is only
  half true; the crash test throws in the dispatcher rather than crashing after the commit.
  Needs an outbox drained after persisting.
- `where` on an agent outcome throws at runtime, and CEL evaluation errors are not
  contained (should move the run to needs attention).
- `task __proto__ done` in the CLI writes to `Object.prototype` (needs `Object.hasOwn`).
- Two missing capabilities are reported as `invalid condition`; the native transition's
  `capability` is a substring match and names only the first.
- Flags are not restricted to `bool`.
- `needs_attention` is terminal (no resume), hitting the level-move cap is silent, and a
  report carries no attempt token.
- CLI: `standards` does not validate its argument or require the `project` capability;
  `--task` is accepted on every command.
- REQ-03 metrics are argued from step timestamps, not computed.
