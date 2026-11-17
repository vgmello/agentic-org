---
kind: spec
title: "How agents run: always on, on call, scheduled, one-off (UX)"
status: for-review
revision: 14
owner: lead
created: "2026-11-08"
updated: "2026-11-15"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [our-org-in-the-app, costs-and-insights-ux]
---

# Spec · How agents run: always on, on call, scheduled, one-off (UX)

Revision 14 retains the deferred model-policy requirements below; revision 13 reconciled identity, execution, recovery and budget controls with
[organization-data 04](../design/organization-data/04-agent-structure.md),
[11](../design/organization-data/11-model-profiles-and-agent-execution.md),
[14](../design/organization-data/14-model-providers-and-connection-pools.md),
[16](../design/organization-data/16-recovery-and-office-continuity.md) and
[17](../design/organization-data/17-budgets-and-controls.md).
The selected target is explicit execution; automatic run modes/schedules, native
delegates and semantic memory remain earlier UX proposals. No runtime engine,
public wire command or migration is implemented by this spec. Existing v1
run_mode/schedule fields retain compatibility behavior and must be staged on
import without inventing a selected scheduling schema. See the
[authority map](../design/documentation-authority.md).

## The request (the user, 2026-11-08)

> Agents aren't always live. HR (or an agent in another project, like an SDM) doesn't need to be always live. It's a **stateful** agent that's almost **serverless**: it only runs when it's called, or on a long interval, like every 30 minutes: it comes up, checks its status, does its work and goes down again, unless it's called. We can also have **ephemeral** agents for other things. We need to support those kinds of agents.

Earlier revision history is available in
[the pre-reconciliation spec](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/specs/agent-run-modes-ux.md).
The operative cost rules are [COST](costs-and-insights-ux.md) and document 17,
which integrate compatible earlier requirements without adopting superseded rules.
One-off labels below are display proposals, without R- prefixes or a
new selected record counter. T- task references are scoped to project or team.

## 0. Decisions

**Selected execution foundation (services not yet implemented).** Identity lives
in profile.md with exactly one home team and optional role/reporting. Execution
lives separately in execution.md: one ordered model profile/pool, canonical target
model, tools and system prompt. People have no execution config. A home-team lead
must be a direct member; subtree management and owner account security authority
are separate from office placement.

A Run action submits an explicit authorized logical turn using current config and
placement revisions and an approved task/workspace key. One open turn and one
not-gone main process per agent; compatible verified-idle processes can be reused.
The UI distinguishes logical turn, routed attempts and process lifetime. A route
selects fresh entitled same-model connections in priority order and obtains an HQ
permit shared across offices/pools. An unreachable coordinator blocks starts.
Skipped candidates expose reasons without being shown as running attempts.

Show turn states Accepted, Running, Switching, Blocked, Recovery required,
Succeeded, Failed and Cancelled. Cancellation/stop remains pending until verified.
Unknown effect/cleanup retains the slot; explicit recovery must reconcile receipts
and checkpoints. Process exit/turn success/handoff never closes the task or deletes
its worktrees. Placement transfer needs old cleanup or verified fencing, then a new
HQ generation. Cached presence alone proves neither launch authority nor stop.

**Deferred run-mode proposal.** The eight decisions below preserve earlier UX;
they do not select automatic wakes, scheduling, persistent memory or one-off launch.
The first selected release offers explicit turns rather than defaulting new hires
to On call. Existing hand-started sessions remain clearly outside office control.

1. **If automatic modes are separately selected,** a managed agent would have a run mode chosen at hire and changeable later (profile › Settings › **How it runs**):

   | Mode | What it means | Keeps memory | Example |
   | --- | --- | --- | --- |
   | **Always on** | A session is kept running. If it stops, the office restarts it. | yes | ada during a phase push |
   | **On call** | Asleep until something needs it: a message, a task assigned or **Run now** (and a mention, if A45 agrees). It wakes, works, and goes back to sleep when idle. | yes | HR lead, the reviewer |
   | **Scheduled** | Wakes on a schedule (every 30 min, weekdays 9–18, daily at 08:00), does its check, and goes back to sleep. Also on call. | yes | an SDM checking project status, a nightly cost report |
   | **One-off** | Proposed separate job run from a role; no employee identity/home. Task workspace and recovery evidence still survive job end where required. | no | a one-off reviewer for a branch, a translation |

2. **Stateful doesn't mean running.** An On call or Scheduled agent keeps its home, memory, journal and identity. Only its session comes and goes. It's a full member: it's in the org chart, it can be messaged, assigned and lead, and its messages wait for it like anyone's.
3. **Asleep is a normal, calm state, not an error.** New status **Asleep** (gray `moon`), with when it wakes: "Asleep · wakes when called" or "Asleep · next run 14:30". **Offline** stays for an Always-on agent with no session (something is wrong, or it isn't started, like Riley today).
4. **Waking is visible.** Every wake is a **run** with its trigger, start, duration, cost and a one-line outcome, listed on the agent's **Runs** tab and in Activity: "HR lead woke up (you asked it to hire a QA engineer) · ran 4 min · ≈ $0.31 · went back to sleep".
5. **Calling an asleep agent just works.** Messages, assignments and Run now wake it. The caller sees "HR lead is asleep. It will wake up to answer (usually under a minute)", never a delivery failure. If it's already running, the message goes to the running session.
6. **Each wake starts from guidance** (`decisions-and-tech-debt-ux.md` §6.2, moment **Waking up**): by default "Read your inbox and backlog, do what's due, record what you did, then stop." Owners can customize it per agent (for example the SDM's "Check every open project's blocked tasks and report to ada").
7. **Sleep is decided by idleness, with a cap.** An On call or Scheduled agent goes back to sleep after it's been idle for **5 min** (no work and no new messages). A run is stopped at its **max run time**, which counts only busy time: **30 min** for a run in the agent's home and **2 h** for a run on a task (ada's agent-sessions design). The run is then marked "Stopped at the time limit". *(rev 2)* *(rev 6)* These limits are the organization's defaults (U20). The contract has no per-agent limit fields today, so every agent uses them and How it runs shows them read-only. A later selected policy could add per-agent limits and their editor; this proposal supplies neither target fields nor an implemented default timer.
8. **One-off agents don't join the org chart.** They're runs of a role, shown on the job (task, review, doc) and in Activity as "Reviewer (one-off run 12)" *(rev 10: was "one-off R-12"; R-n is the release key)*, with their cost on the job. The org chart only shows employees. A one-off runner/number counter requires a separate reviewed integration.

## 1. Where it shows

Selected explicit execution shows requested pool/model, actual attempt connection,
turn status, placed office and status freshness; never local credential values.
Readiness distinguishes missing binding, stale entitlement, unsupported policy,
coordinator outage and exhausted pool. Display incomplete usage alongside known
amounts. These are execution requirements, not a claim of an implemented panel.
The mode icons, automatic wake history and upcoming schedule surfaces below are
**deferred presentation proposals**.

*(rev 10)* **Cost amounts** on runs (Activity rows, the Runs tab, How it runs and RUN-5, RUN-13) show only to the owner and the project manager for their projects ([cost visibility](costs-and-insights-ux.md), COST-7, and document 17). Others see the same rows without the amount. *(rev 12)* For a project manager, a run's amount is the part of its usage records attributed to their projects (`costs-and-insights-ux.md` decision 4), labelled "in your projects" when the run also has other records; a run with none shows no amount, wherever it ran. How it runs' totals ("Last 7 days as on call") add up the same way, with the same label, and their run count and duration cover only runs with records in their projects (COST-7).

- **Org chart card and lists:** the status word gets the mode's detail: "Asleep · on call", "Asleep · next run 14:30", "Working · woken by ada's message". A small mode icon (`zap` Always on, `bell` On call, `clock` Scheduled) sits beside the status, always with the word in the tooltip and for screen readers ("HR lead, agent, on call, asleep").
- **Quick view and profile header:** **How it runs:** On call · "Last run 12:04 (4 min) · next: when called". Actions: **Run now** (primary for asleep agents, with an optional note: "What should HR lead do?"), **Message**, ⋯ › Change how it runs…. **Stop** (⋯) pauses the agent: it's the same as Pause (`our-org-in-the-app.md` DEAL-3), so there's one way to stop an agent, not two.
- **Profile › Runs tab:** one row per run: when · trigger (Schedule · Message from ada · Task T-A17 assigned · Run now by you · Mention) · duration · cost · outcome (the agent's one-line summary, or "Stopped at the time limit" / "Failed: <reason>") · **Open session log**. Filter by trigger and outcome.
- **Home › Working now** includes agents woken right now, with their trigger. **Upcoming** (collapsed) lists scheduled runs in the next 2 hours.
- **Activity:** a wake and a sleep are one row per run (not two), updated live ("running 2 min…" becomes "ran 4 min").

## 2. Flow · Choosing how an agent runs

Selected configuration separates the employee's required team/identity from
execution pool/model selection and local office binding/readiness. A new employee
without execution.md is not runnable; never silently choose the first runtime or
model. HR bootstrap must supply a valid home team rather than inventing a team or
creating a teamless employee. Failed HR execution does not block authorized manual
hiring. Creating config does not launch a model turn.

**Deferred mode editor.** The mockup/presets below are retained earlier scheduling
requirements; do not offer them as functional until a scheduler is separately
selected and implemented.

The proposed editor would appear at Hire Review and profile › Settings › **How it
runs** (a slide-over). Keep unavailable controls out of the current hiring flow;
there is no selected automatic On-call default for imported employees:

```
 How HR lead runs
 (•) On call      Sleeps until it's needed: a message, a task or Run now.
 ( ) Scheduled    Wakes on a schedule and when it's needed.
                  [Every 30 minutes ▾]  Only [Mon–Fri ▾] [09:00]–[18:00] [Europe/Dublin ▾]
 ( ) Always on    Keeps a session running and restarts it if it stops.
 ─────────────────────────────────────────────────────────────────────
 When it wakes    [Read your inbox and backlog, do what's due…  ] (guidance, editable)
 Defaults for all agents: sleeps after 5 min idle · stops after 30 min busy in its home, 2 h on a task
 About cost: on call and scheduled agents only cost while they run.
 Last 7 days as on call: 23 runs · 1 h 40 min · ≈ $4.10
                                                  [Cancel]  [Save]
```

- **Schedule presets:** Every 15 / 30 minutes · Every hour · Daily at… · Weekdays at… · Custom (a plain-words builder, never raw cron: "Every 2 hours on weekdays between 09:00 and 18:00"). The next 3 run times are shown under it.
- **Switching an Always-on agent to On call** while it's working asks: "ada is working now. Switch when this session ends, or now (the session stops)?"
- **One-off** isn't offered here (it isn't a member). It's offered where a job is started: Request review → **Reviewer: [the reviewer (on call) ▾ | Reviewer (one-off)]**, and on tasks › ⋯ › **Run a one-off agent…** (role, model, runtime, instruction).

## 3. States and edge cases

Selected control distinctions:

| Case | Required outcome |
| --- | --- |
| Explicit Run while an open turn exists | Busy; preserve the original turn, with no implicit queue or second process. |
| Unknown command reply | Pending confirmation; resolve the original operation ID/digest before another mutation. |
| Effect/cleanup unknown | Recovery required, preserving workspace/ownership. No blind rerun after crash or transcript loss. |
| HQ manual Pause accepted, office unavailable | Desired Pause with stop pending and freshness. It is not an observed stopped process. |
| Multiple holds | Show manual/security and independent task/daily/monthly budget reasons. Resume or one extension does not clear other reasons. |
| Budget limit known reached | stop_pending until current-turn completion/worker acknowledgement; only then held. Release caps only warn. |
| Usage partial/unavailable | Explicit incomplete totals; never treat unknown usage as zero or guaranteed capacity. Unknown usage alone warns rather than blocks. |
| Cache/office loss | Restore protected receipts, usage/budget/extension and execution authority, then reconcile processes. Git alone cannot prove continuity. |

**Deferred automatic-mode cases.** The following timing/queue behavior remains a
proposal and cannot override the selected outcomes above.

| Case | Behavior |
| --- | --- |
| Called while asleep | Wakes. The caller sees "Waking HR lead…", then normal replies. A wake taking over 60 s shows "HR lead is still starting…" with **Details**. |
| Called while running | Queued for the session handling it and delivered at its next turn boundary (ada's per-session routing). No second session. |
| A scheduled run is due while it's already running (a long run) | That run is skipped and noted on the Runs tab: "Skipped 14:30: still running from 14:00". |
| A run fails to start (runtime missing, signed out, trust) | The run shows **Failed** with the reason and a fix. A missing or signed-out runtime raises Needs you **at once** (nothing will work until it's fixed). Other failures do so after 3 failed wakes in a row: "HR lead can't wake up: Codex isn't signed in" with the fix and **Run now** to retry. Messages keep waiting. *(rev 9)* For the HR lead, hiring doesn't wait: its chip says **Can't start**, and Hire and Add agent record work without it (`our-org-in-the-app.md` §2a, HRL-6). |
| Paused agent (DEAL-3) | HQ durably records Paused; show stop pending at office until acknowledged. Applying Pause stops schedules/wakes and holds messages; an authorized Resume releases only its manual reason; a separately selected mode engine may restore its policy once every hold permits it. An offline runtime is not shown as stopped merely from HQ acceptance. |
| The Mac is asleep or the office is stopped at a scheduled time | The run happens when the office is back, once (missed runs don't pile up): "Ran at 09:12 instead of 08:00 (the Mac was asleep)". |
| Hit Max run time | The session is asked to wrap up (guidance "Time's up: record where you are"), then stopped after 1 min, and the run is marked. |
| An Always-on agent's session stops | Restarted after 10 s, with backoff. After 3 restarts in 10 min: Needs you "ada keeps stopping" with the last error. |
| One-off job finished | The result is posted on the job, and the run (cost, duration, outcome) is attached there. The job result, task worktree when applicable, usage and protected recovery evidence remain under their retention/closure rules. |

## 4. Our organization

**Earlier staffing/mode recommendations, deferred.** These are historical human
preferences, not selected runtime configuration or current presence evidence.

My recommended modes (U19, approved): **HR lead (built-in, `our-org-in-the-app.md` §2a): On call.** Reviewer: On call (woken by "Request review"). ada: Always on during phase pushes, otherwise On call. Builders (app-frontend-dev, server-dev, River, Remy, codex-dev): **On call**, woken by the tasks assigned to them, since the office caps live sessions (agent-sessions design: 4 live, at most 3 always on). agy-senior-dev: Offline while it has no runtime credits. Riley: **Offline**, as the user decided: not started and not on call, so nothing wakes him, and assigning him work warns "Riley is offline and won't pick this up". An SDM in another project: Scheduled, every 30 minutes on weekdays. *(rev 2: Remy was started by the user and is online.)*

## 5. UX acceptance criteria (RUN)

- RUN-1 · **Deferred automatic-mode editor/defaults.** *(rev 9)* Every agent the office runs has a run mode (Always on, On call, Scheduled), chosen in Hire's Review once the office runs agents (agents hired before then get On call) and changeable in profile › Settings › How it runs. An agent someone starts by hand (v1's run mode none) shows **Started by a person**, and the office never starts it.
- RUN-2 · **Deferred automatic sleep/presence labels.** An On call or Scheduled agent that isn't running shows **Asleep** with when it wakes ("wakes when called" or "next run <time>"), and an Always-on agent with no session shows **Offline**.
- RUN-3 · **Deferred implicit wake triggers; explicit turns are selected.** *(rev 3)* A message, an assignment or Run now (and a mention, pending A45) wakes an asleep agent. The caller sees "<name> is asleep. It will wake up to answer", and nothing fails.
- RUN-4 · **Selected single open-turn/process exclusion.** A second explicit Run is Busy while the original turn is open; no second process or implicit queue is created. Per-session delivery at a later turn boundary remains a deferred scheduler requirement.
- RUN-5 · **Selected explicit turn/attempt outcome history.** Show the logical turn, actual routed attempts, durations, permitted amounts/completeness and concrete outcome; process reuse never collapses attempt attribution. A future wake scheduler may group its trigger/activity history separately.
- RUN-6 · **Deferred sleep and busy-time policy/default editor.** *(rev 6)* An agent goes back to sleep after 5 min idle, and a run is stopped at the max busy time (30 min in its home, 2 h on a task), with the run marked. These are organization defaults, shown read-only in How it runs as "Defaults for all agents" while the contract has no per-agent limit fields.
- RUN-7 · **Deferred schedule builder.** The schedule builder uses plain words and presets, shows the next 3 run times, and never asks for cron.
- RUN-8 · **Deferred missed-run policy.** Missed scheduled runs (Mac asleep, office stopped) run once when the office is back and say so. A run due while the previous one is running is skipped and noted.
- RUN-9 · **Selected readiness failure visibility.** Missing binding/auth, unverified driver/model, unsupported policy, coordinator outage and pool exhaustion show concrete reasons/actions without an uncontrolled start. Automatic failed-wake counting and the three-wake escalation remain deferred; message delivery state is retained independently.
- RUN-10 · **Selected durable desired-state/observed-stop distinction; automatic wakes remain deferred.** HQ durably accepts authorized manual Pause with pending office stop if disconnected; the office applies current state and all independent holds before admitting explicit turns, then acknowledges verified stop. Resume clears only the manual reason; stale Pause cannot override it and unresolved budget/security holds remain (DEAL-3, SRV-28).
- RUN-11 · **Deferred wake guidance/editor.** The wake-up guidance is shown and editable in How it runs, per agent, and every wake starts with it.
- RUN-12 · **Deferred role-based one-off runner and short numbers; no R- identifier.** *(rev 11)* One-off agents can be started from a job (review, task). They show as "{Role} (one-off run {n})", for example "Reviewer (one-off run 12)": {n} is the run's per-office number (from 1, never reused, A108), with no key-style prefix; until runs have one, the label is "{Role} (one-off run)", and the job it's on identifies it. *(rev 11)* With more than one office, the label adds the office, "Reviewer (one-off run 12 · Studio Mac)", because numbers repeat across offices. *(rev 12)* Before runs carry a number, it's "Reviewer (one-off run · Studio Mac)". They never appear in the org chart (they aren't members), and their result, cost and duration are attached to the job.
- RUN-13 · **Deferred mode-history/editor; selected cost visibility still applies.** How it runs shows the agent's recent runs and cost, and explains that on-call and scheduled agents only cost while they run.

- RUN-14 · **Selected main-process supervision/recovery.** Each placed runnable agent has office-owned execution state independent of its model conversation. People have no execution controller; idle/paused monitoring uses no model turns. Pause/offboard/transfer reconcile owned main processes and descendants before replacement, preserve independent holds and retain task workspaces. Person-started sessions show supervision unavailable. Native jobs remain deferred.
- RUN-15 · **Deferred native delegates and memory forks; no exception to selected open-turn/main-process invariant.** *(new, rev 7)* Internal delegates and memory forks are owned work inside an employee's run, with shared aggregate limits and attributed usage, not members, independent run modes or role-based one-offs. A future internal-job contract must preserve the selected main-process slot and shared connection permits; unsupported concurrent helper isolation uses a serialized fallback or reports unavailable.

## 6. Open questions

### Deferred model-policy proposal

The earlier Models UX proposes owner-edited **Main session**, **Helpers**, **When it's hard** and **Reviews and security** slots, each with a model/effort and Organization → Role → Agent provenance. A future editor would show inherited read-only values, Override, Reset and Why; presets and a short escalation guideline would fill the table, with escalation timing left to agent judgment. Quick view/roster/Hire Review would show the effective summary and origin. This policy, its storage and helper orchestration remain unselected; role association alone supplies no runtime default, tools or credential authority.

Fast/Balanced/Frontier tiers, exact pinning, neutral efforts and presets need a versioned mapping/capability decision before implementation. The current catalog remains discovered under document 14. Preserve requested canonical model/policy when unavailable; show unverified/not entitled/unsupported readiness and require an explicit authorized configuration change. The older automatic inherited-model fallback and silent effort clamping are superseded. Pool failover never changes the chosen model or weakens policy.

Named helpers, escalation recording, per-slot price hints, activity-based estimates and Main/Helpers/Reviews cost breakdowns need selected runtime/helper/accounting evidence. Until then, show them unavailable rather than a functional four-slot editor or estimate. Any future implementation must retain the one-open-turn/main-process exclusion, shared permits, actual consumer attribution, aggregate/component deduplication and cost visibility. Revisioned changes apply to the next admitted turn, never mutate its frozen input. Target checks must prove unavailable models launch nothing and no role/model setting installs helper tools or exposes credentials.

These retained earlier answers document the deferred proposal and legacy v1
compatibility. A25/A26 do not authorize a target scheduler, automatic fresh-start
recovery or embedding execution in employee identity. A28 cleanup applies only to
safe throwaway inputs; task workspaces survive handoff/run end. A108 is a deferred
one-off display/counter proposal, not a selected T-/R- numbering change.

**For ada (architecture)**
- A45 · A **mention** (`@hr` in a channel or message) as a wake trigger, besides messages, asks, tasks, schedules and Run now. Is it in the agent-sessions triggers?
- ~~A25~~ · *Resolved (ada, W15 agent-sessions design):* the **office** supervises runs. A message, ask or task addressed to the agent at its office, a schedule, or the owner wakes it. The office starts the runtime in the agent's workspace with the wake guidance and **resumes** the last session when the runtime supports it and the session is recent (configurable window). Otherwise it starts fresh from the agent's memory and instruction files.
- ~~A26~~ · *Resolved (ada):* `run_mode` and `schedule` live on AgentProfile (org data at HQ). The agent's office fires the schedule under the login service, so schedules work with the app closed.
- ~~A27~~ · *Resolved (ada):* derived, not stored. Asleep = an on-call or scheduled agent with no live run. Offline = an always-on agent with no live session.
- ~~A28~~ · *Resolved (ada):* a one-off is a Run of a role or template with a temporary workspace, attached to its work item, with no Member. The workspace is cleaned up, and the transcript and outcome stay on the run record (so its **Open session log** link keeps working).
- ~~A108~~ · *Resolved (Rowan, 2026-11-13; rev 11):* {n} is a per-office run number, allocated from 1 and never reused when the one-off run is created, and stored on the run. The label is computed: "{Role} (one-off run {n})", or "{Role} (one-off run)" until runs carry it. With more than one office it adds " · {office}" (my call: numbers repeat across offices). ADR-0003 l.246 and `docs/design/agent-sessions.md` l.426 now use this label.

**For the user**
- ~~U19~~ · *Resolved (the user, 2026-11-08): as recommended.* The modes I recommend for our team (§4): HR lead and the reviewer On call, builders On call, ada Always on during pushes. OK?
- ~~U20~~ · *Resolved (the user, 2026-11-08): as recommended.* Defaults: idle 5 min, max busy time 30 min in the agent's home and 2 h on a task (ada). OK?
