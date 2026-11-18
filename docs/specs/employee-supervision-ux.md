---
kind: spec
title: "Employees: supervision, goals, harness identity and private delegates (UX)"
status: current
owner: lead
created: "2026-11-09"
updated: "2026-11-17"
requested_by: owner
builds_on: [agent-run-modes-ux, processes-ux, chat-ux, roles-ux, costs-and-insights-ux]
---

# Spec · Employees: supervision, goals, harness identity and private delegates (UX)

## The request (the user, 2026-11-09)

Each employee needs deterministic supervision outside its main conversation: detect an
unresponsive runtime, deliver and remind it of its goal, help collect memory, and keep
delegates and subprocesses within limits. Employees must have native harness agent
definitions so a delegate can use the employee's persona. An employee can also create
private specialists and choose between those, a fresh copy of itself and a general
purpose agent. Those choices are driven by organization processes, with defaults the
organization can change. Processes are injected; standards are looked up. Tool help
teaches the `agent-os` commands separately. Channels let an agent ask a team when it
doesn't know whom to ask, then continue with the right person in a DM.

v1 delivers supervision of the main process, health, recovery and the execution view.
Goals, harness replicas, delegation, private specialists and memory collection are
[not in v1](#5-not-in-v1). The data contract (organization-data 04, 10, 11, 14, 16 and
17) owns the records; the [runtime design](../design/employee-supervision.md) owns the
mechanism. Process delivery is in [`processes-ux.md`](processes-ux.md); channels in
[`chat-ux.md`](chat-ux.md).

These views and controls are in the HQ web application
([server installation and access](server-installation-and-access-ux.md)). Supervision
runs at the owning office and survives a browser disconnect. Server paths are
read-only in the UI. An unavailable satellite shows stale status and a specific
command outcome, never an unconfirmed success. A forwarded request with an unknown
outcome stays Pending confirmation and is resolved by the same operation ID. Pause
is committed at HQ while an offline office's stop stays visibly pending until
acknowledged.

## 0. Decisions

An employee has one home team and an optional role and reporting line;
`execution.md` selects its pool, model, tools and prompt independently of identity.
People have no agent execution. Office placement has a durable HQ generation and
fencing and local credential bindings; it does not define team membership. One
office worker owns the supervisor, router and session runner, with one open turn
and one main-process slot per agent. A profile has 1 to 64 ordered connections,
organization-wide subscription identity and shared permits; a fresh local catalog
does not promise available capacity.

1. **An employee is an organization member.** Any internal work it starts is owned by
   that employee, not another hire, org chart entry or independently scheduled employee.
2. **Every office-managed agent has supervision outside the model,** inside
   `agentic-org-office`; there is no employee daemon. People have employee profiles
   without an execution controller. Agent control state persists through idle and
   paused periods, and monitoring needs no model turn. Every agent is started by its
   office; there is no agent outside the product.
3. **Mechanism in code, working habits in processes.** Code delivers instructions,
   monitors lifecycle, meters usage and enforces permissions and resource ceilings.
   Markdown processes guide working habits. No scheduler chooses work based on its
   meaning.
4. **Health is evidence, not an assertion of understanding.** Alive, making observable
   progress, waiting for approval, waiting on a tool or message, suspended on a
   required ask, and unresponsive are distinct. Silence alone does not mean frozen.
5. **Everything an employee runs shares its limits.** Subprocesses count toward the
   employee's and the office's runtime, usage, concurrency and resource budgets.
   Pause, offboarding and shutdown stop them with the main run.
6. **A message never interrupts a busy turn.** A busy agent receives new input at its
   next turn boundary ([ADR-0004](../adr/0004-headless-agent-sessions.md)). Taking
   over an agent is a local command-line operation, never a browser action.
7. **An agent follows the people above it.** It always replies to a person. It
   follows instructions from a person above it in the organization's hierarchy (its
   team lead chain, up to the owner); a message from anyone else is a request, and
   its own chain wins. The rule is defined once, in the
   [supervision design](../design/employee-supervision.md) (decision 6).

## 1. Flow · Inspect an employee

The execution view shows the requested model and pool, the connection of each
actual attempt, the logical turn state, and the placement office and generation
with the age of its status. Blocked routes show concrete readiness or permit
reasons; uncertain effects show Recovery required and keep the task and workspace.
A person can inspect attempt history without seeing credentials, private inputs or
unauthorized cost amounts.

The **Supervision** section uses words and times, not color alone:

- **Health:** "Responding · last event 20 s ago", "Waiting for approval",
  "Waiting on tool · deadline 14:30", "Waiting for an answer from Ana · 12 min" or
  "Unresponsive · probe timed out".
- **Controls:** authorized Pause, Resume and log controls. Desired Pause, stop
  pending and verified stopped are separate; every unresolved manual, security or
  budget reason is shown. Resume cannot clear another reason, and a budget extension
  releases only satisfied holds.

Activity records failed recovery and resource-limit stops without putting private
prompts or memory text into routine logs.

## 2. Flow · Recovery

Recovery reconciles protected journals and receipts and process identity before
any replacement. An attempt is replayed only with proof that it had no effects, or
resumed from a verified compatible execution checkpoint with completed tool
receipts; otherwise it stays Recovery required. Retrying a model to "check its
state" is not recovery. Rebuilding from a cache or Git cannot restore protected
usage, holds, used extensions, chat or launch authority by itself. The old office's
cleanup acknowledgement or a verified machine fence precedes a new HQ placement
generation. Task workspaces survive handoff and turn completion until task closure
or an explicit safe migration.

- A known wait shows its reason and deadline; it is not treated as a frozen model.
- Suspected unresponsiveness triggers a bounded probe that uses no model, then the
  configured stop and recovery sequence. When the retry budget is exhausted,
  **Needs you** says what failed and offers the existing controls.
- A manual pause stays until an authorized Resume releases that manual reason;
  budget and security holds are independent. A reached budget is stop pending until
  the worker acknowledges; it is not proof of a stopped process. Crash recovery never
  resumes a paused or offboarded employee. During a person's local takeover, the
  takeover rules of ADR-0004 apply and nothing else injects into the session.

## 3. Acceptance criteria (EMP)

Cost access follows COST-7 and document 17: owner-only agent totals and history,
project-scoped amounts for project managers, and the authenticated own-agent CLI
exception only. Unknown usage is incomplete, never zero; release warning caps never
stop work.

- EMP-1 · Each placed runnable agent has a deterministic controller inside the office worker, independent of its model conversation. People have no execution controller. Monitoring an idle or paused agent uses no model turns; agents started by a person show the supervision limitation.
- EMP-2 · Health distinguishes a known wait from unresponsiveness. A silent long tool run is not killed merely for producing no tokens; recovery has explicit deadlines and a bounded retry budget.
- EMP-3 · Not in v1: durable goals delivered and reminded at start, resume and recovery.
- EMP-4 · Not in v1: a generated harness replica definition for each employee.
- EMP-5 · Not in v1: fresh-self delegates. A general-purpose sub-agent inside a turn is in v1 (ADR-0025).
- EMP-6 · Not in v1: private specialists.
- EMP-7 · Not in v1: process-driven delegation choices.
- EMP-8 · No launch or subprocess gains more permissions, channels, credentials or spending authority than its employee's grant.
- EMP-9 · Every chargeable attempt keeps its actual consumer, connection and model attribution and known or incomplete usage. Failed attempts are counted without double counting; unknown usage is never zero.
- EMP-10 · Pause, offboarding, relocation, process stop and office crash reconcile marked ownership of main processes and their descendants before replacement. Boot and start identity and captured descendants are verified; a restored PID or a timeout cannot authorize a signal or a new placement. Handoff keeps task worktrees; a successful turn does not require killing a compatible idle process.
- EMP-11 · Not in v1: memory extraction by a helper.
- EMP-12 · Nothing that runs for an employee bypasses quota, permissions, the takeover rules or pause.
- EMP-13 · The logical turn, actual attempts, readiness reasons, fresh or stale office observations and pending versus verified stop are shown in text accessible to assistive technology. Employee and work authorization and amount visibility apply to every control and view.
- EMP-14 · Runtime capability checks cover isolation, descendant accounting and cancellation, and instruction delivery for each supported driver. An unsupported safety guarantee never causes a restriction to be disabled.

## 4. Open questions

- What default probe deadlines and aggregate resource ceilings fit the measured runtimes? They stay explicit, bounded and inspectable.
- How a lead and a remote worker that sees only pushed files exchange work and results, with an audit trail, is not designed.

## 5. Not in v1

- **Goals:** a durable goal per employee, set by the owner or an authorized caller, versioned, delivered at start, resume and recovery, shown as Pending delivery until the runtime accepts it, never waking a paused employee.
- **Harness replicas:** a generated native agent definition carrying the employee's persona, responsibilities and processes, with no inherited conversation.
- **Delegation:** fresh-self delegates, employee-owned private specialists that the owner can inspect, and general-purpose delegates, chosen under an overridable process. Delegates share the employee's limits and never widen its authority.
- **Memory collection:** a bounded helper that extracts a checkpoint from a fixed snapshot and never overwrites newer memory; failure keeps the last good memory and never prevents a stop.
- **An internal-work panel** listing delegates and helpers inside the employee view.
