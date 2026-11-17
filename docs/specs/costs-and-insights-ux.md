---
kind: spec
title: "Costs, tokens, time and insights (UX)"
status: current
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
requested_by: owner
builds_on: [work-items-ux, our-org-in-the-app, decisions-and-tech-debt-ux]
---

# Spec · Costs, tokens, time and insights (UX)

## Scope and contract

[Accounting](../design/organization-data/10-usage-and-cost-accounting.md) owns immutable normalized usage, attribution and corrections, prices and coverage; [budgets](../design/organization-data/17-budgets-and-controls.md) owns policies, periods, extensions, notices and holds; [execution](../design/organization-data/11-model-profiles-and-agent-execution.md) owns turns, attempts, permits and process fencing. This spec owns what people see: owner-wide amounts, project-manager scoped amounts and the own-agent CLI exception. Detailed analytics are [not in v1](#8-not-in-v1).

## The request (the user, 2026-11-08)

> For each project we invoke the agents. We need to see how much the sessions cost, the number of tokens, and insights: how long tasks take, the speed of tasks, the cost of a particular task and overall project costs. We need a structure for this.

> *(the user, later)* No clients: the point is to see **what each project has cost (the investment)**. Immediate figures are fine. Don't worry about plans: value everything by its tokens at **API prices**, in **US dollars**.

**The questions it must answer in one or two clicks:** What did this task, bug fix or project cost? What has this project cost so far (its investment)? Where is the money going (which project, team, agent or model)? How long does work take, and where does it wait? Is anything unusually expensive or slow? Are we within budget?

## 0. Decisions

1. **The unit is an immutable normalized usage event**, not a session total or assumed one-event-per-turn. It captures the actual consuming employee or one-off invocation, occurrence/origin, runtime/provider/model, disjoint token categories, valuation basis/status, exact price version and historical work attribution. A logical turn may have several connection attempts/processes; a run is an opaque correlation ID and can span tasks. Normalize each source exactly once; never sum cumulative totals with their deltas or aggregate/helper reports with their constituents. Totals and agent × task breakdowns derive from the same effective unsuperseded events, including one-offs.
2. **Every number says what is known.** API list-price amounts are estimates: **Runtime estimate** (`runtime_estimate`, ≈) or **Token estimate** (`token_estimate`, ≈), never an invoice or a falsely exact “Reported” cost. **Unpriced** and **unknown** show “—” with an accessible reason. Token/cost completeness is independently complete, partial or unavailable; null means unknown, zero means known zero. A known subtotal says “Usage incomplete” with missing/unpriced record counts and archive freshness/stream gaps; every total it affects repeats that qualification. An all-unknown sum stays “—”. Reasoning is a subset of output, not another token charge; sum input/output/cache-read/cache-write once after provider cache-overlap normalization.
3. **Cost is API-price-equivalent value in US dollars**, independent of subscription fees. Subscription usage says **API-equivalent cost** wherever shown, and mixed totals say “includes API-equivalent”. Store integer micro-USD; token estimates use exact arithmetic and round half up once per event. Use the price version effective at consumption time. Preserve recorded valuation; changing rates never reprices past events.
4. **Attribution is historical captured context.** Explicit work-for/current-task context identifies a task when known. Otherwise retain explicit project/team overhead, or organization overhead when unknown; never infer a task/project from one assigned In progress item, current home team or the number of project memberships. Capture accountable `team_id`, optional project/feature/release/milestone, task type and Build/Run classification at consumption time. Assigned team/assignee and consuming employee remain independent. Today's membership/release changes cannot move past spend. The owner or authorized project manager may propose an attribution correction with a reason: append a full replacement event, preserving consumption/consumer/valuation and original event. Register against the current correction leaf through HQ; its new leaf contributes once. Offline proposals cannot create competing accepted corrections. Every task type links by its `T-` reference, for example `T-A42`, which is the same in every scope.
5. **Time has four honest parts** for every task and bug: **Lead time** (created → done) and **Cycle time** (started → done), split into **Working time** (verified execution intervals attributed to it), **Waiting on you** (a question to the owner, a bug marked as fixed that waits for the owner's verification, a scoped request waiting for the owner's approval, triage suggested to the owner, a task Blocked on the owner, only while that's its blocker), **Blocked** (Blocked status) and idle. Working time isn't tracked yet: until it is, the split shows **the rest** in place of working and idle, and untracked working time is never counted as idle. Lead/cycle/state intervals derive from durable task history and saved waiting spans (document 05), not disposable session logs. Actual working time requires execution evidence; task state alone cannot prove it. Define non-overlapping intervals before showing a sum. Speed is shown with medians, never averages (one stuck task distorts an average).
6. **A project's investment leads its costs:** the total cost to date (and working time and items done) is the first figure of the Cost section on the project's Insights, and the Investment column on the Projects list, with this month beside it. Figures can be immediate (computed when shown), with no need for nightly reports.
7. **Insights point at something to do,** each with a link: "T-A13 cost 4× the median bug fix ($18 vs $4.50) · Open", "Agents waited 6 h for your answers this week · See questions", "Payments has 40% of this month's cost". They're not a wall of charts.
8. **Known spend at a limit requests a budget hold** (document 17). Budgets are optional, in US dollars or total tokens (never both), on a **task** or bug, an **agent** (per day, per month) and a **release** (a cap). At 80%, whoever last set or changed the budget gets one neutral notice. At 100% known spend, stop admitting affected new turns, let current turns finish and track **Stopping · Budget reached** (`stop_pending`) before **Paused/held** is verified. Each reached task/agent budget has one owner item; daily/monthly holds are independent. Reporting lag and in-flight turns can overshoot; a hard limit is not exact provider-spend protection. A release cap only warns. Monthly project budgets are gone (§4).
9. **Copilot runs are costs too** (smart features): they're attributed like any run (decision 4), with no separate Copilot line. An unpriced record shows "—" and every affected total says "excludes N unpriced records". Its known tokens count, with independent token completeness labels.

## 1. Where costs show (small to large)

| Place | What it shows |
| --- | --- |
| **Usage record row** (task page › By agent, agent profile › Cost) | time · runtime · model · tokens in / out / cache read / cache write · cost (Runtime estimate / Token estimate / —, labelled API-equivalent for a subscription plan) · the task and how it was attributed (work for, current, explicit project/team overhead, or organization overhead) |
| **Typed task pages** (side column, **Cost and time**) | cost · tokens · a table by agent (agent × task) · the budget bar, when it has a budget · lead time split into waiting on you · blocked · the rest (working time once it's tracked), with the comparison "Typical for a task in Office: $3–6, 2–5 h" |
| **Project › Insights tab** | The Cost section: this month and last: cost, tokens, working time, tasks and bugs done · cost by captured team, consumer, model, extensible task type and work type (Build/Run where recorded; unknown remains unknown) · speed (median lead time, working time, waiting on you, by kind) · R&D total so far and for the period, by release, milestone, agent and period · **Spent on cancelled work**, inside the total · Not on a task · the insights list · spent of cap for each release with a cap (document 17) |
| **Insights** (nav, org-wide) | the same, across projects · a Projects table (investment to date, this month, last month, change, working time, items done) · Export |
| **Projects list** | an **Investment** column (total to date, US dollars) |
| **Release header** (release page and the Releases tab row) | "Spent $412 of $600 cap" with a bar, or "Spent $412" with no cap. Past the cap: the real figure, neutral styling. A released release (one with a succeeded production deployment record; releases store no status) adds "Spent at release $398" beside the current figure (a release-time snapshot needs retained checkpoint evidence; never recompute it from today's membership) |
| **Agent profile › Overview** | this week's cost, working time ("— not tracked yet") and tasks done, and cost per task done, vs the team median (for a project manager, cost, tasks done and cost per task count only records and tasks in their projects, labelled "in your projects", with no team-median comparison) |
| **Agent profile › Cost** | spend by period (today, this week, this month, last month) in the viewer's zone, by project (with Not on a task) and top tasks by cost (a project manager sees only their own projects' rows and tasks, including a project's own Not on a task bucket, with period totals that add up only those rows, labelled "in your projects"; the organization's Not on a task bucket is hidden) · for the owner only, daily and monthly budget bars in the budget's zone (the earliest-created owner account's, HQ's before TZ-1), labelled with it, for example "Today (America/New_York)", so the two "today" figures can differ · "Stopping · Budget reached" while stop_pending, then "Paused · Budget reached" after verified stopping · for the owner, a neutral "Budget reset" note after its hold releases; say "working again" only when execution is actually admitted |
| **Organization › Cost** (owner only) | totals for the period by project and by consumer/work type, retaining overhead/unknown classification, with each agent's daily and monthly budget bars (owner only) and the org's Not on a task line, with the project Cost section's period filter, time-zone rules and Table view. Each figure opens its records. Run/KTLO remains visible only when captured; no classification engine is implied |
| **Home** | one line: "This month: ≈ $412 · 63 tasks done · 30 h waiting on you" with **Insights**. No budget warning line: Budget reached is an Inbox item (§4) |

**States for Agent profile › Cost and Organization › Cost**: loading shows the layout with placeholders; no records yet reads "No spend yet" (for a project manager, "No spend in your projects yet" when the agent has none there); on Agent profile › Cost, no budget reads "No budget set" with **Set budget…** for the owner (a project manager sees no budget area); the office unreachable reads "Couldn't load costs. Try again." with **Try again**.

**Who sees amounts**: the owner sees all cost, and a project manager sees cost for their projects. A project manager is the lead of the project's accountable team: a position in the hierarchy, not an access role ([agent structure](../design/organization-data/04-agent-structure.md#accounts-people-and-the-organization-owner)). Other members see no amounts anywhere in the app (this table, run and Activity rows, the Runs tab, reports) or in the CLI and API, except the authenticated own-agent exception below, but see a task's **budget state** (none, OK, 80%, reached) on tasks they can see, so they know why a task is blocked (including stop_pending versus held). Only the owner and the project manager see a release's amounts and set its cap. An agent's daily and monthly budget bars are the owner's only, because they count spend from every project the agent works on; a project manager sees an agent's cost only inside their own projects. An agent sees its own current task's budget and its own daily and monthly budget, spent and remaining, through the CLI only, including incomplete-data/freshness labels. This grants no raw archive/history access, other-agent totals or owner UI controls. Apply the same scope to file access, search, CSV, notices, APIs and underlying records, not only bars.

**Working time isn't tracked yet** (it needs session tracking). Until it is, every place that shows working time (the Working time tile, the investment line, the breakdowns' Working column, Insights › Projects, speed and the agent profile) shows "—" with "not tracked yet", never a number. Cost, tokens, items done and the time split (§2) are unchanged.

```
 Projects › Agentic Org › Insights                     [This month ▾]  [Export ▾]
 ┌ Cost ─────────────┐ ┌ Tokens ──────────┐ ┌ Working time ──┐ ┌ Done ──────────────┐
 │ ≈ $412  ▲ 18%     │ │ 61.2 M           │ │ —              │ │ 63 tasks · 21 bugs │
 │ R&D (Build)       │ │ 74% cache reads  │ │ not tracked yet│ │ 4 requests scoped  │
 └───────────────────┘ └──────────────────┘ │30 h waiting on │ └────────────────────┘
                                            │ you            │
                                            └────────────────┘
 Insights
  ⚠ T-A13 "Disable Pay while submitting" cost 4× the median task in App ($18 vs $4.50)  [Open]
  ⓘ Agents waited 30 h for your answers this month, most on ada's questions (19 h)        [See]
  ⓘ codex-dev's tasks finish 2× faster than the team median this month                  [Open]
 By team            Cost       Working   Tasks  Cost/task   Median lead time
  Office            ≈ $171     — †       22     ≈ $7.80     1.6 d
  App               ≈ $132     — †       25     ≈ $5.30     1.1 d
  Agent tooling     ≈ $68 *    — †       12     ≈ $5.70     0.9 d
  Design            ≈ $29      — †        4     ≈ $7.20     0.4 d
  Not on a task     ≈ $12      — †        —        —          —
  * Usage incomplete: Antigravity doesn't report usage
  † working time: not tracked yet
```

- This is the tab's Cost section.
- **Breakdowns** switch with pills: Team · Consumer · Model · Task type · Work type. Build and Run use captured classification; missing classifications stay unknown. Each row opens a filtered list of the effective usage records or items behind it, so every total can be traced to records.
- **Charts:** one trend line (cost per day) and bars for the breakdown, following the dataviz rules: labelled values, no color-only meaning, and an accessible table toggle.
- **Periods:** This week · This month · Last month · Custom · All time. Comparisons are always against the previous period of the same length.

## 2. Flow · Task cost and time (the page block)

```
 Cost and time
 ≈ $6.40 · 1.9 M tokens · 2 agents                    [By agent ▾]
 Budget $10  ▓▓▓▓▓▓░░░ 64% · ≈ includes estimates
 Lead time 1 d 4 h  =  waiting on you 3 h · blocked 0 · the rest 1 d 1 h (working time isn't tracked yet)
 Typical for App: ≈ $3–6 · 1–2 d
```

- **The rest** is the lead time that wasn't waiting on you or blocked. Working time isn't tracked yet, so the rest can't be split into working and idle, and untracked working time is never shown as idle. Once working time is tracked, the line becomes "working 2 h 10 m · waiting on you 3 h · blocked 0 · idle 22 h 50 m", where **idle** is the time nobody worked on it and it wasn't waiting on anyone. Showing it stops "1 day" from reading as "1 day of work".
- **By agent ▾** is the agent × task table: agent, tokens, cost, last activity. Each row opens that agent's usage records on the task. The owner or authorized project manager can correct attribution from here. **Correct attribution (states):** **Move…** opens an authorized task or explicit overhead picker and requires a reason. **Saving:** “Recording correction…”; preserve the original event and append a replacement against the current leaf, with actor/time and immutable consumption/valuation. A conflict refreshes the leaf and requires review of the proposed correction, never overwrites it. If the outcome is unknown after disconnect, say “Correction not confirmed” with **Check status**, retaining operation ID; never claim nothing changed or publish an independent retry. Once confirmed, both scopes' derived totals update and the correction chain remains traceable. A project manager must have authority over both source and destination projects; moving to Not on a task preserves the explicit source project as overhead. Any correction reducing a reached task budget’s spend requires the same person authority as raising/clearing/extending, even if it remains reached; an agent manager sees “Only a person can raise, clear or extend a budget.” Reevaluate against a verified ledger checkpoint, releasing only satisfied holds. A parent may show separately labelled own/subtask/combined analytics where available; its own budget evaluates its recorded task attribution, not an inferred descendant sum.
- **The budget bar** shows spent of the budget, with the 80% mark. The owner or the project manager sets or changes the budget here (§4). When the budget is reached, the bar shows the Budget reached item's actions plus **Clear budget**, with the same rights and states (§4): **Raise budget…** · **Allow 20% more once** (not after a repeat stop) · **Clear budget** · **Stop**. This is where a project manager acts, since the Inbox item goes to the owner (document 17). People who can see amounts but can't act see the bar with no actions; other members see only the budget state (none, OK, 80%, reached), with no amounts (§1 "Who sees amounts").
- **States:** no records yet ("No usage records yet"; not a claim of zero consumption) · an unpriced record ("—", and "excludes N unpriced records") · the runtime gives no usage ("Usage incomplete: Antigravity doesn't report usage.") · "Budget reached" (amber, waiting on the owner).

### Shared duration definitions

With complete lifecycle evidence, lead time runs from creation and cycle time from first Started to last completion, or now while open, excluding Done/Cancelled spans. Reopen preserves first Started; direct To do → Done has cycle time zero. Typed-bug endpoints follow the task details contract. Missing baselines leave earlier time unavailable.

Compute integer whole seconds before aggregation. Aggregate duration displays floor to whole minutes below 1h, whole hours below 48h, then days with one decimal (`floor(seconds*10/86400)/10`). Make precise values accessible. **Time in progress** is elapsed status time attributed to the assignee during each interval, never hours worked; it cannot populate **Working time**, which remains unavailable without verified execution evidence.

## 3. Flow · Investment and export

- **Project › Insights**: its Cost section opens with **Investment**: "Agentic Org has cost ≈ $1,240 so far (since 7 Nov) · agent work: — (not tracked yet) · 180 tasks and 64 bugs done", then this month.
- **Insights › Projects** (org-wide): project · investment to date · this month · last month · change · working time · items done.
- **Export ▾:** **CSV** (one row per effective usage record, with correction lineage available, with stable event/source identity, historical project/team/task/release context, attribution/correction basis, actual consumer, runtime/provider/model, disjoint tokens and token status, cost/status/basis, price version, occurrence/recording time and coverage checkpoint) · **Status report for the month** (`project-status-ux.md`, which replaces a separate monthly report; a supporting project report in organization Git's `projects/<project-id>/reports/<document-id>.md`, with required project_id and null feature/release/deployment scope for this project-wide report, with the breakdowns and the insights in tokens and "Budget reached", never cost amounts; the status report is not in v1, `project-status-ux.md`).

### Cost reads and periods

UI, API and agent-CLI cost reads use the same effective-ledger query and scoped authorization for task, milestone, release, project, agent and owner-only organization figures. The command spelling is an interface decision. The UI keeps **This month** as its initial cost period; the CLI defaults to **All time**. Identical explicit scopes, instant bounds and checkpoints must return identical figures across interfaces.

CLI periods include rolling 7/30/90 × 24-hour windows ending at one captured now, offered calendar periods, a scoped release span, and inclusive whole-date custom ranges. Convert date ranges to half-open instant bounds in the acting account's known IANA zone. From without To ends at now; To without From and Period combined with either date are usage errors. A release span runs from creation to now or its retained release/cancellation instant, and filters captured release attribution; current membership never moves old usage. An organization query refuses a release period and directs the caller to the scoped release query.

Agent calendar day/month and own-budget reads use the selected budget zone and label it. Other CLI/server readers use the acting account zone; agent readers use the earliest-created owner account zone or the explicit HQ fallback. Account-zone persistence and authorization need their owning contract; there is no personal agent zone. A display-only UTC fallback cannot create budget periods.

By-period output uses day buckets for 7d, Monday-week buckets for 30d/90d, and days through 30 days or weeks thereafter for a release span. Other ranges use days through 31 days, weeks through 26 weeks, months thereafter. Label partial edge buckets. Apply scope authorization before aggregation; bucket sums retain the same completeness/coverage qualification as their subtotal.

## 4. Flow · Budgets

Budget controls follow [document 17](../design/organization-data/17-budgets-and-controls.md). There are no project, team, organization or feature budgets.

- **Where set:** any typed task's Cost and time (owner/authorized project manager), agent daily/monthly profile Cost (owner only), and release lifetime cap (owner/authorized project manager). One positive integer token limit or integer micro-USD limit; UI dollar entry converts exactly to micro-USD. Clearing is an explicit new retained policy revision, not deletion. Preserve the budget identity and all history.
- **Periods:** task/release use lifetime. Agent budgets use calendar day/month in the earliest-created owner's selected IANA zone, or HQ's zone before account-zone support. Require a known selected zone, never silently use UTC. Show zone/provenance and inclusive start/exclusive end bounds; day/month is not rolling 24h/30d. A time zone change closes the current period at an explicit boundary and keeps old periods. Viewer-zone spend figures may differ from budget-zone figures.
- **Set/raise dialog:** unit and value, current value prefilled, with required actor authority. Saving reads “Saving…” and retains inputs. Document 17 requires positive limits and reevaluation. A saved limit at/below known spend is reevaluated and may request a hold; it is never labelled instantly stopped. Refusals/conflicts show a field-linked accessible explanation and focus the relevant field. Unknown outcome says “Save not confirmed” with **Check status** and retains operation ID; a verified failure permits retry with the same identity. Changing only a comment or saving the same value never re-arms controls.
- **Actor/unit checks:** an authorized agent project manager may first-set or lower a task limit in the same unit; raising, clearing, extending or changing units requires a human owner/project manager. Agent-budget actions remain owner-only. A unit change is one retained policy-change event, reevaluating the same whole-period/lifetime consumption in its new unit without clearing prior spend.
- **Missing data:** warn “This runtime does not report usage, so this budget cannot stop its unmeasured work.” Unpriced dollar events say they contribute no known dollars and offer a token budget only if token coverage permits. Incomplete usage alone does not block admission. A known lower bound reaching the limit still triggers control; an incomplete amount below it is no guarantee of remaining capacity. Estimated/API-equivalent amounts count. Token totals use the four disjoint categories once.
- **80% base:** one neutral notice to the last policy changer, never an Inbox item. **100% task/agent:** one owner **Budget reached** item for each budget/policy/period/stage and an independent hold. Stop admitting affected new turns, finish current turns, then verify stopped before changing `stop_pending` to `held`. In-progress tasks become Blocked “Budget reached”; other open statuses, including typed bugs, keep status plus hold. Daily/monthly and task holds can coexist. **Release caps** only emit neutral 80/100% warnings, never hold work or create amber Inbox items.
- **Budget reached item:** not dismissible while unresolved. Show **Stopping** versus **Paused/held** accurately and retain stale/unknown outcome indications. Task actions **Raise budget…**, **Allow 20% more once**, **Clear budget** require a human owner/project manager; agent managers see “Only a person can raise, clear or extend a budget.” **Stop** uses existing owner/project-manager authority (person or agent) and requests Cancel/Won't fix through the task transition service. Agent-budget actions are owner-only. Task/release amounts stay owner/project-manager scoped; other authorized readers see state only.
- **One-time extension:** offer only at base reach; below it refuse “Allow 20% once is available only when the budget is reached.” without consuming the choice. For base integer L, effective limit is `L + floor(L/5)`. Refuse if the increment is zero or the new limit is still <= known spend, without consuming the extension. Persist the choice, prior/new limit, usage checkpoint, actor and operation ID before acknowledgement. Repeated clicks/replay cannot extend twice. Reaching the extended limit creates a new reached-stage item with **Raise budget…** and **Stop**, no second Allow once or second 80% warning. Say “This budget already used its one 20% extension.” Only a genuinely changed limit/unit or new agent calendar period re-arms it; cache deletion, reevaluation or comment edits do not reset spend/extension history.
- **Action states:** while Working…, disable other actions. Stop confirms the named task transition (Cancel T-A12 / Close as Won't fix) or “Keep this agent paused until reset?”, with optional reason. Capture actor, reason and actual transition outcome. Keep paused retains the item/hold until period reset or later authorized resolution. Verified refusal is shown inline with focus; unknown outcome shows **Check status** with the same operation ID, not “nothing changed”. Confirmed raise/clear/extend reevaluates and releases only the corresponding satisfied hold. Manual/security/other-budget holds remain.
- **Reset/resume:** close the old calendar period, retain its extension/kept-pause/history, release its budget hold and emit a neutral reset notice through the ordinary pipeline. New periods use the current policy without zeroing old consumption. Show “Budget reset” rather than promising a run resumed; all applicable holds, quota permits and placement readiness must permit it.
- **Distributed enforcement:** offices consult one authorized control state before affected turns. Disconnection cannot authorize clearing a hold; reconcile unknown outcomes before side effects repeat. Current-turn completion, lagging reports and concurrent in-flight turns can overshoot. No per-office independent allowance/counter or exact-spend guarantee is implied.

## 5. Data sources the UX depends on

Stable ledger/price/budget IDs use UUIDv7. Retain archived identities/tombstones for referenced tasks, consumers and historical scopes; closure/deletion never erases their usage or policy history.

The runtime contract can carry token/cost deltas and cumulative scopes independently. Each adapter must prove its source scopes, process epoch/baseline, provider cache semantics and actual model/connection context before capture. Claude print-mode list prices remain **Runtime estimate**, interactive token pricing is **Token estimate**, and any runtime (including Codex/Antigravity) with missing/ambiguous data is partial/unavailable with an issue reason; a product name alone proves no complete coverage.

**Prices:** owner-managed versioned provider/model rates with effective dates in Settings › Costs. Use an exact immutable price ID in each event. Referenced versions cannot change or be deleted; future changes use new IDs and dates and never re-estimate past spend. A new rate takes effect from today or a later date. Empty state says “No prices set: unpriced records add no known dollars.” Add a rate retains values while Saving…, accessible refusal/date focus and verified failure retry; reconcile an unknown save outcome before repeating it.

```text
<organization repository>/pricing/models/<price-id>.yaml
<data-root>/<organization-id>/durable/accounting/
├── usage/<origin-office-id>/<producer-id>/<UTC-date>.jsonl
└── checkpoints/<origin-office-id>/<producer-id>.json
<data-root>/<organization-id>/durable/budgets/<budget-id>/
├── budget.md                         # protected current revision, not shared Git
├── versions/<revision>.yaml          # retained policy history
├── periods/<period-id>.json
├── events/<event-id>.json
├── notices/<notice-id>.json
└── holds/<period-id>/<revision>-<stage>.json
<data-root>/<organization-id>/runtime/<office-id>/ # indexed views only
```

Accepted usage IDs/source keys, producer sequence/checkpoint hashes and correction chains survive retries and cache rebuild; an identical retry compares payload hash and a mismatch conflicts. Coverage is as of a known checkpoint, with gaps/disconnected sources visible; it is not real-time proof of zero spend. Durably append usage/checkpoints and source policy/decision/receipt evidence before acknowledgement through a recoverable commit boundary. Organization Git alone restores neither usage nor budget limits/just-used extensions; protected backup/journal evidence is required. Monetary files, events, exports and notices obey the same access rules as the UI. No prompts, private notes, credentials or chat text enter usage/budget metadata. A shared Git report contains safe identity/state and authorized token summaries only, never private amounts/limit history.

## 6. UX acceptance criteria

**Costs (COST)**
- COST-1 · Every immutable usage row shows actual consumer, time, runtime/provider/model, four disjoint token categories and independent token/cost completeness, estimate/unpriced/unknown basis, exact price version and historical attribution. Reasoning is an output subset; runtime list prices remain estimates.
- COST-2 · Typed task pages show spend/tokens across actual consumers, with drilldown into effective events; no other record duplicates a task's spend. Optional parent/subtask analytics are labelled separately; a task budget uses its captured task attribution.
- COST-3 · Explicit work-for/current-task context wins; otherwise retain explicit project/team overhead or organization unknown scope. New records never infer attribution from one assigned item/home team/project membership count. Corrections append actor-bound replacement events and preserve the original consumption/valuation.
- COST-4 · Every project/organization/breakdown total derives from the same effective correction leaves, counts consumption once and drills into authorized records. Missing-stream coverage, partial values and freshness remain visible in every affected subtotal.
- COST-5 · Every cost is in US dollars at API prices. An agent on a subscription plan shows "API-equivalent cost", and a total that includes any says "includes API-equivalent".
- COST-6 · Copilot/one-off consumption follows the same ledger and actual consumer attribution. Unpriced/unknown values show — with accessible reasons/counts, known tokens still count, and unknown totals never become zero.
- COST-7 · A project manager viewing an agent sees only amounts from usage records attributed to their own projects (decision 4), and these equal the agent's figures in those projects' own breakdowns (COST-4). Agent profile › Overview's cost, tasks done and cost per task (both parts of the ratio limited to their projects, with no team-median comparison) and Agent profile › Cost's period totals, rows and top tasks count only those records, including a project's own Not on a task bucket, and read "in your projects". The organization's Not on a task bucket and the daily and monthly budget bars don't show; the owner sees them all. On runs (How it runs, its totals, the Runs tab, Activity), a run's amount for them is the part attributed to their projects, labelled "in your projects" when the run also has other records, and a run with none shows no amount. Run counts and durations in How it runs' totals cover only runs with records in their projects. With no spend in their projects, the tab reads "No spend in your projects yet".

- COST-8 · Unknown save/correction outcomes reconcile retained actor/fingerprint receipts before retry; conflicts never overwrite a correction leaf or mutate accepted events. A PM must control both correction scopes. An agent PM cannot reduce reached spend from 25 to 24 against limit 20; a permitted person may, leaving the hold active. A move to project overhead retains its project.
- COST-9 · Protected usage/checkpoints and budget policy/extension/notice/hold evidence survive office-cache loss; Git alone is insufficient. File/API/export/search/notice access follows the same scoped amount rules and own-agent exception.
- COST-10 · Future cost readers agree for identical explicit scope/bounds/checkpoint, despite UI This month and CLI All time defaults. Mid-day rolling windows show partial edge buckets summing to the subtotal; DST calendar budget days remain distinct from rolling 24h. Invalid date/period combinations are refused.

**Time and speed (TIME)**
- TIME-1 · Tasks and bugs show lead time split into working, waiting on you, blocked and idle, and the parts add up to the lead time. Until working time is tracked, the split shows waiting on you, blocked and "the rest", with "working time isn't tracked yet", and untracked working time is never counted as idle.
- TIME-2 · Speed figures (lead time, working time, cost per task) are medians, labelled "median".
- TIME-3 · Each task and bug shows the typical range for its team and kind beside its own numbers.
- TIME-4 · Every typed task shows supported cycle/lead durations and open intervals say “so far”. Started day 0, Done day 2, reopened day 9 and Done day 10 yields cycle time 3 days, excluding the terminal gap. Unknown baselines remain unavailable. Aggregate formatter checks: 59m59s → 59 min; 47h59m → 47 h; 48h → 2.0 d; 126h → 5.2 d.
- TIME-5 · Task and bug lists can show sortable Cycle time and Lead time columns.
- TIME-6 · Insights may show median lead and cycle time by project, captured accountable team and assignee scope where evidence exists; current membership never rewrites historical usage or time.
- TIME-7 · Waiting on you counts questions to the owner, bugs marked as fixed that wait for the owner's verification, scoped requests waiting for the owner's approval, triage suggested to the owner, and tasks Blocked on the owner (only while the owner is the blocker).

**Insights (INS)**
- INS-1 · The project's Insights tab shows cost, tokens, working time and items done for the period, with the change from the previous period. Until session tracking exists, working time shows "—" with "not tracked yet" (tile, investment line, speed figures, breakdowns, Insights › Projects and the agent profile), never a number.
- INS-2 · Breakdowns use captured team, consumer, model, extensible task type and Build/Run classification, with unknown retained and an accessible table view.
- INS-3 · Insight sentences link to authorized underlying items, with explicit sample and coverage limitations: an outlier ≥3× the comparable team median, waiting on the owner, or team or project cost concentration ≥40%.
- INS-4 · A runtime with no usage data is marked in every total it affects, never counted as zero.
- INS-5 · The project's Cost section leads with investment. Org-wide project investment and per-effective-event CSV obey cost scope. A monthly report uses a stable-ID supporting project document, required project scope and at most one same-project feature/release/deployment scope; shared Git contains no monetary amounts/private limits.

**Budgets (BUD)**
- BUD-1 · Typed-task lifetime, agent calendar daily/monthly and release lifetime limits use tokens or micro-USD. Known 80% base produces one neutral notice; 100% task/agent creates independent owner items and stop_pending holds before verified held state. Release warns only. Unknown usage alone never blocks admission; known lower bounds can prove reach.
- BUD-2 · A permitted agent PM may first-set/lower in the same unit, but cannot change units, raise, clear or extend. Unresolved reached items are not dismissible; show stopping versus held. Task raise/clear/extend requires human owner/project-manager authority, Stop uses existing transition authority, agent-budget actions require owner. A satisfied change releases only its own hold, preserving manual/security/other-budget holds.
- BUD-3 · Below-reach Allow once refuses without consuming the choice. At reach, Allow once durably grants L+floor(L/5) only when nonzero and above known spend. Deduplicate by operation ID; repeated extended reach offers no second extension. Only a genuinely changed limit/unit or new agent calendar period re-arms, retaining prior spend/history.

- BUD-4 · Calendar bounds use the selected known owner/HQ IANA zone/provenance and retain DST-aware period keys; reset closes old periods without erasing history and never clears unrelated holds.
- BUD-5 · Reached evaluations include verified ledger checkpoint, completeness/estimated/API-equivalent flags and missing/unpriced counts; no known capacity is promised by an incomplete below-limit subtotal.

## 7. Open questions

- Runtime adapter coverage: each adapter's exact token and cost scopes, cache overlap, process baselines and completeness are verified before its usage is called complete.

## 8. Not in v1

Detailed analytics: Progress and Speed sections, usual-time alerts, throughput comparisons and automatic wait detection. When built, they follow these rules:

- Throughput/completed lead/cycle populations use top-level typed tasks whose last completion lies in the period and still stands, excluding Cancelled/deleted records. Reopened incomplete items leave until completed again. Lead/cycle use full supported history; status intervals clip to the period. A release filter never rewrites accounting attribution.
- Use nearest-rank median/85th percentile with `n items`; below five show “—, n items”. A median opens its whole population; an elapsed-status total opens intervals that sum to it. Completion metrics attribute to assignee at completion, status time to assignee during each interval; later reassignment/membership changes cannot rewrite them. Usage consumer/accountable-team attribution remains separate.
- Status buckets are exclusive. Waiting on you is a union overlay of supported owner blockers/review/hand-off/questions/task-budget holds and available TIME-7 evidence. Never add it to status sums. An additive lead-time display removes overlap from other components before claiming a sum. Two overlapping one-hour waits spanning 90 minutes count 90 minutes, not 120.
- Usual cycle needs ten completed same-type items over 90 days. Over-usual compares supported lifecycle time since first Started against that type’s 85th percentile; never flag a never-started item. Usual throughput uses prior full, nonoverlapping spans of the compared length over 90 days, excluding current/partial-oldest spans; require two full spans and ten completions within them. Otherwise no comparison. Checks: 7d uses eleven prior spans, 30d two, 46d none; newer excluded completions cannot meet the minimum. Do not rank members.
