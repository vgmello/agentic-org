---
kind: spec
title: "Employees: supervision, goals, harness identity and private delegates (UX)"
status: for-review
revision: 3
owner: owner
created: "2026-11-09"
updated: "2026-11-15"
requested_by: owner
reviewers: [ac-staff-engineer-ada, ac-product-designer-eden, owner]
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
teaches the `agent-os` commands separately (the request used the legacy `org-agent` name). Channels let an agent ask a team when it
doesn't know whom to ask, then continue with the right person in a DM.

Revision 3 reconciles the body with organization-data 04/10/11/14/16/17.
Selected execution uses explicit logical turns/attempts and one main-process slot;
automatic goals/reminders, native delegation, private memory and scheduling remain
unselected. They are retained as specifically deferred proposals below, not launch
prerequisites. No implementation is claimed.
Runtime design: [`employee-supervision.md`](../design/employee-supervision.md).
Process delivery: [`processes-ux.md`](processes-ux.md).
Channels: [`chat-ux.md`](chat-ux.md), extended there rather than duplicated here.

For standalone v1, these views and controls use the authenticated HQ web application
defined in [server installation and access](server-installation-and-access-ux.md).
Supervision runs at the owning office and survives a browser disconnect. Server
paths are read-only in the UI; an unavailable satellite shows stale status and a
specific command outcome, never an unconfirmed successful goal change or stop.
New goal changes to an office known to be unreachable are rejected as unavailable
before admission; the saved goal remains unchanged. A forwarded request with an
unknown outcome stays Pending confirmation and is resolved by the same operation ID.
Pause is different: HQ commits durable paused status while an offline office's
runtime stop remains visibly pending until acknowledged.

## 0. Decisions (proposed)

Selected foundation: an employee has one home team with optional role/reporting;
execution.md selects its pool/model/tools/prompt independently of identity. People
have no agent execution. Office placement has durable HQ generation/fencing and
local credential bindings; it does not define team membership. One office worker
owns the supervisor/router/session runner. Its one open turn/main-process slot is
not relaxed for helpers. Profiles have 1–64 ordered connections, organization-wide
subscription identity and shared permits; a fresh local catalog does not promise
available capacity. The following additional behaviors are deferred proposals.

1. **An employee is an organization member.** A private delegate, a memory helper and
   a temporary fork are internal work owned by that employee, not another hire, org
   chart entry or independently scheduled employee. Role-based one-off runs
   remain a separate deferred integration feature (RUN-12).
2. **Every office-managed agent has supervision outside the model.** The selected
   topology places it inside agentic-org-office; there is no employee daemon.
   People retain employee profiles without an execution controller. Agent control
   state persists through idle/paused periods; automatic sleep remains deferred. Monitoring needs no model turn. An employee started by a person shows
   **Started by a person · supervision unavailable** until adopted through a verified
   office launch; the app must not pretend to control it.
3. **Mechanism in code, working habits in processes.** Code provisions the employee's
   replica definition, delivers instructions and goals, monitors lifecycle, meters
   usage and enforces permissions and resource ceilings. Markdown processes guide
   delegation, specialist creation, consultation, memory habits and communication.
   No scheduler chooses a specialist or fans out tasks based on their meaning.
4. **A goal is durable and explicit.** The owner, or a caller already authorized by
   the work/organization tools, can set or replace it. The supervisor records who,
   when and the version; it delivers the current goal at start, resume, wake and
   recovery, and after context compaction when observable. A task session also gets
   its task objective and constraints. A reminder never silently changes either.
5. **The employee is a real harness agent.** Its persona has a reserved, generated
   agent definition in the harness's supported format and agent folder, plus verified
   registration/selection. `CLAUDE.md` and `AGENTS.md` are instruction documents, not
   interchangeable agent registration formats. The generated replica keeps identity,
   responsibilities and applicable processes, but has no inherited conversation by
   default. It does not become a second independently operating employee.
6. **Private specialists are reusable definitions owned by the employee.** The main
   employee chooses and briefs them under its effective process, reviews their output
   and owns the result. A specialist cannot give itself rights, another member's
   identity or an org-wide registration. The organization owner can inspect them;
   “private” means scoped to the employee, not hidden from the owner.
7. **Memory collection should spare the main conversation.** A bounded helper can
   extract a checkpoint from a stable snapshot or a temporary fork when supported.
   Semantic extraction is model work, not deterministic monitoring. The supervisor
   schedules and validates it; it does not invent memories or require the main
   session to perform routine extraction. Helpers cannot directly overwrite the
   employee's memory or run project tools.
8. **Health is evidence, not an assertion of understanding.** Alive, making observable
   progress, waiting for approval, waiting on a tool/message, asleep and unresponsive
   are distinct. Silence alone does not mean frozen. A responsive agent may still be
   off goal; the supervisor can remind it, not prove its reasoning is correct.
9. **Internal work shares the employee's limits.** Delegates, helper forks and their
   subprocesses count toward that employee's and office's applicable runtime, usage,
   concurrency and resource budgets. Nested work never creates a fresh allowance.
   Pause, offboarding and shutdown stop owned internal work as well as the main run.

## 1. Flow · Inspect an employee

The selected execution view identifies requested model/pool and actual attempt
connection, logical turn state and placement office/generation with status age.
Blocked routes show concrete readiness/permit reasons; uncertain effects show
Recovery required and preserve the task/workspace. A human can inspect the attempt
history without exposing credentials, private inputs or unauthorized cost amounts.

The deferred **How it runs** proposal retains RUN's mode/default designs. Its
**Supervision** section uses words and times, not color alone:

- **Health:** “Responding · last event 20 s ago”, “Waiting for approval”,
  “Waiting on tool · deadline 14:30”, “Asleep” or “Unresponsive · probe timed out”.
- **Current goal:** text, version, changed by and when; **Edit goal** for an
  authorized caller. Show a changed goal as **Pending delivery** until accepted by
  the runtime; “Delivered” means transport acceptance, not semantic understanding.
- **Internal work:** active delegates and helpers, purpose, selected definition,
  parent run, elapsed time, reported usage and cancellation state. This is a view
  inside the employee, never extra roster rows.
- **Memory:** last successful checkpoint and source run, “Collecting”, “Deferred:
  quota unavailable” or a concrete failure; never a success timestamp on failure.
- **Controls:** authorized Pause, Resume, Interrupt and log controls, when their
  reviewed handlers exist. Desired Pause, stop pending and verified stopped are
  separate; show every unresolved manual/security/budget reason. Resume cannot
  clear another reason, and a budget extension releases only satisfied holds.
  Stopping an internal job uses the same authorization as controlling the employee.

Changing a goal does not wake a paused employee, clear an approval, force a task
switch or discard unmerged work. A busy session receives the update at the next
safe turn boundary; an explicit authorized interrupt remains a separate action.
Activity records goal changes, failed recovery and resource-limit stops without
dumping private prompts or memory text into routine logs.

## 2. Flow · Delegate under a process

**Deferred native-delegation behavior.** No job launch/control support is implied
by an employee record, execution config or process Markdown file.

The default, overridable **Delegation and consultation** process asks the employee
to consider whether a bounded task benefits from delegation, then choose:

| Choice | Use and context |
| --- | --- |
| Fresh self | The employee's own persona, a task brief and required guidance; no parent conversation. Useful for independent review. |
| Private specialist | An employee-created definition for a recurring specialty, with only the context it needs. |
| General purpose | A harness-provided agent for work that needs no employee-specific persona. |

This table describes the default process, not a coded routing rule or a requirement
to delegate every task. The organization can change it through the existing process
approval and inheritance flow. The parent decides whether to create a specialist,
whether to share additional context and how to assess the result. Resource ceilings
and permission boundaries remain enforced whichever process applies.

Definitions show name, purpose, owner, revision, harness compatibility and whether
they are the generated replica or an employee-authored specialist. Updating a
definition affects future launches; a running delegate retains its recorded version.
No fresh-self job receives the main transcript merely because it shares a persona.

## 3. Flow · Recovery and memory

Selected recovery reconciles protected journals/receipts and process identity before
replacement. An attempt replays only with proof of no effects, or resumes using a
verified compatible execution checkpoint with completed tool receipts; uncertainty
remains Recovery required. Automatic retry to “check state” is insufficient.
Cache/Git reconstruction cannot restore protected usage, holds, used extensions,
chat or launch authority by itself. Old cleanup acknowledgement or a verified
machine fence precedes a new HQ placement generation. Task workspaces survive
handoff and turn completion until task closure or explicit safe migration.

The additional health-reminder and semantic memory behaviors below remain deferred;
a memory summary cannot serve as an execution checkpoint or effect receipt.

- A known wait shows its reason and deadline; it is not treated as a frozen model.
- Suspected unresponsiveness triggers a bounded non-model probe where available,
  then the configured interrupt/stop/recovery sequence. After the retry budget is
  exhausted, **Needs you** says what failed and offers the existing controls.
- A manual pause remains until an authorized Resume releases that manual reason;
  budget/security holds remain independent. A reached budget is stop pending until
  worker acknowledgement and is not proof of a stopped process. A crash-recovery policy never resumes a paused or
  offboarded employee. During a person's take-over, the lease rules still apply;
  background reminders/helpers do not inject into the interactive conversation.
- A checkpoint helper reads a fixed snapshot up to an identified event, returns a
  bounded candidate with provenance, and exits. The office commits it only if its
  source and memory revision are still current. Stale candidates are retried or
  deferred within limits, never silently overwrite newer memory.
- If a runtime cannot isolate a fork, use a supported isolated snapshot helper or
  show the feature as unavailable. Collection failure leaves the last good memory
  intact and never prevents a necessary stop. No unbounded checkpoint turn at exit.

## 4. Requirements acceptance criteria (EMP)

Selected portions below remain requirements for the unimplemented execution
integration; deferred portions preserve the earlier UX without selecting engines.
Cost access follows COST-7/document 17: owner-only agent browser totals/history,
project-scoped PM amounts and the authenticated own-agent CLI exception only.
Unknown usage is incomplete, never zero; release warning caps never stop work.

- EMP-1 · **Selected execution foundation; asleep/scheduled persistence remains deferred.** Each placed runnable agent has a deterministic controller inside the office worker, independent of its model conversation. People have no execution controller. Idle/paused monitoring uses no model turns; person-started sessions show the supervision limitation.
- EMP-2 · **Selected safe observation; automatic investigation/restart policy deferred.** Health distinguishes a known wait from unresponsiveness. A silent long tool run is not killed merely for producing no tokens; recovery has explicit deadlines and a bounded retry budget.
- EMP-3 · **Deferred goal delivery/reminders.** An admitted goal change records actor/version and delivers at start, resume, wake, recovery and supported context reset. A new request to an office known to be offline is unavailable with no goal write or queued change; an already-forwarded unknown outcome stays Pending confirmation and lookup/retry uses the same ID. Test reconnect, duplicate submission and expected-version conflict; pending delivery is visible and paused agents remain paused.
- EMP-4 · **Deferred reusable native replica behavior; admitted launch identity validation still required.** Every supported employee harness receives a generated replica file and verified native registration/selection. A missing or wrong definition is an explicit preflight failure, never silent general-purpose fallback.
- EMP-5 · **Deferred native delegation.** A fresh-self delegate has the employee's persona and required guidance but no inherited transcript by default; its work remains attributed to its owning employee.
- EMP-6 · **Deferred private specialists; owner inspection never extends to author-only private task notes.** An employee can create, inspect, revise and use its private specialists without creating members or editing another employee's definitions; the owner can inspect them.
- EMP-7 · **Deferred process-driven delegation and inherited resolution.** Delegation choices and specialist creation are driven by a seeded, overridable Markdown process. Changing that process changes guidance without a runtime code deployment; code never mandates one delegate type for a semantic task category.
- EMP-8 · **Deferred internal jobs; the no-authority-widening restriction remains selected for all launches.** The parent remains responsible for internal work and its outputs. Internal jobs cannot gain more permissions, channels, credentials or spending authority than their parent grant.
- EMP-9 · **Selected actual-route accounting/unknown labels; child-job model deferred.** Every chargeable attempt retains actual consumer/connection/model attribution and known/incomplete usage. Failed attempts remain counted without overlapping parent/component double counting. A future child-job extension must retain owner/parent/definition/lifecycle and shared limits; unknown usage is never zero.
- EMP-10 · **Selected cleanup/fencing for main processes and descendants; native job lifecycle deferred.** Pause, offboard, relocation, process stop and office crash reconcile marked ownership before replacement. Verify boot/start identity and captured descendants; a restored PID or timeout cannot authorize a signal or new placement. Handoff preserves task worktrees; turn success does not require killing a compatible idle process. Native-job completion rules remain deferred.
- EMP-11 · **Deferred semantic memory extraction.** A memory helper uses a bounded snapshot/fork with no project write tools, exits or is cancelled by deadline, and cannot overwrite newer memory. Main work continues unless the existing session lock requires serial execution.
- EMP-12 · **Selected quota/hold/permission boundaries; memory/reminder behavior deferred.** Memory failures preserve the last good checkpoint. Helpers and reminders cannot bypass quota, permissions, take-over leases or pause, and cannot create recursive helper work.
- EMP-13 · **Selected turn/attempt/readiness/stop-pending presentation; goals/internal-work/memory panels deferred.** Display logical turn, actual attempts, readiness reasons, fresh/stale office observations and pending versus verified stop in text accessible to assistive technology. Apply employee/work authorization and amount visibility to every control/view. Additional goal/internal-work/memory panels retain the same boundary when separately selected.
- EMP-14 · **Selected production-driver conformance; native-delegate capability suite deferred.** Runtime capability checks cover registration, isolation, descendant accounting/cancellation and instruction delivery. Unsupported delegation is visible and follows the process fallback; unsupported safety guarantees never cause restrictions to be disabled.

## 5. Questions for implementation review

- Which installed versions of Claude Code, Codex and Antigravity pass the new native-definition and delegation checks? Vendor documentation is evidence of format, not proof of isolation in our launch configuration.
- What default probe deadlines, reminder interval, helper size/time caps and aggregate resource ceilings fit the measured runtimes? Keep them explicit, bounded and inspectable; do not invent a per-agent limit editor while RUN-6 exposes organization defaults only.
- What memory schema and retention rules should checkpoint candidates use? The invariants in §3 and EMP-11/12 apply whichever storage format is chosen.

These questions remain open for the deferred proposals. They do not authorize a
scheduler/memory/delegate engine or alter the selected execution data contract.
