---
kind: spec
title: "Costs, tokens, time and insights (UX)"
status: for-review
revision: 4
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-08"
requested_by: owner
reviewers: [ac-staff-engineer-ada, codex-dev, owner]
builds_on: [work-items-ux, our-org-in-the-app, decisions-and-tech-debt-ux]
---

# Spec · Costs, tokens, time and insights (UX)

Extends `work-items-ux.md` (tasks, bugs), `our-org-in-the-app.md` (sessions, runtimes, Activity) and `decisions-and-tech-debt-ux.md` (debt docs).

## The request (the user, 2026-11-08)

> For each project we invoke the agents. We need to see how much the sessions cost, the number of tokens, and insights: how long tasks take, the speed of tasks, the cost of a particular task and overall project costs. We need a structure for this.

> *(rev 2, the user)* No clients: the point is to see **what each project has cost (the investment)**. Immediate figures are fine. Don't worry about plans: value everything by its tokens at **API prices**, in **US dollars**.

**Revision 4 (2026-11-08): aligned with ada's W7 model.** Working time isn't tracked yet (it needs session tracking). Until it is, the time split shows waiting on you, blocked and **the rest**, never idle, so untracked work is never counted as idle (decision 5, §2). Waiting on you counts questions to you, Fixed bugs you reported, scoped requests, triage suggested to you and decisions waiting for your acceptance. An agent sets its current task with `org-agent task current <id>` (was `status --task`, C14 resolved). Task numbers follow `work-items-ux.md` rev 6 (`T-13`). **Criteria rewritten (same ID):** TIME-1. **Added:** TIME-7.

**Revision 3 (2026-11-08): reviewer fix.** A39: organization totals need session usage from every office (§7).

**Revision 2 (2026-11-08):** clients removed (decision 6, §3), plans removed: every cost is tokens at API prices (decision 3), US dollars, budgets per project only, and an **Investment** figure (the project's total to date) leads the project's Insights. U16–U18 resolved. COST-5, INS-5 and BUD-1 rewritten.

**The questions it must answer in one or two clicks:** What did this task, bug fix or project cost? What has this project cost so far (its investment)? Where is the money going (which project, team, area, agent or model)? How long does work take, and where does it wait? Is anything unusually expensive or slow? Are we within budget?

## 0. Decisions

1. **The unit is the session.** Every agent session records its runtime, model, tokens (input, output, cache read, cache write), cost, start, end, active time, and **what it worked on**. Everything else (task, bug, project, team, agent) is a roll-up of sessions.
2. **Every number says where it came from.** Cost is **Reported** (the runtime said so), **Estimated** (tokens × the price list, dated) or **Not available** (the runtime gives neither). It shows as "$4.12", "≈ $4.12" or "—" with the reason on hover. A blank is never shown as $0.
3. **Cost is always tokens at API prices, in US dollars** *(rev 2, the user)*, whatever plan the runtime is on, so work compares fairly across runtimes and projects. Plans aren't shown or tracked. When a runtime reports a cost at API prices, that figure is used (Reported). Otherwise the app multiplies tokens by the dated price list (≈).
4. **A session counts toward the task it worked on.** In order: the task the session was started on; otherwise the task the agent set as current (`org-agent task current T-17`, also set when the agent starts a task, C14); otherwise the agent's only In progress task. Anything else is **Not linked to a task**: shown, counted in the project and agent totals, and never guessed onto a task. When an agent switches task mid-session, the session splits at the switch.
5. **Time has four honest parts** for every task and bug: **Lead time** (created → done) and **Cycle time** (started → done), split into **Working time** (sessions active on it), **Waiting on you** (a question to the owner, a Fixed bug the owner reported, a scoped request, triage suggested to the owner, a task Blocked waiting for the owner's decision on a proposed ADR, only while that's its blocker (ada Q11)), **Blocked** (Blocked status) and idle. Working time isn't tracked yet *(rev 4)*: until it is, the split shows **the rest** in place of working and idle, and untracked working time is never counted as idle. All of it is derived from the item's state changes (ada, A22). Speed is shown with medians, never averages (one stuck task distorts an average).
6. **A project's investment is its headline** *(rev 2: no clients)*: the total cost to date (and working time and items done) is the first figure on the project's Insights and on the Projects list, with this month beside it. Figures can be immediate (computed when shown), with no need for nightly reports.
7. **Insights point at something to do,** each with a link: "T-13 cost 4× the median bug fix ($18 vs $4.50) · Open", "Agents waited 6 h for your answers this week · See questions", "Payments has 40% of this month's cost". They're not a wall of charts.
8. **Budgets are optional and gentle.** A monthly budget per project warns at 80% (Home) and at 100% (Needs you). Nothing stops agents, since stopping is the owner's call (Pause, dogfood DEAL-3).
9. **Copilot runs are costs too** (smart features), shown as "Copilot" per project, so smart features are never hidden spend.

## 1. Where costs show (small to large)

| Place | What it shows |
| --- | --- |
| **Session row** (agent profile › Sessions, task page) | runtime · model · duration (active / total) · tokens in / out / cache · cost (Reported / ≈ / —) · the task |
| **Task, bug and debt pages** (side column, **Cost and time**) | cost · tokens · sessions (n) · lead time split into waiting on you · blocked · the rest (working time once it's tracked), with the comparison "Typical for a task in Office: $3–6, 2–5 h" |
| **Project › Insights tab** (new tab, after Docs) | this month and last: cost, tokens, working time, tasks and bugs done · cost by team, area, agent, model and kind (task, bug, debt payback, copilot, retro, report, not linked; was: task, bug, debt, copilot, not linked) · speed (median lead time, working time, waiting on you, by kind) · the insights list · the budget bar |
| **Insights** (nav, org-wide) | the same, across projects · a Projects table (investment to date, this month, last month, change, working time, items done) · Export |
| **Projects list** | an **Investment** column (total to date, US dollars) |
| **Agent profile › Overview** | this week's cost, working time and tasks done, and cost per task done, vs the team median |
| **Home** | one line: "This month: ≈ $412 · 63 tasks done · 30 h waiting on you" with **Insights** · budget warnings |

```
 Projects › Agentic Org › Insights                     [This month ▾]  [Export ▾]
 ┌ Cost ─────────────┐ ┌ Tokens ──────────┐ ┌ Working time ──┐ ┌ Done ──────────────┐
 │ ≈ $412  ▲ 18%     │ │ 61.2 M           │ │ 96 h           │ │ 63 tasks · 21 bugs │
 │ budget $600 ▓▓▓▓░ │ │ 74% cache reads  │ │30 h waiting on │ │ 4 debt paid        │
 └───────────────────┘ └──────────────────┘ │ you            │ └────────────────────┘
                                            └────────────────┘
 Insights
  ⚠ T-13 "Disable Pay while submitting" cost 4× the median task in App ($18 vs $4.50)  [Open]
  ⓘ Agents waited 30 h for your answers this month, most on ada's questions (19 h)        [See]
  ⓘ codex-dev's tasks finish 2× faster than the team median this month                  [Open]
 By team            Cost       Working   Tasks  Cost/task   Median lead time
  Office            ≈ $171     41 h      22     ≈ $7.80     1.6 d
  App               ≈ $132     30 h      25     ≈ $5.30     1.1 d
  Agent tooling       $68 *    17 h      12       $5.70     0.9 d
  Design            ≈ $29       6 h       4     ≈ $7.20     0.4 d
  Not linked        ≈ $12       2 h       —        —          —
  * includes 1 runtime with no cost data (Antigravity): tokens only
```

- **Breakdowns** switch with pills: Team · Area · Agent · Model · Kind. Each row opens a filtered list of the sessions or items behind it, so every total can be traced to sessions.
- **Charts:** one trend line (cost per day, with the budget line) and bars for the breakdown, following the dataviz rules: labelled values, no color-only meaning, and an accessible table toggle.
- **Periods:** This week · This month · Last month · Custom · All time. Comparisons are always against the previous period of the same length.

## 2. Flow · Task cost and time (the page block)

```
 Cost and time
 ≈ $6.40 · 1.9 M tokens · 3 sessions                  [Sessions ▾]
 Lead time 1 d 4 h  =  waiting on you 3 h · blocked 0 · the rest 1 d 1 h (working time isn't tracked yet)
 Typical for App: ≈ $3–6 · 1–2 d
```

- **The rest** *(rev 4)* is the lead time that wasn't waiting on you or blocked. Working time isn't tracked yet, so the rest can't be split into working and idle, and untracked working time is never shown as idle. Once working time is tracked, the line becomes "working 2 h 10 m · waiting on you 3 h · blocked 0 · idle 22 h 50 m", where **idle** is the time nobody worked on it and it wasn't waiting on anyone. Showing it stops "1 day" from reading as "1 day of work".
- **Sessions ▾** lists them (agent, runtime, model, start, active time, tokens, cost). A session split across tasks shows "part of a session · 40 m of 2 h".
- **States:** no sessions yet ("No agent time on this yet") · only unattributed sessions nearby ("River had 2 sessions not linked to a task while this was in progress. **Link them…**") · the runtime gives no usage ("Antigravity doesn't report usage, so cost and tokens show —.").

## 3. Flow · Investment and export

- **Project › Insights** opens with **Investment**: "Agentic Org has cost ≈ $1,240 so far (since 7 Nov) · 212 h of agent work · 180 tasks and 64 bugs done", then this month.
- **Insights › Projects** (org-wide): project · investment to date · this month · last month · change · working time · items done · budget.
- **Export ▾:** **CSV** (one row per session, with project, task, agent, runtime, model, tokens, cost, the cost source and times) · **Status report for the month** (`project-status-ux.md`, which replaces a separate monthly report; a Markdown doc saved to the project's `docs/reports/`, with totals, the breakdowns and the insights).

## 4. Flow · Budgets

- **Project Settings › Budget:** a monthly amount in US dollars with "Warn me at 80%" (on by default).
- **80%:** a yellow line on Home: "Agentic Org has used 80% of its $600 budget with 9 days left." **Insights**.
- **100%:** a Needs you item: "Agentic Org is over budget ($612 of $600)." **Pause agents…** (dogfood DEAL-3, choose who) · **Raise budget…** · **Dismiss for this month**.
- **Estimated costs count toward budgets,** and the bar says "≈ includes estimates".

## 5. Data sources the UX depends on (for ada and codex-dev)

| Runtime | What it gives (to confirm, C15) | Shown as |
| --- | --- | --- |
| Claude Code, print mode (`claude -p`, copilot) | cost and usage per run | Reported |
| Claude Code, interactive sessions | token usage per message in the transcript; no cost | ≈ (tokens × the price list) |
| Codex (`codex exec`, sessions) | token usage | ≈ |
| Antigravity | unknown | — (cost and tokens) until it reports |

**Prices:** a dated price list per model, in Settings › Costs ("Prices from 2026-11-01 · **Update**"), editable for custom rates. Changing it re-estimates past months only on request ("Re-estimate past costs?").

## 6. UX acceptance criteria

**Costs (COST)**
- COST-1 · Every session row shows runtime, model, active and total duration, tokens (in, out, cache) and cost with its source: Reported ($), Estimated (≈ $) or Not available (— with the reason on hover).
- COST-2 · Task, bug and debt pages show cost, tokens, number of sessions and the sessions behind them.
- COST-3 · A session counts toward a task only by the rule in decision 4. Others show as "Not linked to a task" and are counted in the project and agent totals.
- COST-4 · Project and organization totals equal the sum of their sessions, and every breakdown row opens the sessions behind it.
- COST-5 · *(rev 2)* Every cost is in US dollars at API prices, whatever the runtime's plan, and plans aren't shown.
- COST-6 · Copilot runs show as "Copilot" in project costs.

**Time and speed (TIME)**
- TIME-1 · *(rev 4)* Tasks and bugs show lead time split into working, waiting on you, blocked and idle, and the parts add up to the lead time. Until working time is tracked, the split shows waiting on you, blocked and "the rest", with "working time isn't tracked yet", and untracked working time is never counted as idle.
- TIME-2 · Speed figures (lead time, working time, cost per task) are medians, labelled "median".
- TIME-3 · Each task and bug shows the typical range for its team and kind beside its own numbers.
- TIME-4 · *(new)* Every task and bug shows cycle time (started → done) beside lead time, both derived from state changes, and open items say "so far".
- TIME-5 · *(new)* Task and bug lists can show sortable Cycle time and Lead time columns.
- TIME-6 · *(new)* Insights shows median lead time and cycle time per project, area, team and assignee for the period.
- TIME-7 · *(new, rev 4)* Waiting on you counts questions to the owner, Fixed bugs the owner reported (waiting for the owner's check), scoped requests waiting for approval, triage suggested to the owner, and tasks Blocked waiting for the owner's decision on a proposed ADR (only while that decision is the task's blocker, ada Q11).

**Insights (INS)**
- INS-1 · The project's Insights tab shows cost, tokens, working time and items done for the period, with the change from the previous period.
- INS-2 · Breakdowns by team, area, agent, model and kind are available, each with a table view for screen readers.
- INS-3 · Insights are sentences with a link to the items behind them. At least these exist: an outlier task or bug (≥ 3× its team's median), waiting on you this period, and a cost concentration (one area or team ≥ 40%).
- INS-4 · A runtime with no usage data is marked in every total it affects, never counted as zero.
- INS-5 · *(rev 2)* The project's Insights leads with its investment to date. The org-wide Insights lists projects by investment, and Export gives a per-session CSV and a monthly report doc in `docs/reports/`.

**Budgets (BUD)**
- BUD-1 · *(rev 2)* A project can have a monthly budget in US dollars. At 80% Home shows a warning line, and at 100% a Needs you item offers Pause agents, Raise budget and Dismiss for this month.
- BUD-2 · Budgets never stop agents by themselves.

## 7. Open questions

**For codex-dev and ada**
- ~~A39~~ · *Resolved (ada):* phase 1 is HQ only, so HQ's `office.db` has everything. In phase 2, `session_usage` syncs to HQ as work data on the per-origin streams (like tasks), so HQ shows organization totals without querying offices. This changes `docs/design/offices-hq-and-satellites.md`, which says sessions aren't copied, and ada updates it.
- ~~C14~~ · *Resolved (ada W7):* the agent sets its current task with `org-agent task current <id>` (`--clear` clears it), and `task start` sets it automatically, so sessions are attributed without guessing.
- C15 · What each runtime really reports (cost, tokens, cache) in print mode and interactive sessions, and where the office collects it (the inbox adapters, W8?).
- ~~A21~~ · *Resolved (ada):* `office.db` gets `session_usage` (session, work item, runtime, model, tokens in/out, cost, reported or estimated, start, end). It's transitional, so it lives in the database. The adapters (codex-dev) capture it, and server-dev stores it.
- ~~A22~~ · *Resolved (ada):* yes, derived from work-item state events (waiting on the owner since/until), with no separate timestamps.

**For the user**
- ~~U16~~ · *Resolved (the user):* don't track plans. Always tokens at API prices.
- ~~U17~~ · *Resolved (the user):* no clients. Show each project's investment.
- ~~U18~~ · *Resolved (the user):* US dollars. Budgets per project (my recommendation, since there are no clients).
