---
kind: spec
title: "Costs, tokens, time and insights (UX)"
status: for-review
revision: 6
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-13"
requested_by: owner
reviewers: [ac-staff-engineer-ada, codex-dev, owner]
builds_on: [work-items-ux, our-org-in-the-app, decisions-and-tech-debt-ux]
---

# Spec · Costs, tokens, time and insights (UX)

Extends `work-items-ux.md` (tasks, bugs), `our-org-in-the-app.md` (sessions, runtimes, Activity) and `decisions-and-tech-debt-ux.md` (debt docs).

## The request (the user, 2026-11-08)

> For each project we invoke the agents. We need to see how much the sessions cost, the number of tokens, and insights: how long tasks take, the speed of tasks, the cost of a particular task and overall project costs. We need a structure for this.

> *(rev 2, the user)* No clients: the point is to see **what each project has cost (the investment)**. Immediate figures are fine. Don't worry about plans: value everything by its tokens at **API prices**, in **US dollars**.

**Revision 6 (2026-11-13): aligned with costs-and-budgets-v1 Rev 13 §12.1, D42.** The PRD (`costs-and-budgets-v1.md`) wins where this spec differs, and its §5.4 and §5.4a hold the full budget rules. The unit is the usage record, rolled up agent × task, not the session (decision 1). An agent on a subscription plan shows "API-equivalent cost" (decision 3). Attribution follows the PRD's order: work for a task, the current task at the time, the one-In-progress fallback, else **Not on a task**. The session-start task and "Not linked to a task" are gone (decision 4). Budgets are on tasks, agents and releases, and task and agent budgets stop work at 100%. Monthly project budgets that never stop agents are gone (decision 8, §4). The Budget reached item isn't dismissable, and after an Allow 20% more once the next stop offers only Raise budget and Stop (§4). Past costs are never re-estimated, and the owner edits the price list in Settings › Costs with an effective date, forward only (§5). Copilot runs are attributed like any run, with no separate Copilot line (decision 9). The Kind breakdown gains work type. Insights leads with Progress, and the Cost section follows (INS-5, `project-insights-v1` D3). **Criteria rewritten (same ID):** COST-1 to COST-6, INS-2, INS-5, BUD-1, BUD-2. **Added:** BUD-3. For ada: `docs/design/smart-features.md` l.132 still says this spec shows copilot runs as "Copilot"; they are now attributed like any run (COST-6).

**Revision 5 (2026-11-08): review fixes.** Working time isn't tracked yet, so wherever Insights shows it (the Working time tile, the investment line, the breakdowns' Working column, Insights › Projects and the agent profile line) it shows "—" with "not tracked yet" until session tracking exists, never a number (§1). Revision 4's note on waiting on you now states the narrow rule. **Criteria rewritten (same ID):** INS-1.

**Revision 4 (2026-11-08): aligned with ada's W7 model.** Working time isn't tracked yet (it needs session tracking). Until it is, the time split shows waiting on you, blocked and **the rest**, never idle, so untracked work is never counted as idle (decision 5, §2). Waiting on you counts questions to you, Fixed bugs you reported, scoped requests, triage suggested to you and a task Blocked waiting for your decision on a proposed ADR, only while it's the blocker. An agent sets its current task with `org-agent task current <id>` (was `status --task`, C14 resolved). Task numbers follow `work-items-ux.md` rev 6 (`T-13`). **Criteria rewritten (same ID):** TIME-1. **Added:** TIME-7.

**Revision 3 (2026-11-08): reviewer fix.** A39: organization totals need session usage from every office (§7).

**Revision 2 (2026-11-08):** clients removed (decision 6, §3), plans removed: every cost is tokens at API prices (decision 3), US dollars, budgets per project only, and an **Investment** figure (the project's total to date) leads the project's Insights. U16–U18 resolved. COST-5, INS-5 and BUD-1 rewritten.

**The questions it must answer in one or two clicks:** What did this task, bug fix or project cost? What has this project cost so far (its investment)? Where is the money going (which project, team, area, agent or model)? How long does work take, and where does it wait? Is anything unusually expensive or slow? Are we within budget?

## 0. Decisions

1. **The unit is the usage record** *(rev 6, D42)*: one report of tokens and cost from a runtime, normally one per turn. It records the agent, the time, the runtime, the model, the four token counts (input, output, cache read, cache write), the cost and its basis, the price used, and the task it counts toward and how. The base roll-up is **agent × task**: an agent's records on one task. Everything else (task, bug, release, project, team, agent) rolls up from it. A **session run** (an agent's work from wake to sleep) has many records and can span several tasks.
2. **Every number says where it came from.** Cost is **Reported** (the runtime said so), **Estimated** (tokens × the price stored with the record) or **No price** (the run has tokens but no price) *(rev 6)*. It shows as "$4.12", "≈ $4.12" or "—" with the reason on hover. A total with N runs with no price says "excludes N runs with no price", and its tokens include them. A runtime that reports no usage adds "Usage incomplete: {reason}" to every total it affects. A blank is never shown as $0.
3. **Cost is always tokens at API prices, in US dollars** *(rev 2, the user)*, whatever plan the runtime is on, so work compares fairly across runtimes and projects. When a runtime reports a cost at API prices, that figure is used (Reported). Otherwise the app multiplies tokens by the price list (≈). *(rev 6)* An agent on a subscription plan shows its cost as **API-equivalent cost**: what the same tokens cost at API list prices. It's labelled wherever it shows, a total that includes any says "includes API-equivalent", and it counts against dollar budgets like any other cost.
4. **Each usage record counts toward one task** *(rev 6)*. First match wins: the task the agent is doing **work for** (reviewing it, or invoked for it as a helper); otherwise the agent's current task at the time of the record (`org-agent task current T-17`, also set when the agent starts a task, C14); otherwise, when the agent has no current task and exactly one In progress task or bug, that item, marked "by fallback". Anything else is **Not on a task**: the project's bucket when the agent is a member of exactly one project, otherwise the organization's. It's shown, counted in the project and agent totals, and never guessed onto a task. The owner and the project manager can move a record to another task, with a reason (`costs-and-budgets-v1.md` requirement 33).
5. **Time has four honest parts** for every task and bug: **Lead time** (created → done) and **Cycle time** (started → done), split into **Working time** (sessions active on it), **Waiting on you** (a question to the owner, a Fixed bug the owner reported, a scoped request, triage suggested to the owner, a task Blocked waiting for the owner's decision on a proposed ADR, only while that's its blocker (ada Q11)), **Blocked** (Blocked status) and idle. Working time isn't tracked yet *(rev 4)*: until it is, the split shows **the rest** in place of working and idle, and untracked working time is never counted as idle. All of it is derived from the item's state changes (ada, A22). Speed is shown with medians, never averages (one stuck task distorts an average).
6. **A project's investment leads its costs** *(rev 2: no clients; rev 6: `project-insights-v1` D3)*: the total cost to date (and working time and items done) is the first figure of the Cost section on the project's Insights, which follows Progress and Speed, and the Investment column on the Projects list, with this month beside it. Figures can be immediate (computed when shown), with no need for nightly reports.
7. **Insights point at something to do,** each with a link: "T-13 cost 4× the median bug fix ($18 vs $4.50) · Open", "Agents waited 6 h for your answers this week · See questions", "Payments has 40% of this month's cost". They're not a wall of charts.
8. **Budgets stop work at 100%** *(rev 6, D42)*. Budgets are optional, in US dollars or total tokens (never both), on a **task** or bug, an **agent** (per day, per month) and a **release** (a cap). At 80%, whoever last set or changed the budget gets one neutral notice. At 100%, work on the task stops and an agent pauses, each with one **Budget reached** Inbox item for the owner per budget reached (an agent at both its daily and monthly limits gets two). A release cap only warns. Monthly project budgets are gone (§4).
9. **Copilot runs are costs too** (smart features) *(rev 6)*: they're attributed like any run (decision 4), with no separate Copilot line. A run with no price shows "—" and the totals it's in say "excludes N runs with no price". Its tokens always count.

## 1. Where costs show (small to large)

| Place | What it shows |
| --- | --- |
| **Usage record row** (task page › By agent, agent profile › Cost) *(rev 6)* | time · runtime · model · tokens in / out / cache read / cache write · cost (Reported / ≈ / —, labelled API-equivalent for a subscription plan) · the task and how it was attributed (work for, current, by fallback, or Not on a task) |
| **Task, bug and debt pages** (side column, **Cost and time**) | cost · tokens · a table by agent (agent × task) · the budget bar, when it has a budget *(rev 6)* · lead time split into waiting on you · blocked · the rest (working time once it's tracked), with the comparison "Typical for a task in Office: $3–6, 2–5 h" |
| **Project › Insights tab** (its position on the project page follows `information-architecture-v2.md` and `project-insights-v1` D40) | *(rev 6)* Progress and Speed lead (`project-insights-v1` D3), and the Cost section follows with: this month and last: cost, tokens, working time, tasks and bugs done · cost by team, area, agent, model and kind (task, bug, debt payback, retro, report, Not on a task; rev 6: copilot is no longer a kind; was: task, bug, debt, copilot, not linked), with work type (v1: Build only, shown as R&D) · speed (median lead time, working time, waiting on you, by kind) · R&D total so far and for the period, by release, milestone, agent and period · **Spent on cancelled work**, inside the total · Not on a task · the insights list · spent of cap for each release with a cap (`costs-and-budgets-v1` §6, AC-CB9-1) |
| **Insights** (nav, org-wide) | the same, across projects · a Projects table (investment to date, this month, last month, change, working time, items done) · Export |
| **Projects list** | an **Investment** column (total to date, US dollars) |
| **Agent profile › Overview** | this week's cost, working time ("— not tracked yet") and tasks done, and cost per task done, vs the team median |
| **Home** | one line: "This month: ≈ $412 · 63 tasks done · 30 h waiting on you" with **Insights**. *(rev 6)* No budget warning line: Budget reached is an Inbox item (§4) |

**Working time isn't tracked yet** (it needs session tracking). Until it is, every place that shows working time (the Working time tile, the investment line, the breakdowns' Working column, Insights › Projects, speed and the agent profile) shows "—" with "not tracked yet", never a number. Cost, tokens, items done and the time split (§2) are unchanged.

```
 Projects › Agentic Org › Insights                     [This month ▾]  [Export ▾]
 ┌ Cost ─────────────┐ ┌ Tokens ──────────┐ ┌ Working time ──┐ ┌ Done ──────────────┐
 │ ≈ $412  ▲ 18%     │ │ 61.2 M           │ │ —              │ │ 63 tasks · 21 bugs │
 │ R&D (Build)       │ │ 74% cache reads  │ │ not tracked yet│ │ 4 debt paid        │
 └───────────────────┘ └──────────────────┘ │30 h waiting on │ └────────────────────┘
                                            │ you            │
                                            └────────────────┘
 Insights
  ⚠ T-13 "Disable Pay while submitting" cost 4× the median task in App ($18 vs $4.50)  [Open]
  ⓘ Agents waited 30 h for your answers this month, most on ada's questions (19 h)        [See]
  ⓘ codex-dev's tasks finish 2× faster than the team median this month                  [Open]
 By team            Cost       Working   Tasks  Cost/task   Median lead time
  Office            ≈ $171     — †       22     ≈ $7.80     1.6 d
  App               ≈ $132     — †       25     ≈ $5.30     1.1 d
  Agent tooling       $68 *    — †       12       $5.70     0.9 d
  Design            ≈ $29      — †        4     ≈ $7.20     0.4 d
  Not on a task     ≈ $12      — †        —        —          —
  * Usage incomplete: Antigravity doesn't report usage
  † working time: not tracked yet
```

- *(rev 6)* This is the tab's Cost section, after Progress and Speed (`project-insights-v1` D3).
- **Breakdowns** switch with pills: Team · Area · Agent · Model · Kind. Kind also groups by work type *(rev 6)*: v1 has Build only, shown as R&D. Each row opens a filtered list of the usage records or items behind it, so every total can be traced to records.
- **Charts:** one trend line (cost per day) and bars for the breakdown, following the dataviz rules: labelled values, no color-only meaning, and an accessible table toggle.
- **Periods:** This week · This month · Last month · Custom · All time. Comparisons are always against the previous period of the same length.

## 2. Flow · Task cost and time (the page block)

```
 Cost and time
 ≈ $6.40 · 1.9 M tokens · 2 agents                    [By agent ▾]
 Budget $10  ▓▓▓▓▓▓░░░ 64%
 Lead time 1 d 4 h  =  waiting on you 3 h · blocked 0 · the rest 1 d 1 h (working time isn't tracked yet)
 Typical for App: ≈ $3–6 · 1–2 d
```

- **The rest** *(rev 4)* is the lead time that wasn't waiting on you or blocked. Working time isn't tracked yet, so the rest can't be split into working and idle, and untracked working time is never shown as idle. Once working time is tracked, the line becomes "working 2 h 10 m · waiting on you 3 h · blocked 0 · idle 22 h 50 m", where **idle** is the time nobody worked on it and it wasn't waiting on anyone. Showing it stops "1 day" from reading as "1 day of work".
- **By agent ▾** *(rev 6)* is the agent × task table: agent, tokens, cost, last activity. Each row opens that agent's usage records on the task. A record attributed by fallback is marked "by fallback", and the owner or the project manager can move a record from here. A parent shows own, subtasks and total, and its budget bar counts the total.
- **The budget bar** shows spent of the budget, with the 80% mark. The owner or the project manager sets or changes the budget here (§4).
- **States:** no records yet ("No agent time on this yet") · a run with no price ("—", and "excludes N runs with no price") · the runtime gives no usage ("Usage incomplete: Antigravity doesn't report usage.") · "Estimate $5, proposed by Kai · **Accept**" (Accept for the owner and the project manager) · "Budget reached" (amber, waiting on the owner).

## 3. Flow · Investment and export

- **Project › Insights** *(rev 6)*: its Cost section, after Progress and Speed, opens with **Investment**: "Agentic Org has cost ≈ $1,240 so far (since 7 Nov) · agent work: — (not tracked yet) · 180 tasks and 64 bugs done", then this month.
- **Insights › Projects** (org-wide): project · investment to date · this month · last month · change · working time · items done.
- **Export ▾:** **CSV** (one row per usage record *(rev 6)*, with project, task, how it was attributed, agent, runtime, model, tokens, cost, the cost source and time) · **Status report for the month** (`project-status-ux.md`, which replaces a separate monthly report; a Markdown doc saved to the project's `docs/reports/`, with totals, the breakdowns and the insights).

## 4. Flow · Budgets

*(rev 6, D42)* Budgets follow `costs-and-budgets-v1.md` §5.4. Its decision table (§5.4a) gives every status, action, actor and refusal, so this section shows the UI and doesn't copy it. Monthly project budgets are gone.

- **Where they're set:** a task's or bug's budget in its Cost and time section (the owner or the project manager); an agent's daily and monthly budgets on its profile › Cost (the owner); a release cap on the release header (the owner or the project manager). Each is in US dollars or total tokens, never both.
- **80%:** one neutral notice to whoever last set or changed the budget. Never amber, never an Inbox item.
- **100% of a task budget:** the agents on it finish their turn and stop. An In progress task moves to Blocked "Budget reached". A To do or In review task keeps its status and gets a budget hold, and so does a bug at any open status. The owner gets one amber **Budget reached** Inbox item.
- **100% of an agent budget:** the agent pauses after its current turn and shows "Paused · Budget reached" in amber, with a Budget reached Inbox item. A daily pause lifts at the next day, a monthly one at the next month, with a neutral notice.
- **A release cap only warns:** neutral notices at 80% and 100%. It never stops work and makes no Inbox item.
- **The Budget reached item isn't dismissable,** because it holds work stopped. Its actions: **Raise budget…** · **Allow 20% more once** · **Stop**, which is **Cancel** on a task, **Won't fix** on a bug and **Keep paused** for an agent. On a task budget, raising, clearing and Allow once are a person's actions (the owner or a human project manager). An agent project manager is refused: "Only a person can raise, clear or extend a budget." Stop is for the owner or the project manager, human or agent. On an agent budget, every action is the owner's. The item clears when the stop is resolved (`costs-and-budgets-v1.md` requirement 23).
- **The repeat stop:** Allow 20% more once raises the limit by 20%, one time. When spending reaches the new limit, the work stops again, and the new Budget reached item offers only **Raise budget…** and **Stop** (Keep paused for an agent), with no second Allow once (AC-CB6-4). A new budget value re-arms it, and so does, for an agent budget, the next day or month.
- **Estimated costs count toward budgets,** and the bar says "≈ includes estimates". API-equivalent cost counts like any other cost. Runs with no price add nothing to a dollar budget.

## 5. Data sources the UX depends on (for ada and codex-dev)

| Runtime | What it gives (to confirm, C15) | Shown as |
| --- | --- | --- |
| Claude Code, print mode (`claude -p`, copilot) | cost and usage per run | Reported |
| Claude Code, interactive sessions | token usage per message in the transcript; no cost | ≈ (tokens × the price list) |
| Codex (`codex exec`, sessions) | token usage | ≈ |
| Antigravity | unknown | — (cost and tokens) until it reports |

**Prices** *(rev 6)*: the owner edits the price list in **Settings › Costs**: a rate per model with an effective date (today or later), and the list of past and scheduled rates. A rate applies from its effective date forward. Each record stores the price it used, so a price change never changes a stored record, and past costs are never re-estimated.

## 6. UX acceptance criteria

**Costs (COST)**
- COST-1 · *(rev 6)* Every usage record row shows time, runtime, model, tokens (input, output, cache read, cache write), the task and how it was attributed, and cost with its source: Reported ($), Estimated (≈ $) or no price (— with the reason on hover).
- COST-2 · *(rev 6)* Task, bug and debt pages show cost, tokens and a table by agent (agent × task), and each row opens the records behind it. A parent shows own, subtasks and total.
- COST-3 · *(rev 6)* A record counts toward a task only by the order in decision 4: work for a task, the current task at the time, the fallback (marked "by fallback"), else Not on a task. Not on a task is counted in the project (or organization) and agent totals.
- COST-4 · *(rev 6)* Project and organization totals equal the sum of their usage records, and every breakdown row opens the records behind it.
- COST-5 · *(rev 6)* Every cost is in US dollars at API prices. An agent on a subscription plan shows "API-equivalent cost", and a total that includes any says "includes API-equivalent".
- COST-6 · *(rev 6)* Copilot runs are attributed like any run, with no separate Copilot line. A run with no price shows "—", every total it's in says "excludes N runs with no price", and its tokens count.

**Time and speed (TIME)**
- TIME-1 · *(rev 4)* Tasks and bugs show lead time split into working, waiting on you, blocked and idle, and the parts add up to the lead time. Until working time is tracked, the split shows waiting on you, blocked and "the rest", with "working time isn't tracked yet", and untracked working time is never counted as idle.
- TIME-2 · Speed figures (lead time, working time, cost per task) are medians, labelled "median".
- TIME-3 · Each task and bug shows the typical range for its team and kind beside its own numbers.
- TIME-4 · *(new)* Every task and bug shows cycle time (started → done) beside lead time, both derived from state changes, and open items say "so far".
- TIME-5 · *(new)* Task and bug lists can show sortable Cycle time and Lead time columns.
- TIME-6 · *(new)* Insights shows median lead time and cycle time per project, area, team and assignee for the period.
- TIME-7 · *(new, rev 4)* Waiting on you counts questions to the owner, Fixed bugs the owner reported (waiting for the owner's check), scoped requests waiting for approval, triage suggested to the owner, and tasks Blocked waiting for the owner's decision on a proposed ADR (only while that decision is the task's blocker, ada Q11).

**Insights (INS)**
- INS-1 · *(rev 5)* The project's Insights tab shows cost, tokens, working time and items done for the period, with the change from the previous period. Until session tracking exists, working time shows "—" with "not tracked yet" (tile, investment line, speed figures, breakdowns, Insights › Projects and the agent profile), never a number.
- INS-2 · *(rev 6)* Breakdowns by team, area, agent, model and kind (with work type) are available, each with a table view for screen readers.
- INS-3 · Insights are sentences with a link to the items behind them. At least these exist: an outlier task or bug (≥ 3× its team's median), waiting on you this period, and a cost concentration (one area or team ≥ 40%).
- INS-4 · A runtime with no usage data is marked in every total it affects, never counted as zero.
- INS-5 · *(rev 6, `project-insights-v1` D3)* The project's Insights leads with Progress. Its Cost section follows Progress and Speed and opens with the investment to date. The org-wide Insights lists projects by investment, and Export gives a per-record CSV and a monthly report doc in `docs/reports/`.

**Budgets (BUD)**
- BUD-1 · *(rev 6)* A task or bug, an agent (per day, per month) and a release can have a budget in US dollars or total tokens, never both. At 80%, whoever last set or changed it gets one neutral notice, never an Inbox item. At 100%, work on the task stops and an agent pauses, each with one amber Budget reached Inbox item for the owner per budget reached (an agent at both limits gets two). A release cap only warns. The full rules are `costs-and-budgets-v1.md` §5.4 and §5.4a.
- BUD-2 · *(rev 6)* The Budget reached item isn't dismissable. It offers Raise budget, Allow 20% more once and Stop (Cancel on a task, Won't fix on a bug, Keep paused for an agent). On a task budget, raising, clearing and Allow once are a person's actions, and an agent project manager is refused with "Only a person can raise, clear or extend a budget."
- BUD-3 · *(new, rev 6)* After an Allow 20% more once, when spending reaches the new limit, the work stops again, and the new Budget reached item offers only Raise budget and Stop (Keep paused for an agent), with no second Allow once (AC-CB6-4).

## 7. Open questions

**For codex-dev and ada**
- ~~A39~~ · *Resolved (ada):* phase 1 is HQ only, so HQ's `office.db` has everything. In phase 2, `session_usage` syncs to HQ as work data on the per-origin streams (like tasks), so HQ shows organization totals without querying offices. This changes `docs/design/offices-hq-and-satellites.md`, which says sessions aren't copied, and ada updates it.
- ~~C14~~ · *Resolved (ada W7):* the agent sets its current task with `org-agent task current <id>` (`--clear` clears it), and `task start` sets it automatically, so sessions are attributed without guessing.
- C15 · What each runtime really reports (cost, tokens, cache) in print mode and interactive sessions, and where the office collects it (the inbox adapters, W8?).
- ~~A21~~ · *Resolved (ada):* `office.db` gets `session_usage` (session, work item, runtime, model, tokens in/out, cost, reported or estimated, start, end). It's transitional, so it lives in the database. The adapters (codex-dev) capture it, and server-dev stores it. *(Rev 6: the unit is now the usage record, D42.)*
- ~~A22~~ · *Resolved (ada):* yes, derived from work-item state events (waiting on the owner since/until), with no separate timestamps.

**For the user**
- ~~U16~~ · *Resolved (the user):* don't track plans. Always tokens at API prices.
- ~~U17~~ · *Resolved (the user):* no clients. Show each project's investment.
- ~~U18~~ · *Resolved (the user):* US dollars. Budgets per project (my recommendation, since there are no clients). *(Rev 6: replaced by task, agent and release budgets, D42.)*
