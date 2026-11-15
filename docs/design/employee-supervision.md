# Design: employee supervision, native identity and internal work

Status: **execution integration requirements and deferred behavior proposals,
rev 2** (2026-11-15). Documentation only. [Document 04](organization-data/04-agent-structure.md)
owns employees; [11](organization-data/11-model-profiles-and-agent-execution.md)
and [14](organization-data/14-model-providers-and-connection-pools.md) own
execution/profile/pool contracts, [16](organization-data/16-recovery-and-office-continuity.md)
recovery evidence and [17](organization-data/17-budgets-and-controls.md) budgets.
See the [authority map](documentation-authority.md). Those selected data/control
contracts do not select automatic schedules, semantic goals/reminders, native
delegates, private memory or checkpoint-extraction engines. Earlier behavior is
retained below with its specific scope rather than made a prerequisite for turns.
The UX traceability remains [EMP-1..14](../specs/employee-supervision-ux.md).

In standalone v1, the [server design](standalone-server.md) owns instance paths and
access: browser controls arrive through authenticated HQ operations and are routed
to the owning office. That office runs the supervisor, keeps its durable records
and resolves employee paths. Browser disconnection does not end a run, checkpoint
or delegate. HQ views show the freshness of remote status; cached status is not
proof that an offline satellite completed a control operation.

## Decisions (proposed)

1. **One office worker owns execution.** The agentic-org-office process contains
   the supervisor, router and session runner. There is no employee supervisor
   daemon. org-supervise remains the OS-control library; org-runtime adapters
   plan/translate, and org-office-sessions is the planned integration layer.
   Domain modules do not acquire process-control dependencies.
2. **Use existing employee identity.** One required home_team_id, optional role
   and reports_to belong to profile.md; model/profile/tools/prompt belong to
   execution.md. Placement/generation, local credentials and workspaces are
   separate authorities. People have no agent execution. A lead must be a direct
   home-team member and manages its subtree; ancestor-team membership alone
   grants no control. Owner account authority is separate from employee flags.
3. **One open logical turn and main-process slot.** A runnable agent selects one
   model profile (ordered pool) and one canonical model. Connections use private
   office-local bindings and shared HQ permits. Warm verified-idle reuse is allowed
   only with the same launch digest. Unknown cleanup retains the slot. A proposed
   helper/delegate is not an exception to the selected invariant.
4. **Explicit work first.** Current selected execution accepts explicit turns;
   it does not automatically choose work, queue messages, remind goals, extract
   memory or spawn specialists. Later behavior must use the same admission,
   fencing, budget and recovery contracts. Missing native capabilities stay
   unavailable; restrictive tools are never enabled merely to support delegation.
5. **Mechanism does not grant authority.** Generated prompts are projections of
   admitted identity/role/policy. An execution file, editable memory or specialist
   cannot grant permissions. The native replica and process-driven delegation
   proposals in §§2–5 remain deferred independently of core execution.

## 1. Control loop and durable records

The selected records are the same records as document 11, not a parallel employee
supervisor schema. Durable journals are authoritative; SQL is an indexed projection.

| Record | Minimum selected facts and authority |
| --- | --- |
| Employee/execution config | Existing identity/home team; optional config required before runnable with model_profile_id, canonical model_id, tools and prompt. Portable definitions live in organization Git. |
| Placement | HQ's protected monotonically increasing placement_generation and office; replacement requires old verified cleanup or an explicit machine fence. |
| Profile/connection/binding | Ordered connections, subscription identity shared organization-wide, local private credentials; fresh auth/model probes and compatible policy qualify routes. |
| Turn/attempt/process | Frozen input/pool/model/workspace/policy; actual connection/permit/usage per attempt; mark and boot/start identity before input. One open turn and not-gone main process. |
| Execution checkpoint/tool receipt | Immutable admitted context and verified effect outcomes. Unknown in-flight effects prohibit blind replay. Native portability must be verified by the destination driver. |
| Holds | Manual/security and each budget independently; stop_pending is requested stop, held requires acknowledgement. Policy/period/used-extension history survives restart. |

The protected sources include immutable input/checkpoint/tool files, execution
journal/receipts and HQ permit/placement authority. Losing SQLite does not erase
these; losing the sources while a child may exist returns RECOVERY_REQUIRED.
Git alone does not restore protected chat, private notes, usage or budget history.
Task workspaces follow document 04's runtime employee/task/repository tree with one
cache per office/repository. Handoff, turn end and process exit do not close a task
or authorize removal of its worktree. Recompute sandbox/Git/credential/other-task
denies before using the target layout.

The selected control path consumes facts and explicit commands:

1. Validate caller scope, current config/placement and every active hold. Admit
   no new work while recovering/stopping. Resolve lost acknowledgements through
   the original operation receipt; an unknown response is not a failed mutation.
2. Choose eligible same-model connections in frozen order; obtain the shared HQ
   permit. Observe known wait/protocol/tool facts without interpreting silence as
   failure. A local cache cannot grant quota or override stale authority.
3. Persist ownership before spawn/input, pump a single turn and record actual-route
   usage and tool outcomes. Retry each connection at most once per dispatch cycle.
4. On failure stop/reconcile; replay only with proof of no effects or resume a
   verified compatible checkpoint with completed receipts. Missing evidence means
   recovery_required, retaining slot/workspace/evidence until explicit recovery.
5. Release permit/process ownership only after verified terminal/idle/cleanup facts.
   Expired permits remain release_pending and consume capacity. Worker startup
   reconciles journals/owned processes before any replacement; host recovery does
   not race a live worker. No crash-trigger automatic rerun is implied.

### Deferred goal, health-reminder and memory control proposals

A separate future behavior contract may retain desired run policy, goal version,
reminder/deadline history, native job ownership and memory candidate/base revisions.
These sketches are not selected tables or a scheduler already in the worker.
A future loop must preserve pauses/takeover exclusivity, rate-limit reminders,
coalesce extraction requests and retain retry/backoff evidence across restarts.
It must use explicit execution turns and never recover unknown effects by issuing
a fresh “check your state” prompt. Health investigation remains bounded and cannot
invent side-effect completion from a model summary.

Health is orthogonal to desired state and presence. Track last protocol event,
active operation, known wait and deadline, probe result and process identity.
No token output is not by itself a stall. A tool with an explicit deadline uses that
deadline; an unknown-duration operation enters a configured bounded investigation
path rather than being assumed healthy forever. OS liveness cannot establish
semantic progress. If a runtime exposes no probe, say so and use only verified
transport/operation deadlines. Retry counts and backoff survive office restarts so
a crash loop cannot reset its allowance. Goal reminders are rate-limited and contain
only current relevant text, not a growing history of previous reminders.

**Deferred goal authority proposal.** An authenticated owner authorized through HQ's HTTP boundary
can set the employee goal; an office certificate alone grants no human authority.
An agent may
propose an update or set an objective through a tool only when already authorized
for that member/work item; this design grants no new cross-agent control rights.
Use expected-version updates, preserve actor history and reconcile competing task
and employee goals visibly. The supervisor repeats the goal, never resolves a
semantic conflict or silently rewrites the assignment. Acceptance by the transport
does not prove the agent read or understood it.

### Goal command admission and offline behavior

Choose **reject before admission** for a new goal change when its owning office is
known to be unavailable: return `unavailable`, preserve the previous goal/version,
and enqueue nothing. The browser keeps its unsaved draft and offers an explicit
retry after reconnect. Once forwarded, a lost acknowledgement is an unknown outcome,
not confirmed rejection: retain the operation ID/payload digest and Pending confirmation
until office lookup/retry resolves it. The office durably stores the goal, actor,
expected-version result and deduplication record atomically before acknowledging.
An expired lookup window is explicit and never silently resubmits a fresh command.

These proposed office-owned goal changes are distinct from HQ-authoritative manual Pause.
HQ can commit the desired pause while runtime stop acknowledgement is pending;
reconciliation applies current authority and all independent holds before new work.
Resume releases its manual reason only; no goal update or recovery releases a
security/budget hold. Raising, clearing or extending a budget releases only the
holds satisfied by that policy change. Unknown usage remains explicitly incomplete;
known reported amounts can still reach limits, and release caps warn only. EMP-3 covers offline
rejection, ambiguous replies, same-ID retry and conflict. No unbounded goal queue
or new goal authority at a relaying satellite is implied.

## 2. Native harness definitions

**Deferred native replica/specialist proposal.** No automatic provisioning, native
delegation or memory service is selected by document 11. Its launch-generated
configuration is distinct from the additional reusable-persona behavior below.

Keep employee profile.md separate from execution.md and the admitted policy/process snapshot; render native
artifacts from that source. Generated replica content and employee-authored
specialists are distinct. Reserve `org-<lowercase member id>` for the replica;
scope specialist names to the employee, reject collisions and path traversal, and
record the content digest used by each launch. Updates are atomic and idempotent.
Do not overwrite handwritten files or alter the user's global harness config.

Resolve the effective approved role/override from the handbook as
[`roles-ux.md`](../specs/roles-ux.md) requires; editable home memory/notes cannot
redefine role boundaries. A generated definition is a projection of that authority,
not another editable source of truth or a way around role approvals.

The adapter materializes a protected generated replica file in a supported harness
agent folder scoped to this employee/run, then registers/selects the same content
through a verified launch mechanism. It may use launch-time registration to preserve
isolation when ordinary project discovery is disabled; the file remains an auditable
artifact, not an excuse to enable arbitrary workspace settings. A task worktree's
agent file never replaces the reserved employee definition. Native child prompts
must receive the actual persona/process content, not merely a reference to the main
thread's appended prompt.

### Format research (checked 2026-11-09)

| Harness | Official format evidence | Office integration requirement |
| --- | --- | --- |
| Claude Code | [Custom subagents](https://code.claude.com/docs/en/sub-agents): `.claude/agents/*.md`, YAML metadata plus Markdown prompt; session-local `--agents` registration is also documented. | Generate the native file and register/select the reserved definition under the existing isolated launch. Existing `initialize` id/description check stays; additionally verify a delegated replica receives its full prompt and permissions. |
| Codex | [Subagents](https://developers.openai.com/codex/subagents): custom definitions in `.codex/agents/*.toml`. [AGENTS.md](https://developers.openai.com/codex/guides/agents-md) documents layered instructions, a separate mechanism. | Generate native configuration using the installed version's schema and prove loading, selection and descendant controls in app-server mode. Do not assume a Markdown file registers a Codex agent. |
| Antigravity | The repository's [runtime research](../research/agent-runtimes-control.md) remains the starting point; no portable native agent-definition format is established here. | Spike the installed harness. Do not invent a file convention or label a prompt-only persona “native registered”. Expose the limitation until a supported mapping is verified. |

There is no universal `agents.md` registration standard established by these sources.
Documentation describes vendor capabilities, not what the repo's tested versions
support under restricted settings. Record runtime version, source and actual fixture
evidence. Do not copy examples' model or permission settings into organization policy.

**Private specialists.** Store source definitions in an employee-owned workspace
area, outside shared generated/global registration. The employee can author and
revise them through scoped file/tool access; the office validates and renders a
launch snapshot. Only approved prompt/metadata fields pass through: no hooks,
arbitrary MCP servers, credential paths, bypass modes or policy widening. A private
specialist is not a private process: it has no power to override inherited processes
or the owner-approval rules for handbook changes. This proposed owner inspection applies to specialist definitions only; author-only
private task notes have no owner/admin exception and must never be copied into a
specialist or helper snapshot without the author's authorized disclosure. Record
history and retain provenance on offboarding; never reuse another employee's scope.

## 3. Delegation lifecycle and resource ownership

**Deferred.** The selected execution service has no native-delegate orchestration.
The following requirements apply only after a separately selected compatible job
model, shared permit integration and driver conformance establish safe ownership.

The main agent selects fresh self, private specialist or general purpose using its
effective process and task brief. A fresh-self launch defaults to an empty history;
the parent may deliberately choose a bounded context fork where supported. Persist
that distinction. Relevant effective processes and tool help reach every child;
standards remain discoverable on demand, with the parent's authorized scope.

Register intent before spawn and acknowledge the native job id before accepting
work. Internal job ids are not organization member handles. Native child calls run
under an attributable restricted grant, not a copied unrestricted parent credential.
The grant is the intersection of the parent's rights, task scope and child purpose;
no child may access a conversation the parent cannot read or impersonate another
employee. Output is returned to the parent for evaluation. Nested delegates, if
allowed, remain owned by the original employee and consume the same depth/concurrency
and aggregate budget. A checkpoint helper cannot delegate.

A future extension must obtain shared connection permits for chargeable child work;
a local aggregate allowance cannot create subscription capacity. Aggregate main,
delegate and helper usage without double counting native parent
totals that already include children. Expose missing measurements as unknown.
Limits cover concurrent model jobs, descendant count/depth, wall time, token/spend
where available and observable OS resources. Reserve capacity atomically before
launch; release it after confirmed termination, not just after a timeout response.
Quota denial defers optional helpers instead of consuming an emergency second slot.
The organization can configure hard ceilings; process text cannot raise them.

Pause, parent-turn/run completion, offboarding, placement transfer, lease transfer and
office shutdown cancel/drain internal jobs according to a bounded stop policy.
No detached delegate survives a parent run unless an explicit future durable-job
design establishes ownership; this proposal provides no such exception. Reconcile
native child ids and marked OS descendants, including escaped process groups, using
the existing PID/start-time/mark checks. Never kill by a process name or stale PID.
The known same-user process-containment limitations in agent-sessions still apply;
adapters must report incomplete cleanup rather than claiming every process is gone.

## 4. Memory checkpoints without occupying the main conversation

**Deferred memory-consolidation proposal.** These model-generated memory summaries
are distinct from document 11's execution checkpoints/tool receipts. A summary
cannot prove effects, confer resume portability or substitute for recovery evidence.
No memory file/schema/commit service is selected here.

Checkpoint triggers are deterministic lifecycle events or configured intervals:
safe turn boundary, before eviction, after a meaningful run, or an explicit request.
Which information is useful and how the employee uses memory remain process guidance.
The supervisor coalesces requests per source watermark, bounds input/output/time and
chooses an isolated supported extraction mechanism, not a semantic work strategy.

1. Capture an immutable authorized snapshot with session/run id, event watermark,
   active goal version and base memory revision. Do not read a transcript while it
   changes and call that a stable checkpoint.
2. Start a short-lived isolated fork if supported, otherwise a helper with only the
   supplied snapshot. It cannot control the main session, inherit its messaging
   credentials, write the task/home or call organization tools. Its only result is
   a candidate checkpoint. Treat transcript content as data and redact secrets.
3. Validate structure, source/provenance, size and allowed content. The office-owned
   memory commit path checks the base revision and deduplicates by source watermark.
   A stale candidate never overwrites newer memory; retry/defer within the budget.
   Keep generated checkpoints separate from employee-authored persona/instruction
   files. A model summary is fallible data, never a new governing process.
4. Persist through the employee's authorized memory path; task sessions keep the
   existing read-only home grant. Record usage, outcome and last successful source.
   Delete temporary forks and inputs after the retention policy allows it.

This is not the copilot's interactive “draft, then confirm” flow; it does not change
`org-copilot`'s no-write rule. Memory schema/retention and the exact commit API need
implementation design review. Preserve the last good checkpoint on failure. A stop
deadline always wins over extraction; recover from the last committed checkpoint.

## 5. Processes, standards, tools and context

[`processes.md`](processes.md) §3.4 owns the bounded delivery mechanism. The effective
process set, short behavioral instructions, changed text and deferred-body retrieval
are composed into the harness's main instructions and required child guidance.
Structural process/standard files and override flags are selected; inherited
resolution/injection engines remain deferred. Where applicable, overrides default
to allowed unless explicitly prohibited by the owning definition; this proposal
cannot silently impose universal private-only-adds semantics.

The deferred behavior proposal would seed overridable processes for **delegation and consultation**, **memory hygiene**
and **channel questions and follow-up**. They provide defaults for choosing children,
briefing/reviewing them, curating memories and taking a team question into a DM.
The parent makes these decisions; the tools merely enable the operations. Existing
sealed autonomy processes and owner-only decisions still bind these defaults.

Standards such as technology stacks and cloud conventions are indexed/referenced,
not bulk injected. A process tells the agent when to retrieve the applicable standard
and by what reference. Inject concise `agent-os` command/tool help separately from
behavioral process text, with progressive help for detailed arguments. No process
edit invents commands or changes tool permissions.

## 6. Capability checks, implementation slices and validation

Core execution follows AE-01..18 in document 11, durable recovery in 16 and
independent budget controls in 17; those are selected requirements with services
still to implement. E0–E5 and EMP entries concerning goals/delegates/memory below
are deferred proposals, not launch prerequisites for explicit turns. Capability
research never substitutes for authenticated production composition or application
migrations. EMP-1/2/9/10/12/13 retain their compatible execution evidence as scoped
in the UX table; other EMP behavior remains individually deferred there.

### Requirement traceability

| Owner request | Acceptance and design coverage |
| --- | --- |
| Deterministic employee supervisor, goals, memory forks, resource/process cleanup | EMP-1..3, EMP-9..13; §§1, 3, 4; RUN-14/15 |
| Native employee files/persona, private specialists, agent-selected delegates | EMP-4..8, EMP-14; §§2, 3; ROLE-11; draft delegation process |
| Process-driven behavior, injected processes, retrieved standards, separate tool help, bounded context | EMP-7; §5; PROC-27..30 and processes design §3.5 |
| Channel team questions, joining and DM continuation guided by process | CHAT-19..23; ADR-0003 §6.8; PROC-27 |

### Capability checks

Before enabling native internal work, each adapter must demonstrate:

- the generated file's schema, discovery/explicit registration and selected persona;
- fresh-self context isolation and required instructions in the child itself;
- allowed specialist loading with global/project untrusted definitions excluded;
- bounded native descendants, inherited permission intersection, aggregate usage
  semantics, cancellation, escaped subprocess cleanup and restart reconciliation;
- independent checkpoint extraction or a safe serialized fallback;
- changed goal/process delivery on resume and after supported compaction, with no
  duplicated history and no interference during take-over.

Plan slices follow existing ownership, without making domain crates depend on
runtime adapters:

| Slice | Required result | Existing owners |
| --- | --- | --- |
| E0 | Versioned format and isolation spikes; record unsupported capabilities honestly | River, codex-dev; ada for contracts |
| E1 | Durable goal/supervisor state, bounded health/recovery rules, main-session routing | server-dev with River for process control |
| E2 | Generated replica, scoped private definitions, tracked native/office delegates and budgets | runtime owners, server-dev, Remy for CLI/bindings |
| E3 | Bounded snapshot extraction, revision-checked memory commit, retention design | server-dev/runtime owners; ada reviews memory contract |
| E4 | Process seeds, context delivery/help, standards retrieval and channel workflow | existing process, CLI, messaging and app owners |
| E5 | Accessible supervision/internal-work views and acceptance checks | app-frontend-dev, Eden |

Prove EMP criteria with fixture-driven fake time/process events and isolated temporary
workspaces before real-harness integration checks. Include long silent tools, approval
waits, crash loops, stale goals, compaction, exhausted budgets, paused employees,
parent death, nested jobs, stale memory writes and unsupported native registration.
Cross-check PROC-27..30 and CHAT-19..23. Runtime/memory tests never write a user's
organization workspace. This PR changes documentation only; it does not claim those
implementation tests have run.
