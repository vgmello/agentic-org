# Design: employee supervision, native identity and internal work

Status: **proposed, rev 1** 2026-11-09. Requested by the owner. Reviewers: ada,
Eden, River, server-dev, Remy and codex-dev for their existing boundaries.
Implements [`employee-supervision-ux.md`](../specs/employee-supervision-ux.md)
(EMP-1..14), with [`agent-sessions.md`](agent-sessions.md),
[`processes.md`](processes.md) and ADR-0003. Documentation only; no contract or
runtime implementation is added here.

In standalone v1, the [server design](standalone-server.md) owns instance paths and
access: browser controls arrive through authenticated HQ operations and are routed
to the owning office. That office runs the supervisor, keeps its durable records
and resolves employee paths. Browser disconnection does not end a run, checkpoint
or delegate. HQ views show the freshness of remote status; cached status is not
proof that an offline satellite completed a control operation.

## Decisions (proposed)

1. **One supervisor record per employee, not per delegate.** The office's existing
   deterministic scheduler/pump hosts the control loop. `org-supervise` continues to
   own OS process control only. No second model supervises the main model by default,
   and a per-employee daemon is an implementation option, not a requirement.
2. **Preserve the current dependency direction.** `org-office` owns goal, health,
   budget and checkpoint rules as domain state. `org-office-sessions` observes
   runtime events and executes those rules through `SessionControl` and adapters.
   `org-supervise` handles spawn, ownership, stop and reap. Adapters translate native
   protocols behind `org-runtime`; they never pick a delegation strategy.
3. **Provision identity deterministically; choose work through processes.** The
   generated employee replica is mandatory. Choosing a delegate, creating specialists,
   consulting, deciding what to remember and moving channel questions to DMs are
   agent behavior guided by versioned processes. Enforcement is reserved for
   lifecycle, access, resource bounds and existing explicit enforced process rules.
4. **The current single-main-session invariant stays.** At most one employee main
   session runs at a time. Owned internal jobs may coexist only when the runtime
   proves isolation and accounting. Otherwise helpers serialize through the same
   slot or are unavailable; they never start an untracked second employee session.
5. **Do not enable unsafe native tools merely to support delegation.** The previous
   prohibition remains the fallback. Native delegate tools are enabled only after
   the adapter passes the checks in §6, or a supervised office capability provides
   the equivalent. General-purpose native delegates must pass the same checks.

## 1. Control loop and durable records

Sketches below are domain requirements, not final wire shapes. Shared contract
changes remain additive and require their existing owners' review.

| Record | Minimum facts |
| --- | --- |
| Employee supervisor | member, placement/generation, desired run state, active main run, goal version, health evidence/deadlines, recovery attempts, effective resource policy |
| Goal | member, text, optional work reference, version, authorized actor/time, supersedes; per-session last accepted delivery version |
| Internal job | id, employee, parent run/job, purpose, kind (delegate/checkpoint), definition revision, context mode, runtime id, lifecycle, deadline, grants, usage/unknowns |
| Checkpoint | employee, source session/run and event watermark, base memory revision, candidate id, validation/commit outcome, resulting revision |

The supervisor survives session eviction. Offboarding and placement changes use the
existing authoritative member state. Startup reconciles records against marked live
processes and native jobs before any restart. Old placement/generation jobs never
attach to a newly placed employee. Internal jobs have no Member, independent home
schedule, inbox identity or top-level supervisor.

The loop consumes facts and time, not model judgments:

1. Check authoritative desired state and lease. Paused/offboarded means stop, not
   recover. Take-over suspends automatic prompt injection and helper launches.
2. Observe main and child lifecycle, transport, tool/approval/message waits and
   resource meters. Persist coarse transitions; avoid per-token database writes.
3. At a safe boundary, deliver changed goals/process guidance and bounded reminders
   once per version or configured interval. The office sends normal encoded control
   input with its header, never a runtime slash command or a second simultaneous turn.
4. When eligible, schedule a bounded checkpoint job; coalesce duplicate requests.
5. On a real deadline violation, probe where supported, interrupt, stop/reap and
   recover under the existing run-mode rules and retry budget. Record evidence and
   raise Needs you when exhausted. Healthy sleeps require no live model process.

Health is orthogonal to desired state and presence. Track last protocol event,
active operation, known wait and deadline, probe result and process identity.
No token output is not by itself a stall. A tool with an explicit deadline uses that
deadline; an unknown-duration operation enters a configured bounded investigation
path rather than being assumed healthy forever. OS liveness cannot establish
semantic progress. If a runtime exposes no probe, say so and use only verified
transport/operation deadlines. Retry counts and backoff survive office restarts so
a crash loop cannot reset its allowance. Goal reminders are rate-limited and contain
only current relevant text, not a growing history of previous reminders.

**Goal authority.** An authenticated owner authorized through HQ's HTTP boundary
can set the employee goal; an office certificate alone grants no human authority.
An agent may
propose an update or set an objective through a tool only when already authorized
for that member/work item; this design grants no new cross-agent control rights.
Use expected-version updates, preserve actor history and reconcile competing task
and employee goals visibly. The supervisor repeats the goal, never resolves a
semantic conflict or silently rewrites the assignment. Acceptance by the transport
does not prove the agent read or understood it.

## 2. Native harness definitions

Keep one canonical employee identity/profile and process manifest; render native
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
or the owner-approval rules for handbook changes. The owner can inspect it. Record
history and retain provenance on offboarding; never reuse another employee's scope.

## 3. Delegation lifecycle and resource ownership

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

Aggregate main, delegate and helper usage without double counting native parent
totals that already include children. Expose missing measurements as unknown.
Limits cover concurrent model jobs, descendant count/depth, wall time, token/spend
where available and observable OS resources. Reserve capacity atomically before
launch; release it after confirmed termination, not just after a timeout response.
Quota denial defers optional helpers instead of consuming an emergency second slot.
The organization can configure hard ceilings; process text cannot raise them.

Pause, main-run completion, offboarding, placement transfer, lease transfer and
office shutdown cancel/drain internal jobs according to a bounded stop policy.
No detached delegate survives a parent run unless an explicit future durable-job
design establishes ownership; this proposal provides no such exception. Reconcile
native child ids and marked OS descendants, including escaped process groups, using
the existing PID/start-time/mark checks. Never kill by a process name or stale PID.
The known same-user process-containment limitations in agent-sessions still apply;
adapters must report incomplete cleanup rather than claiming every process is gone.

## 4. Memory checkpoints without occupying the main conversation

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
Org/team/project resolution and private-only-adds semantics are unchanged.

Seed overridable processes for **delegation and consultation**, **memory hygiene**
and **channel questions and follow-up**. They provide defaults for choosing children,
briefing/reviewing them, curating memories and taking a team question into a DM.
The parent makes these decisions; the tools merely enable the operations. Existing
sealed autonomy processes and owner-only decisions still bind these defaults.

Standards such as technology stacks and cloud conventions are indexed/referenced,
not bulk injected. A process tells the agent when to retrieve the applicable standard
and by what reference. Inject concise `org-agent` command/tool help separately from
behavioral process text, with progressive help for detailed arguments. No process
edit invents commands or changes tool permissions.

## 6. Capability checks, implementation slices and validation

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
