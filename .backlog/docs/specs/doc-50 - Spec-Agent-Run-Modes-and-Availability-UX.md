---
id: doc-50
title: 'Spec: Agent Run Modes and Availability (UX)'
type: specification
created_date: '2026-11-21T00:20:00Z'
updated_date: '2026-11-21T00:20:00Z'
tags:
  - spec
  - ux
  - agents
  - run-modes
  - v1
---
---
kind: spec
title: "How agents run: explicit turns, supervision and recovery (UX)"
status: current
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
requested_by: owner
builds_on: [our-org-in-the-app, costs-and-insights-ux]
---

> [!NOTE] Review note (2026-11-21)
> Reviewed against ADR-0028, Organization Data contracts, and `doc-1` (Vision).
> - **Internal & ADR-0028 contradiction:** The intro and Decision 2 omit person DMs, mentions, and task assignments as wake triggers, contradicting **ADR-0028** (`FC-186`, `C10`) and this spec's own `RUN-3` (which includes ADR-0028). Reconcile intro/Decision 2 with ADR-0028 during `C3`/`C10` UI wiring.

# Spec · How agents run: explicit turns, supervision and recovery (UX)

The data contract owns the records:
[organization-data 04](../design/organization-data/04-agent-structure.md),
[11](../design/organization-data/11-model-profiles-and-agent-execution.md),
[14](../design/organization-data/14-model-providers-and-connection-pools.md),
[16](../design/organization-data/16-recovery-and-office-continuity.md) and
[17](../design/organization-data/17-budgets-and-controls.md). The
[session design](../design/agent-sessions.md) owns how a run goes. Cost visibility
follows [COST](costs-and-insights-ux.md) and document 17.

## The request (the user, 2026-11-08)

> Agents aren't always live. HR (or an agent in another project, like an SDM) doesn't need to be always live. It's a **stateful** agent that's almost **serverless**: it only runs when it's called, or on a long interval, like every 30 minutes: it comes up, checks its status, does its work and goes down again, unless it's called. We can also have **ephemeral** agents for other things. We need to support those kinds of agents.

v1 runs agents by explicit turns, and suspends and resumes an agent waiting on a
required answer ([ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)). One
thing besides a person's Run starts a turn for an idle agent: being addressed by another
agent (an ask, a hand-off or a mention by name), or receiving an answer to a question it
asked ([ADR-0023](../adr/0023-an-agents-ask-starts-a-turn.md)).
Automatic run modes (On call, Scheduled, Always on) and one-off agents are
[not in v1](#5-not-in-v1) for ordinary agents. The built-in HR lead is never kept
running: it runs only when called (a message to it, an `@hr` mention or a hire
request starts one turn), and it stops when the turn ends
([our organization in the app](our-org-in-the-app.md) §2a).

## 0. Decisions

1. **Identity and execution are separate.** Identity lives in `profile.md`, with
   exactly one home team and an optional role and reporting line. Execution lives in
   `execution.md`: one ordered model profile or pool, the canonical target model,
   tools and system prompt. People have no execution configuration. A home-team lead
   is a direct member; subtree management and the owner's account authority are
   separate from office placement.
2. **A turn is explicit.** Run submits an authorized logical turn using the current
   configuration and placement revisions and an approved task or workspace key.
   Messages and assignments do not start model work by themselves, with two stated
   exceptions: the answer to a required ask resumes a suspended agent (ADR-0020), and
   an ask, a hand-off or a mention by name from another agent, or an answer to the
   agent's own question, starts one turn for an idle agent, under an hourly limit
   (ADR-0023).
3. **One open turn and one main process per agent.** A compatible verified-idle
   process can be reused. The UI distinguishes the logical turn, its routed
   attempts and the process lifetime.
4. **Routing keeps the model.** A route selects fresh entitled connections for the
   same model in priority order and obtains an HQ permit shared across offices and
   pools. An unreachable coordinator blocks starts. Skipped candidates show reasons
   and are never shown as running attempts.
5. **Turn states** are Accepted, Running, Switching, Blocked, Recovery required,
   Succeeded, Failed and Cancelled. Cancellation and stop stay pending until
   verified. An unknown effect or cleanup keeps the slot; explicit recovery
   reconciles receipts and checkpoints. A process exit, turn success or handoff
   never closes the task or deletes its worktrees. Moving an agent to another office
   needs the old office's cleanup or verified fencing, then a new HQ placement
   generation. Cached presence alone proves neither launch authority nor a stop.
6. **Stopping an agent is Pause** (`our-org-in-the-app.md` DEAL-3). There is one way
   to stop an agent.

## 1. Where it shows

Execution shows the requested pool and model, the connection of the actual
attempt, turn status, the placed office and how fresh the status is; never a
credential value. Readiness distinguishes a missing binding, stale entitlement,
unsupported policy, coordinator outage and an exhausted pool. Known amounts are
shown with incomplete usage marked as such.

**Cost amounts** on turns (Activity rows, the Runs tab, RUN-5) show only to the
owner and to the project manager for their projects ([cost visibility](costs-and-insights-ux.md),
criterion COST-7, document 17). Others see the same rows without the amount. For a project
manager, a turn's amount is the part of its usage records attributed to their
projects (`costs-and-insights-ux.md` decision 4), labelled "in your projects" when
the turn also has other records; a turn with none shows no amount.

- **Quick view and profile header:** status and the current turn. Actions:
  **Run now** (with an optional note: "What should HR lead do?"), **Message**, and
  ⋯ › **Pause**.
- **Profile › Runs tab:** one row per turn: when · who started it (Run now by you,
  a message, mention, ask or assignment from a named employee, or the answer to a
  required ask) · duration · cost · outcome (the agent's one-line
  summary, or "Failed: <reason>") · **Open session log**. Filter by outcome.
- **Activity:** one row per turn, updated live ("running 2 min…" becomes
  "ran 4 min").

## 2. Flow · Setting up how an agent runs

The employee's home team and identity are separate from the execution pool and
model and from the office's local binding and readiness. A new employee without
`execution.md` is not runnable; the first runtime or model is never chosen
silently. The built-in HR lead is created with the organization in its top-level
team; every other agent is created only by a hire that someone who may hire has confirmed. A failed HR execution
does not block hiring with the manual form. Creating configuration does not start a
model turn.

**Advanced settings (sandbox and permissions):** under an agent's execution configuration
(`agent_execution_configs.permission_preset` in `execution.md`, never on a provider
connection), the UI presents user-friendly security presets mapped strictly to
`org-agent-runtime::PermissionMode` plus named `PermissionPolicy` rules—always subject
to `effective_permission_mode`, the fixed denies, and mandatory escalation denials—defaulting
to a sandboxed workspace with interactive approval routing (`safe_ask` → `Ask`, alongside
`read_only` → `Plan`, `workspace_edits` → `AcceptEdits`, and `autonomous_sandboxed` →
`AcceptEdits` + named workspace-scoped sandboxed command allow rules;
[ADR-0032](../adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md)).
The product never exposes a bypass / unrestricted skip-permissions switch, preserving
the `org-agent-runtime` no-bypass invariant.
A per-agent sandbox on/off switch in Advanced settings, on by default, is planned
(owner decision 2026-11-20, FC-189 follow-up); it is not built yet and adds no stored field until it is.
Browser-driven one-time subscription sign-in for
connections (Claude paste-code via `claude setup-token`, preferred, or `claude auth login --claudeai`, and Codex device-code via `codex login --device-auth`) is also defined
in ADR-0032 under ADR-0004 decision 3's narrow enrollment exception without exposing a
browser terminal for agent sessions.

## 3. States and edge cases

| Case | Required outcome |
| --- | --- |
| Run while an open turn exists | Busy; the original turn is kept, with no implicit queue and no second process. |
| Unknown command reply | Pending confirmation; the original operation ID and digest are resolved before another change. |
| Effect or cleanup unknown | Recovery required, keeping the workspace and ownership. No blind rerun after a crash or a lost transcript. |
| HQ accepted a manual Pause, office unavailable | Desired Pause with stop pending, and its freshness. It is not an observed stopped process. |
| Several holds | Manual, security and independent task, daily and monthly budget reasons are shown. Resume or one extension does not clear other reasons. |
| Budget limit known reached | Stop pending until the current turn completes or the worker acknowledges; only then held. Release caps only warn. |
| Usage partial or unavailable | Totals marked incomplete; unknown usage is never zero or guaranteed capacity. Unknown usage alone warns rather than blocks. |
| Cache or office loss | Protected receipts, usage, budget, extensions and execution authority are restored, then processes reconciled. Git alone cannot prove continuity. |
| Agent suspended on a required ask | Presence reads "Waiting for an answer from <person or agent>" with the time since; it holds no turn, permit or subscription slot. The answer resumes it in the same session; a refused start check keeps it suspended with the reason ([ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)). |
| Run fails to start (runtime missing, signed out, trust) | The turn shows **Failed** with the reason and a fix. For the HR lead, hiring doesn't wait: its chip says **Can't start**, and Hire works without it, with the manual form (`our-org-in-the-app.md` §2a, HRL-6). |

## 4. UX acceptance criteria (RUN)

- RUN-1 · Not in v1: an automatic run mode (Always on, On call, Scheduled) chosen at hire.
- RUN-2 · Not in v1: Asleep and Offline presence derived from a run mode.
- RUN-3 · A turn starts from an explicit Run, from the answer to a required ask, from an ask, a hand-off or a mention by name sent by another agent to an idle agent, or from an answer to a question the agent itself asked (ADR-0023, WAKE-1 to WAKE-7; ADR-0024), or from a reactive completion wake (`background_wake`) of the agent's own active background task or sub-agent ([ADR-0025](../adr/0025-agents-start-sub-agents.md) decision 6). By [ADR-0028](../adr/0028-an-agent-is-always-available.md) a direct message or a mention from a person, and a task assigned to the agent, start a turn the same way (AVAIL-1 to AVAIL-6). An announcement, `@here`, and an ordinary post or an undirected question in a channel start nothing. An agent that is not working shows as Available, whether or not its process is running.
- RUN-4 · A second Run is Busy while the original turn is open; no second process or implicit queue is created.
- RUN-5 · Turn history shows the logical turn, its actual routed attempts, durations, permitted amounts and completeness, and the concrete outcome; process reuse never merges attempt attribution.
- RUN-6 · Not in v1: idle sleep and a maximum busy time per run.
- RUN-7 · Not in v1: a schedule builder.
- RUN-8 · Not in v1: catching up missed scheduled runs.
- RUN-9 · A missing binding or sign-in, an unverified driver or model, unsupported policy, coordinator outage and pool exhaustion show concrete reasons and actions without an uncontrolled start. Message delivery state is kept independently.
- RUN-10 · HQ durably accepts an authorized manual Pause, with the office stop pending if the office is disconnected; the office applies current state and all independent holds before admitting turns, then acknowledges a verified stop. Resume clears only the manual reason; a stale Pause cannot override it, and unresolved budget or security holds remain (DEAL-3, SRV-28).
- RUN-11 · Not in v1: per-agent wake-up guidance.
- RUN-12 · Not in v1: one-off agents started from a job.
- RUN-13 · Not in v1: run-mode history and cost summary in a How it runs panel. Cost visibility rules apply wherever turn costs appear.
- RUN-14 · Each placed runnable agent has office-owned execution state independent of its model conversation. People have no execution controller; monitoring an idle or paused agent uses no model turns. Pause, offboarding and transfer reconcile owned main processes and their descendants before replacement, keep independent holds and keep task workspaces.
- RUN-15 · An agent can start sub-agents and background tasks inside its active session with its runtime's own tools; they share its limits, stay running across turns while people and agents keep messaging the agent, are recorded and costed as the employee, and are stopped and reaped on Pause, Stop, Offboard or stall timeout (ADR-0025, SUB-1 to SUB-7). Not in v1: memory forks.

## 5. Not in v1

- **Automatic run modes.** On call for anything else (asleep until any post or Run now wakes
  it; v1 has the rules of ADR-0023 and ADR-0028), Scheduled (wakes on a plain-words schedule, never cron) and Always on (kept
  running and restarted). With them: Asleep presence, wake history with triggers,
  idle sleep after 5 minutes, a maximum busy time (30 minutes in the agent's home,
  2 hours on a task), wake-up guidance, missed-run catch-up, and an "Upcoming" list.
  A scheduler would use the same execution admission and recovery service.
- **One-off agents:** a run of a role, not an employee, started from a job and shown
  on it as "Reviewer (one-off run)".
- **Memory forks** inside an employee's run. Sub-agents inside a turn are in v1
  ([ADR-0025](../adr/0025-agents-start-sub-agents.md)).
- **A model policy per slot** (main session, helpers, hard problems, reviews) with
  Organization → Role → Agent inheritance. Until then an agent's model is the one in
  its execution configuration; an unavailable model launches nothing and is never
  silently replaced, and pool failover never changes the chosen model.
