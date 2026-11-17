# Design: employee supervision

How an office runs and supervises its agents. [Document 04](organization-data/04-agent-structure.md)
owns employees; [11](organization-data/11-model-profiles-and-agent-execution.md) and
[14](organization-data/14-model-providers-and-connection-pools.md) own execution,
profiles and pools; [16](organization-data/16-recovery-and-office-continuity.md) owns
recovery evidence and [17](organization-data/17-budgets-and-controls.md) budgets. How
a session is driven is in [agent sessions](agent-sessions.md). The screens and the
EMP criteria are in the [employee supervision UX](../specs/employee-supervision-ux.md).

Browser controls arrive as authenticated operations at HQ and are routed to the
office where the agent is placed. That office runs the supervisor, keeps the durable
records and resolves the agent's paths. Closing the browser never ends a run. HQ
shows how fresh a remote status is; a cached status is not proof that an offline
satellite completed an operation.

## Decisions

1. **One office owns execution.** `agentic-org-office` contains the supervisor,
   router and session runner; there is no separate supervisor daemon. `org-supervise`
   is the OS process library and `org-runtime` the vendor runtime contract. Domain
   modules have no process-control dependencies.
2. **Employee identity is the organization's.** The home team, role and reporting
   line are in `profile.md`; model profile, model, tools and prompt are in
   `execution.md`. Placement and its generation, local credentials and workspaces are
   separate authorities. People have no execution. A lead is a direct member of the
   home team and manages its subtree. The owner is a person; the host records
   ownership on that person's account, never in an employee field. An agent exists
   only through an approved hire, provisioned by product code; the built-in HR lead,
   created with the organization, is the one exception. No agent is started by
   hand, brought in from outside or bound to a run the office did not start.
3. **One open turn and one main process per agent.** A runnable agent selects one
   model profile (an ordered pool) and one model. Connections use private
   office-local bindings and HQ-issued permits. A verified idle process is reused only
   with the same launch digest. An unknown cleanup keeps the slot.
4. **Explicit work only.** A turn starts from an explicit instruction, the answer to a
   required ask ([ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)), or a
   recovery the office performs. The office does not choose work, queue messages into
   a busy turn, remind goals, extract memory or spawn specialists.
5. **Mechanism grants no authority.** Generated instructions are projections of the
   admitted identity, role and policy. An execution file, a note or a process document
   cannot grant a permission.
6. **People above an agent direct it.** A person is whatever place the administrator
   or owner gives them in the hierarchy: a member of a team an agent leads, the lead
   of a team of agents, or a lead of leads. There are no access roles, and any person
   can interact with any agent. An agent always replies to a person who messages it.
   It follows instructions from a person above it in the organization's hierarchy:
   the leads of its home team and of each ancestor team, up to the owner. A message
   from a person who is not above it, including one who reports to it, is a request: the agent answers it and
   may act on it, but where it conflicts with what its own chain asked, its chain
   wins, and between two people in its chain, the one higher up wins. The office
   gives the agent, with each delivered message, whether the sender is above it,
   computed from the current team tree; the agent does not work it out from names.
   This rule decides whose word the agent follows; it grants no permission
   (decision 5).

## 1. Control loop and durable records

The records are those of document 11; there is no separate supervisor schema. Durable
journals are authoritative and the database is a projection.

| Record | Facts and authority |
| --- | --- |
| Employee and execution configuration | Identity and home team; `execution.md` with `model_profile_id`, `model_id`, tools and prompt, required before the agent can run |
| Placement | HQ's monotonically increasing `placement_generation` and office; a replacement needs verified cleanup of the old process or an explicit machine fence |
| Profile, connection, binding | Ordered connections; the subscription identity shared across the organization; local private credentials; fresh authentication and model probes qualify a route |
| Turn, attempt, process | Frozen input, pool, model, workspace and policy; the actual connection, permit and usage per attempt; ownership mark and boot and start identity recorded before input |
| Execution checkpoint, tool receipt | Immutable admitted context and verified effect outcomes; an unknown in-flight effect forbids blind replay |
| Holds | Manual, security and each budget hold independently; `stop_pending` is a requested stop and `held` needs acknowledgement |
| Suspension on an ask | The suspended agent and the ask it waits on (ADR-0020 decision 9) |

Losing the database does not erase the protected sources; losing them while a child
may still exist returns `recovery_required`. Task workspaces follow document 04's
runtime tree with one cache per office and repository. Handoff, the end of a turn or
a process exit never closes a task or removes its worktree.

The control path:

1. Validate the caller's scope, the current configuration and placement, and every
   active hold. Admit no new work while recovering or stopping. A lost
   acknowledgement is resolved through the original operation receipt; an unknown
   response is not a failed mutation.
2. Choose eligible connections for the model in the frozen order and obtain the HQ
   permit. A local cache cannot grant quota or override stale authority. Silence is
   not interpreted as failure.
3. Record ownership before spawn and input, run a single turn, and record the actual
   route's usage and tool outcomes. Each connection is retried at most once per
   dispatch cycle.
4. On failure, stop and reconcile. Replay only with proof of no effects, or resume a
   verified compatible checkpoint with completed receipts. Missing evidence means
   `recovery_required`, keeping the slot, workspace and evidence until an explicit
   recovery.
5. Release the permit and process ownership only after verified terminal, idle or
   cleanup facts. An expired permit stays `release_pending` and consumes capacity. The
   office reconciles its journals and owned processes at startup before any
   replacement. A crash never triggers an automatic rerun.

**Health** is separate from desired state and presence. The office tracks the last
protocol event, the active operation, a known wait and its deadline, a probe result
and the process identity. No output is not by itself a stall. A tool with an explicit
deadline uses it; an operation of unknown duration enters a bounded investigation
rather than being assumed healthy. OS liveness does not prove progress. A runtime
with no probe says so and only verified transport and operation deadlines are used.
Retry counts and backoff survive office restarts, so a crash loop cannot reset its
allowance.

## 2. Pause, holds and offline offices

Pause is a durable HQ status change: HQ commits it even when the agent's office is
disconnected, and the runtime stop stays pending until the office acknowledges it.
Employee status, a manual pause and each budget hold are distinct. On reconnect the
office applies the current placement generation and every hold before new work.
While HQ is unreachable an office may still shut an agent down locally for safety,
without claiming authority it does not have; a later Resume supersedes stale stop
work. Resume releases only its own manual reason; nothing else releases a security
or budget hold. Raising, clearing or extending a budget releases only the holds that
change satisfies. Unknown usage stays explicitly incomplete; known amounts can still
reach limits, and release caps warn only.

A command for an office known to be offline is refused before admission with
`unavailable`, unless that operation states a queue rule, and nothing is enqueued.
The browser keeps the person's draft and offers a retry after reconnect. Once a
command has been forwarded, a lost acknowledgement is an unknown outcome: the
browser keeps the operation ID and shows Pending confirmation until a lookup or
retry with the same ID resolves it (EMP-3).

## 3. An agent waiting for a required answer

A required ask that times out suspends the agent
([ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)). The office tells the
agent, gives it the grace period to end its turn, stops its process and any process
it started that is still running, and records the suspension durably. A suspended
agent holds no turn, permit or subscription slot. The answer resumes the same session
through the normal start checks of §1; if one refuses, the agent stays suspended with
the reason shown and the start is retried.

## 4. Processes, standards and tool help

The [process design](processes.md#32-delivery-to-sessions-proc-7) owns what the agent
receives: non-overridable documents in full in a must-follow section, overridable
guidance as summaries with retrieval, and standards fetched when a process or task
calls for them. Help for `agent-os` is built-in guidance, given separately from
process text. No process edit invents commands or changes tool permissions.

## 5. Not in v1

Goals and goal reminders, automatic scheduling, generated native agent definitions
and private specialists, delegation to child agents, and memory checkpoints are not
part of v1. An agent that delegates inside its vendor tool does so within that tool's
own sandbox and the office's rules; the office grants a child nothing and tracks no
child job. EMP criteria about these features describe later work.

## 6. Validation

Prove the control loop with fixture-driven fake time and process events and isolated
temporary workspaces before real vendor tools are used. Cover long silent tools,
approval waits, crash loops, exhausted budgets, paused agents, a suspension across an
office restart, lost acknowledgements and incomplete cleanup. Tests never write to a
real organization.

## What the code does today

`crates/org-supervise` starts, stops and cleans up processes with ownership marks, and
`crates/org-runtime` defines the runtime contract. The office uses `org-supervise`
only to recover owned processes at startup (`apps/agentic-org-office/src/owned_recovery.rs`);
nothing launches an agent, and there is no permit or placement code
(implementation plan C3).
