# Workflow runtime: how it would work and should we add it

Companion to the [POC README](README.md). This is a design exploration, not a selected
decision. Product docs referenced here are the source of truth for the current design; see
the [documentation authority map](../../docs/design/documentation-authority.md).

## The problem the first POC skipped

The first POC let anyone "fire" an event. In a real application that raises four questions:

1. **Why would an event fire?** Something real must happen: an agent finishes a block, a
   task moves, a file is published, a person answers, time passes.
2. **Which run does it belong to?** Many runs are active at once.
3. **Did the next step actually execute?** A handoff can be lost: the agent host is down,
   the office restarts, the agent never starts.
4. **Who is responsible for making it happen?**

Today the product has none of the machinery: there is no agent turn runner yet (FC-101),
task status changes are stored but not published (`apps/agentic-org-office/src/lib.rs`
drops `work.changed`), task statuses are only `todo | in_progress | blocked | done`, and
[agent-sessions](../../docs/design/agent-sessions.md#sessions-and-triggers) says messages,
assignments and schedules never create model work implicitly. Process execution and
automatic wakes are listed as deferred engines in [processes](../../docs/design/processes.md).

## The model

**Agents and the host only report. One runner moves runs.**

```
 reports                         runner (one writer: the office)
 ───────                         ───────────────────────────────
 agent outcome (step id) ─┐      1. dedupe by operation id
 host event (subject)    ─┼────▶ 2. correlate: step id, or subject → run → current step
 fact change             ─┘      3. evaluate CEL rules; exactly one may match
                                 4. one commit: close step, open next step (state: ready)
 timer tick ─────────────────▶   reconcile loop
                                   - level rules: fire when a `when` holds
                                   - ready step, no turn → dispatch (start the agent)
                                   - dispatched, not started within lease → redispatch
                                   - too many attempts → needs attention (lead)
```

| Question | Answer in the POC |
| --- | --- |
| Why would it fire? | Agent outcomes (`report`), host events (`task.status_changed`), level rules over facts (`task.status == 'in_review'`), ticks. Commands start runs. |
| Which run? | Outcomes carry the step id (stale or early ones are rejected). Host events carry a subject (task id) and only reach runs bound to it whose current step listens. |
| Did step 2 run? | Steps go `ready → dispatched → running → done`. The move and the next step's creation are one commit; dispatch is retried from saved state, so a crash in between is recovered (test: *survives a crash between the move and the dispatch*). |
| Who is responsible? | The runner. Agents never move the flow; an ambiguous match stops the run for a person instead of guessing. |

### Conditions in CEL

```yaml
- from: execute-tasks                 # level-triggered: fires as soon as this is true,
  when: task.status == 'in_review'    # however it became true (even a missed event)
  fallback: { on: submitted_for_review }
  to: code-review

- from: code-review                   # edge-triggered with a payload filter
  on: task.status_changed
  where: event.to == 'in_progress'
  fallback: { on: changes_requested }
  to: execute-tasks

- from: code-review                   # judgment (approved) guarded by a fact
  on: approved
  when: task.status == 'done'
  fallback: { on: approved }
  to: reconcile
```

Every expression is type-checked when the flow loads: `task.stauts`, `task.status == 1` or
an undeclared `flags.x` fail before any run starts. Prefer level rules: they do not depend
on delivering every event exactly once.

**Facts vs judgments.** A fact the system can read (`project.standards_defined`) is never
asked of an agent when the host can supply it; agent flags are only for judgments
(`standards_changed`), and only the flags a transition reads are accepted.

### Optional integration (capabilities)

The engine owns small capability contracts (`task`, `project`); a host implements the
ones it can. When a host lacks one, each transition that needs it uses its declared
`fallback`, so **one flow file runs with or without the product integration**. Load fails
if a needed capability has neither an implementation nor a fallback.

| Port | Default (no integration) | Product |
| --- | --- | --- |
| Facts (`task`, `project`) | absent → fallbacks | office task/project data, the REQ-07 IA index |
| Events | agent outcomes only | a durable cursor over `work_feed` (status changes are already stored there) |
| Dispatcher | print the block (CLI) | admit an agent turn (`RunTurn`) for the step's employee |
| Run store | JSON file / memory | office journal (single writer, ADR-0002) |

## How it fits the overall design

- **Process packages (REQ-07).** The runner is a generic primitive like tasks; the DSS
  flow is just one package's YAML. Methodology stays out of the platform.
- **Office as single writer (ADR-0002).** Runs and steps belong in the office journal,
  accepted with operation ids and receipts as in ADR-0006.
- **No implicit work (agent-sessions).** The runner is the explicit caller that admits a
  turn. It must use the same admission, one-turn-per-employee and recovery rules — no
  hidden queue, and never a blind rerun when an effect is unknown (doc 11).
- **Processes delivery.** Instead of injecting a whole handbook, a turn gets the current
  block plus its allowed outcomes, which fits the 24 KiB process context cap.
- **Messaging (ADR-0003, REQ-04).** A user question is a step waiting for an answer
  event; an `ask_user` card's answer is just another event source.
- **Metrics (REQ-03).** Step timestamps give block duration, handoff/queue latency
  (`openedAt → startedAt`), rework (review → execute loops), escalations (HITL).

### Dependency direction

```
apps (office)  ──▶  org-flow (engine, runner, capability contracts)
     │                         ▲
     └── task/project adapters ┘   (implement the engine-owned contracts)
```

`org-flow` would depend on nothing product-specific; the office composes it with its
adapters. No cycle, and the engine stays testable without the office.

## Should we add it?

**What it buys**

1. **Guarantees.** "The reviewer gets the work" becomes something the system ensures and
   can prove, instead of hoping an agent remembers a skill's instruction.
2. **Measurement.** REQ-03 (compare workflow versions) is impossible without defined steps
   and step records; skills alone produce no comparable data.
3. **Real handoffs (REQ-02).** Today a handoff is a message plus an assignment; nothing
   makes the next agent act or notices when it doesn't.
4. **Smaller agent context.** Each turn gets one block, not the whole process.
5. **Visibility.** People see where every piece of work is and why it is stuck.

**What it costs**

1. **A new engine** the design explicitly deferred: it needs an ADR and acceptance.
2. **Hard prerequisites.** Without a turn runner (FC-101) the dispatcher can only print
   Markdown, so the strongest guarantee waits for that work.
3. **A second state machine beside task status.** Mitigated by reading task status as a
   fact instead of copying it, but still something to keep consistent.
4. **Rigidity.** Over-specified flows fight agent judgment. Keep flows coarse (handoffs
   and gates); inside a block, the agent decides.
5. **Flow upgrades.** Changing a flow while runs are active needs a version policy
   (runs keep their version, or migrate explicitly).

**Recommendation.** Worth adding, as an optional library adopted in stages, because
REQ-02 and REQ-03 cannot be met reliably without it:

1. Library + CLI with no product integration (this POC's shape) — agents use it as a tool.
2. `task`/`project` capabilities from office data and a `work_feed` cursor.
3. The dispatcher, once agent turns exist (FC-101), plus the ADR for process execution.

Each stage ships on its own; nothing forces the full integration up front.

## Workflow or agent-driven?

Inside a step the work is judgment (investigate, write, decide); agents do that well and a
flow only gets in the way. Between steps the work is coordination (someone else must act,
maybe much later, and a lost handoff is costly); agents do that badly. Test: *if this step
is skipped or the handoff lost, who notices?* If nobody notices and it matters, it needs a
system guarantee.

| Level | What | Fits |
| --- | --- | --- |
| 1. Skills only | The agent follows process docs | Exploratory, one-off or single-agent work; cheap to skip |
| 2. Automation rules | "Task moved to In Review → assign and start the reviewer" | Handoffs between actors (REQ-02), waiting on people/CI |
| 3. Full workflow (this POC) | Defined steps, step records, guarantees, diagram | Gates that must not be skipped, comparing process versions (REQ-03), repeated processes |

For DSS: the Product DSS dialogue, Technical DSS writing, implementation and architect Q&A
are agent-driven; publish → architect, In Review → reviewer and changes → executor are
handoff rules; a full workflow pays off only if REQ-03 is a real goal. Level 2 is a subset
of this runner (task-event transitions only). Undecided.

## Open decisions

1. **"In Review":** add `in_review` to task statuses, or keep it as a flow step shown as a
   board column. The POC's fake board has `in_review` to mirror REQ-02.
2. **Runner authority:** a capability to admit turns and move tasks (ADR-0006 starts
   owner-only).
3. **Busy agents:** one open turn per employee — queue steps, or hire more executors
   (REQ-07).
4. **Fan-out:** a feature's N tasks as N child runs plus a join (not in the POC; each run
   here has one subject task).
5. **Flow versions:** pin running runs to their version, or migrate them.
