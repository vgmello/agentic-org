---
kind: spec
title: "How agents run: always on, on call, scheduled, one-off (UX)"
status: for-review
revision: 8
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-10"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [our-org-in-the-app, costs-and-insights-ux]
---

# Spec · How agents run: always on, on call, scheduled, one-off (UX)

Extends `our-org-in-the-app.md` (agent record, statuses, Activity) and `costs-and-insights-ux.md` (sessions and cost).

## The request (the user, 2026-11-08)

> Agents aren't always live. HR (or an agent in another project, like an SDM) doesn't need to be always live. It's a **stateful** agent that's almost **serverless**: it only runs when it's called, or on a long interval, like every 30 minutes: it comes up, checks its status, does its work and goes down again, unless it's called. We can also have **ephemeral** agents for other things. We need to support those kinds of agents.

**Revision 6 (2026-11-08): limits are organization defaults (ada, from the How it runs build).** The idle and busy limits are the organization's defaults (U20: 5 min idle, 30 min busy in the agent's home, 2 h on a task), and the contract has no per-agent fields today, so How it runs shows them read-only as "Defaults for all agents" (decision 7, §2). `agent-sessions.md` decision 8 allows per-agent limits later; if they arrive, they become editable here. **Criteria rewritten (same ID):** RUN-6 *(rev 6)*.

**Revision 5 (2026-11-08): review fixes.** The built-in hiring agent is always called "HR lead" (`our-org-in-the-app.md` §2a, HRL-1): the mode table's example, How HR lead runs, the wake and failure messages, the screen-reader label, the Run now note and U19 say "HR lead". A45's mention example uses the handle, `@hr`. The user's request is quoted as said. No criteria changed.

**Revision 4 (2026-11-08): aligned with ada's W7 model.** Item numbers on the Runs tab follow `work-items-ux.md` rev 6 and carry the project key (`Task ac-T-17 assigned`). No criteria changed.

**Revision 3 (2026-11-08): review fixes.** A mention as a trigger is pending A45. A message to a running agent is queued for the session handling it and delivered at its next turn boundary. An agent the office doesn't run shows **Started by a person** (v1's run mode none). Schedules have a time zone. RUN-1, RUN-3 and RUN-4 rewritten.

**Revision 2 (2026-11-08): aligned with ada's agent-sessions design (rev 3).** Max run time counts **busy time**: 30 min for runs in the agent's home, 2 h for runs on a task. A missing or signed-out runtime raises Needs you at once. One-offs show as "<role> (one-off R-n)" with no member. Stop means Pause. Recommended modes respect the live-session cap. Remy is online (the user started him), and Riley stays Offline. RUN-6, RUN-9 and RUN-12 rewritten.

**Revision 7 (2026-11-09): owner-requested employee supervision.**
[`employee-supervision-ux.md`](employee-supervision-ux.md) adds deterministic
supervision, goal delivery, private delegates and memory helpers. These do not add
run modes, hires or a second main session. Limits remain organization defaults;
RUN-14/15 clarify how internal work follows lifecycle and accounting.

## 0. Decisions

1. **Every agent has a run mode,** chosen at hire and changeable later (profile › Settings › **How it runs**):

   | Mode | What it means | Keeps memory | Example |
   | --- | --- | --- | --- |
   | **Always on** | A session is kept running. If it stops, the office restarts it. | yes | ada during a phase push |
   | **On call** | Asleep until something needs it: a message, a task assigned or **Run now** (and a mention, if A45 agrees). It wakes, works, and goes back to sleep when idle. | yes | HR lead, the reviewer |
   | **Scheduled** | Wakes on a schedule (every 30 min, weekdays 9–18, daily at 08:00), does its check, and goes back to sleep. Also on call. | yes | an SDM checking project status, a nightly cost report |
   | **One-off** | Started for one job from a role, with no lasting memory or home. It reports the result on the job and is gone. | no | a one-off reviewer for a branch, a translation |

2. **Stateful doesn't mean running.** An On call or Scheduled agent keeps its home, memory, journal and identity. Only its session comes and goes. It's a full member: it's in the org chart, it can be messaged, assigned and lead, and its messages wait for it like anyone's.
3. **Asleep is a normal, calm state, not an error.** New status **Asleep** (gray `moon`), with when it wakes: "Asleep · wakes when called" or "Asleep · next run 14:30". **Offline** stays for an Always-on agent with no session (something is wrong, or it isn't started, like Riley today).
4. **Waking is visible.** Every wake is a **run** with its trigger, start, duration, cost and a one-line outcome, listed on the agent's **Runs** tab and in Activity: "HR lead woke up (you asked it to hire a QA engineer) · ran 4 min · ≈ $0.31 · went back to sleep".
5. **Calling an asleep agent just works.** Messages, assignments and Run now wake it. The caller sees "HR lead is asleep. It will wake up to answer (usually under a minute)", never a delivery failure. If it's already running, the message goes to the running session.
6. **Each wake starts from guidance** (`decisions-and-tech-debt-ux.md` §6.2, moment **Waking up**): by default "Read your inbox and backlog, do what's due, record what you did, then stop." Owners can customize it per agent (for example the SDM's "Check every open project's blocked tasks and report to ada").
7. **Sleep is decided by idleness, with a cap.** An On call or Scheduled agent goes back to sleep after it's been idle for **5 min** (no work and no new messages). A run is stopped at its **max run time**, which counts only busy time: **30 min** for a run in the agent's home and **2 h** for a run on a task (ada's agent-sessions design). The run is then marked "Stopped at the time limit". *(rev 2)* *(rev 6)* These limits are the organization's defaults (U20). The contract has no per-agent limit fields today, so every agent uses them and How it runs shows them read-only. `agent-sessions.md` decision 8 allows per-agent limits later; if they're added, How it runs offers them as editable fields under the defaults.
8. **One-off agents don't join the org chart.** They're runs of a role, shown on the job (task, review, doc) and in Activity as "Reviewer (one-off R-12)", with their cost on the job. The org chart only shows members.

## 1. Where it shows

- **Org chart card and lists:** the status word gets the mode's detail: "Asleep · on call", "Asleep · next run 14:30", "Working · woken by ada's message". A small mode icon (`zap` Always on, `bell` On call, `clock` Scheduled) sits beside the status, always with the word in the tooltip and for screen readers ("HR lead, agent, on call, asleep").
- **Quick view and profile header:** **How it runs:** On call · "Last run 12:04 (4 min) · next: when called". Actions: **Run now** (primary for asleep agents, with an optional note: "What should HR lead do?"), **Message**, ⋯ › Change how it runs…. **Stop** (⋯) pauses the agent: it's the same as Pause (`our-org-in-the-app.md` DEAL-3), so there's one way to stop an agent, not two.
- **Profile › Runs tab:** one row per run: when · trigger (Schedule · Message from ada · Task ac-T-17 assigned · Run now by you · Mention) · duration · cost · outcome (the agent's one-line summary, or "Stopped at the time limit" / "Failed: <reason>") · **Open session log**. Filter by trigger and outcome.
- **Home › Working now** includes agents woken right now, with their trigger. **Upcoming** (collapsed) lists scheduled runs in the next 2 hours.
- **Activity:** a wake and a sleep are one row per run (not two), updated live ("running 2 min…" becomes "ran 4 min").

## 2. Flow · Choosing how an agent runs

At hire (Review step) and in profile › Settings › **How it runs** (a slide-over):

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

| Case | Behavior |
| --- | --- |
| Called while asleep | Wakes. The caller sees "Waking HR lead…", then normal replies. A wake taking over 60 s shows "HR lead is still starting…" with **Details**. |
| Called while running | Queued for the session handling it and delivered at its next turn boundary (ada's per-session routing). No second session. |
| A scheduled run is due while it's already running (a long run) | That run is skipped and noted on the Runs tab: "Skipped 14:30: still running from 14:00". |
| A run fails to start (runtime missing, signed out, trust) | The run shows **Failed** with the reason and a fix. A missing or signed-out runtime raises Needs you **at once** (nothing will work until it's fixed). Other failures do so after 3 failed wakes in a row: "HR lead can't wake up: Codex isn't signed in" with the fix and **Run now** to retry. Messages keep waiting. |
| Paused agent (DEAL-3) | HQ durably records Paused; show stop pending at office until acknowledged. Applying Pause stops schedules/wakes and holds messages; current Resume restores the mode. An offline runtime is not shown as stopped merely from HQ acceptance. |
| The Mac is asleep or the office is stopped at a scheduled time | The run happens when the office is back, once (missed runs don't pile up): "Ran at 09:12 instead of 08:00 (the Mac was asleep)". |
| Hit Max run time | The session is asked to wrap up (guidance "Time's up: record where you are"), then stopped after 1 min, and the run is marked. |
| An Always-on agent's session stops | Restarted after 10 s, with backoff. After 3 restarts in 10 min: Needs you "ada keeps stopping" with the last error. |
| One-off job finished | The result is posted on the job, and the run (cost, duration, outcome) is attached there. Nothing remains in the org. |

## 4. Our organization

My recommended modes (U19, approved): **HR lead (built-in, `our-org-in-the-app.md` §2a): On call.** Reviewer: On call (woken by "Request review"). ada: Always on during phase pushes, otherwise On call. Builders (app-frontend-dev, server-dev, River, Remy, codex-dev): **On call**, woken by the tasks assigned to them, since the office caps live sessions (agent-sessions design: 4 live, at most 3 always on). agy-senior-dev: Offline while it has no runtime credits. Riley: **Offline**, as the user decided: not started and not on call, so nothing wakes him, and assigning him work warns "Riley is offline and won't pick this up". An SDM in another project: Scheduled, every 30 minutes on weekdays. *(rev 2: Remy was started by the user and is online.)*

## 5. UX acceptance criteria (RUN)

- RUN-1 · *(rev 3)* Every agent the office runs has a run mode (Always on, On call, Scheduled), chosen at hire and changeable in profile › Settings › How it runs. An agent someone starts by hand (v1's run mode none) shows **Started by a person**, and the office never starts it.
- RUN-2 · An On call or Scheduled agent that isn't running shows **Asleep** with when it wakes ("wakes when called" or "next run <time>"), and an Always-on agent with no session shows **Offline**.
- RUN-3 · *(rev 3)* A message, an assignment or Run now (and a mention, pending A45) wakes an asleep agent. The caller sees "<name> is asleep. It will wake up to answer", and nothing fails.
- RUN-4 · *(rev 3)* A message to a running agent is queued for the session handling it and delivered at its next turn boundary, and a second session is never started.
- RUN-5 · Each wake is one run on the Runs tab and one Activity row, with trigger, duration, cost and outcome, updated live.
- RUN-6 · *(rev 6)* An agent goes back to sleep after 5 min idle, and a run is stopped at the max busy time (30 min in its home, 2 h on a task), with the run marked. These are organization defaults, shown read-only in How it runs as "Defaults for all agents" while the contract has no per-agent limit fields.
- RUN-7 · The schedule builder uses plain words and presets, shows the next 3 run times, and never asks for cron.
- RUN-8 · Missed scheduled runs (Mac asleep, office stopped) run once when the office is back and say so. A run due while the previous one is running is skipped and noted.
- RUN-9 · *(rev 2)* A missing or signed-out runtime raises a Needs you item at once with the fix. Other failures do so after three failed wakes in a row. Waiting messages are kept.
- RUN-10 · HQ durably accepts Pause with pending office stop if disconnected; the office applies current state before admitting schedules/wakes and acknowledges stopping. Resume restores the mode without stale Pause delivery overriding it (DEAL-3, SRV-28).
- RUN-11 · The wake-up guidance is shown and editable in How it runs, per agent, and every wake starts with it.
- RUN-12 · *(rev 2)* One-off agents can be started from a job (review, task). They show as "<role> (one-off R-n)", never appear in the org chart (they aren't members), and their result, cost and duration are attached to the job.
- RUN-13 · How it runs shows the agent's recent runs and cost, and explains that on-call and scheduled agents only cost while they run.

- RUN-14 · *(new, rev 7)* Every office-managed employee has the supervision state described by EMP-1..14. Sleeping/paused records persist without model monitoring turns. Pause, offboard and placement changes stop/reconcile internal delegates, helpers and subprocesses; recovery never overrides Pause. Person-started sessions show supervision unavailable.
- RUN-15 · *(new, rev 7)* Internal delegates and memory forks are owned work inside an employee's run, with shared aggregate limits and attributed usage, not members, independent run modes or role-based one-offs. The main-session lock stays; unsupported concurrent helper isolation uses a serialized fallback or reports unavailable.

## 6. Open questions

**For ada (architecture)**
- A45 · A **mention** (`@hr` in a channel or message) as a wake trigger, besides messages, asks, tasks, schedules and Run now. Is it in the agent-sessions triggers?
- ~~A25~~ · *Resolved (ada, W15 agent-sessions design):* the **office** supervises runs. A message, ask or task addressed to the agent at its office, a schedule, or the owner wakes it. The office starts the runtime in the agent's workspace with the wake guidance and **resumes** the last session when the runtime supports it and the session is recent (configurable window). Otherwise it starts fresh from the agent's memory and instruction files.
- ~~A26~~ · *Resolved (ada):* `run_mode` and `schedule` live on AgentProfile (org data at HQ). The agent's office fires the schedule under the login service, so schedules work with the app closed.
- ~~A27~~ · *Resolved (ada):* derived, not stored. Asleep = an on-call or scheduled agent with no live run. Offline = an always-on agent with no live session.
- ~~A28~~ · *Resolved (ada):* a one-off is a Run of a role or template with a temporary workspace, attached to its work item, with no Member. The workspace is cleaned up, and the transcript and outcome stay on the run record (so its **Open session log** link keeps working).

**For the user**
- ~~U19~~ · *Resolved (the user, 2026-11-08): as recommended.* The modes I recommend for our team (§4): HR lead and the reviewer On call, builders On call, ada Always on during pushes. OK?
- ~~U20~~ · *Resolved (the user, 2026-11-08): as recommended.* Defaults: idle 5 min, max busy time 30 min in the agent's home and 2 h on a task (ada). OK?
