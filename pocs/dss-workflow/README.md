# POC: DSS process as a Markdown + JSON workflow

## Question

Can the **Desired State Specification (DSS) process** (REQ-06) be expressed with the
workflow model from **REQ-01** (Markdown action blocks wired by JSON, triggered by agent
commands) and **REQ-02** (a status change such as *In Review* hands the next block to a
different agent)? What would it look like in a Svelte UI using
[Svelte Flow](https://svelteflow.dev/)?

## What it does

```
workflow/
  dss.flow.json      ← REQ-01 wiring: agents, command triggers, blocks, transitions
  blocks/*.md        ← REQ-01 blocks: the instructions an agent executes
src/lib/engine.ts    ← pure engine: validate wiring, run command, fire event
scripts/flow.ts      ← agent-side CLI: command/event in → next block's Markdown out
src/App.svelte       ← Svelte Flow canvas + simulator panel
```

**Flow** (12 blocks, 7 agent personas):

1. `/project-setup` → *Brand-new project checks* (REQ-01's example) → DSS dialogue.
2. `/dss` → *Product DSS dialogue* (product owner talks to the user) → *Publish Product DSS*
   into the domain hierarchy. Publishing **starts the factory**.
3. Guard `standards.defined` (standards *or* an existing Technical DSS) picks **autonomous**
   (no user questions) or **interactive** (architect asks the user only open decisions)
   *Technical DSS* derivation. Autonomous mode escalates with `decision.open`.
4. Guard `standards.changed` optionally routes through *Version architecture standards*
   (the byproduct artifact).
5. *Implementation planning* (staff engineer) breaks both DSS artifacts into tasks.
6. *Execute tasks* (execution agent) ↔ *Architecture consultation* on
   `question.architecture` — agents ask the architect persona, not the user.
7. `status:in_review` → *Review* is **handed to the reviewer agent** (REQ-02);
   `status:changes_requested` returns it; `status:approved` goes to the next task when the
   reviewer passes `tasks.remaining`, otherwise to reconcile.
8. *Reconcile* compares trace links against the published DSS: `drift.detected` → plan
   again; `converged` → done. `product-dss.new-version` loops back to the dialogue.

Transitions are `{ from, on, to, when? }`. `on` is an event or a `status:<name>`
transition; `when` is a context flag guard (`flag` / `!flag`).

## Run it

```sh
bun install
bun run dev        # UI: pick /dss, toggle context flags, click events; click a block to preview it
bun run test       # engine tests (command, guards, REQ-02 handoff, broken wiring)
bun run flow command project-setup
bun run flow fire publish-product-dss product-dss.published standards.defined
bun run flow fire execute-tasks status:in_review    # prints reviewer block with a HANDOFF line
```

## Outcome

- **Yes, it fits.** The whole DSS process is 12 Markdown blocks + 20 JSON transitions.
  REQ-06's branching (autonomous vs interactive, optional standards) needs only
  **guards on context flags**; no code per step.
- **REQ-02 handoff falls out of the model**: each block names its agent, so a status
  event that crosses to a block owned by another agent *is* the handoff. No special
  handoff construct is needed.
- **Agent contract is tiny**: the agent sends a command or event and gets back Markdown
  plus the events it may emit next (`scripts/flow.ts`). That is the shape for an
  `agent-os` / MCP tool later.
- **Svelte Flow works well** for read-and-simulate: custom nodes, live edge state
  (open / blocked by guard / last traversed), drag to rearrange. Layout is hand-placed;
  back-edges (review → execute, reconcile → planning) overlap labels — a real version
  should use auto-layout (e.g. ELK/dagre) and dedicated back-edge routing.

### Gaps before taking it into the product

- Run state is in memory only; the product needs persisted runs per task/office and
  real task status events instead of buttons.
- Context flags are not persisted: the agent passes them with each event (CLI) or they are
  ticked by hand (UI). A real run should store context per run.
- DSS artifacts themselves (product hierarchy, versions, trace links) are described in
  the blocks but not modelled — that is REQ-06's data model, a separate piece.
- No editing of the flow in the UI (Svelte Flow supports it; wiring would need to write
  back to JSON with validation).
- A new Product DSS version is only picked up from *Converged*; publishing v2 mid-delivery
  needs re-entry edges (or a per-run supersede rule) from the delivery blocks.
- The CLI is stateless: any caller can fire from any block, and nothing enforces that only
  the assigned agent executes a block.
- A block has one owning agent; the architect + staff engineer pairing on the Technical DSS
  is only described in the Markdown, not modelled.
- Parallel tasks (many executors at once) are not modelled — the flow is one cursor.
