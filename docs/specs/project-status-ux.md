---
kind: spec
title: "Project status: a visual status and a periodic report for each project (UX)"
status: for-review
revision: 9
owner: lead
created: "2026-11-08"
updated: "2026-11-15"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [features-ux, work-items-ux, costs-and-insights-ux, decisions-and-tech-debt-ux, retrospectives-ux, agent-run-modes-ux]
---

# Spec · Project status: a visual status and a periodic report for each project (UX)

Extends `features-ux.md` (feature progress), `work-items-ux.md` (tasks, bugs, time), `costs-and-insights-ux.md` (investment, Insights, organization-Git project `reports/`), `decisions-and-tech-debt-ux.md` (debt, exceptions, ADRs), `retrospectives-ux.md`, `agent-run-modes-ux.md` (one-off and scheduled runs) and the smart-features candidates.

## The request (the user, 2026-11-08)

> Every once in a while I want a **status**, a **visualization of the status of my project**. One of those ephemeral (one-off) runs, using the smartness of the system, calculates it for us and **creates a report for each project**.

**Revision 9 (2026-11-15): integrated reporting requirements.** Adds shared metric references and condensed deferred Flow/digest behavior. Status and report presentation remain a UX proposal; automatic reporting schedules, delivery triggers and Reporter orchestration are deferred. [Projects and documents](../design/organization-data/02-project-structure.md), [typed tasks/features](../design/organization-data/03-features-and-work.md), [accounting](../design/organization-data/10-usage-and-cost-accounting.md) and [budget controls](../design/organization-data/17-budgets-and-controls.md) own their data. Budget/accounting services are not implemented by this spec. Compatible earlier PRD requirements are integrated into these existing contracts; contradictory older rules do not override them. Retrospective actions and area-based metrics are independently deferred.

A report is a supporting document in organization Git: required project, optional at most one same-project feature/release/deployment scope, stable ID, schema/revision, title, `kind: report`, `status: draft | current | deprecated`, owner and creation/update attribution. Current requires an active owner. Store a project report at `projects/<project-id>/reports/<document-id>.md`; use feature `reports/` or release/deployment `documents/` for those narrower scopes. Dates belong in metadata, not identity. New IDs use ULID; retained imported IDs do not change. Keep the report narrative and safe fact snapshot separate from protected monetary snapshots.

## 0. Decisions

1. **Two things, one source:** a live **Status** page per project (always current, no run needed) and a periodic **status report**, a dated snapshot with a written summary, saved in the repository (`projects/<project-id>/reports/<document-id>.md`). The report is the page frozen at a moment, plus what changed and what to do next.
2. **Numbers are facts, words are drafted.** The office computes every number (progress, cycle time, cost, bugs, debt) the same way as Insights. A **one-off run** (role "Reporter", `agent-run-modes-ux.md`) writes only the narrative: the summary, the risks and the recommendations, each citing the facts. The narrative is marked **Drafted** until the owner reads it, and nothing in it can change a number.
3. **Health in three words:** **On track · At risk · Off track**, from plain rules first (the defaults in §4: Off track when a Critical bug is open for more than a day; At risk when a feature in progress is past twice its typical cycle time, you've been waited on for more than 24 h, or an unresolved task Budget reached item is open on the project's work *(rev 5)*) and the Reporter's reasoning second. The badge always shows **why** ("At risk: you've been waited on for 26 h, and F-6 is past twice its typical cycle time").
4. **Deferred scheduling:** on a schedule per project (default: **weekly, Monday 08:00**, and also daily, after each delivered feature, or off) and **on demand** (**Generate report** on the Status page or ⌘K. Starting a one-off is app-only, so agents ask the owner instead.). A period with no activity produces a one-line report ("No changes since 30 Nov") without a run, so it costs nothing.
5. **It ends with what needs the owner:** decisions waiting (decision proposals and scoped request tasks; retrospective actions and expiring exceptions remain deferred integrations) and questions waiting, each a link. A report is useful only if it's actionable.
6. **The costs spec's monthly report becomes this report** with a month period (`costs-and-insights-ux.md` §3), so there's one report type, not two.

## 1. Flow 1 · The Status page (the project's **Overview** tab)

The initial [project Overview](work-items-ux.md#3-flow-1--project-page) keeps its
brief, counts and recent activity. This richer status/reporting proposal does
not replace that view before its data and presentation services are selected.

*(rev 5)* **Who sees cost:** the Investment tile and the cost trend show amounts only to the owner and the project's manager (document 17 and COST-7). Everyone else sees the tile as the budget state only, for example "1 budget reached", with no dollar figure, or "No budgets reached" when none is.

*The mock-ups in this spec and in `costs-and-insights-ux.md` are set in December 2026. The project started on 7 Nov.*

In the deferred reporting experience, the project's Overview tab **would be** this Status page. Its earlier tiles (needs triage, open bugs, tasks, brief) are part of it, so there's no separate Status tab.

```
 Agentic Org · Status                                   ● At risk             [Generate report]
 Why: you've been waited on for 26 h (3 questions) · F-6 is past twice its typical cycle time
      · 1 task budget stop pending
 ┌ Features ──────────────────────────────┐ ┌ Investment ─────────────┐ ┌ Flow (last 7 d) ───────────┐
 │ F-7 Quick capture  ▓▓▓▓░░ 4/6     │ │ ≈ $1,240 to date         │ │ 31 tasks done · 9 bugs fixed│
 │ F-6 Run modes      ▓░░░░░ 1/7     │ │ ≈ $412 this month ▲18%   │ │ cycle time median 3 h 10 m │
 │ F-8 Retros         ░░░░░░ 0/9     │ │ 1 budget reached ⚠       │ │ waiting on you 26 h ⚠       │
 │ 5 live · 1 planned                │ └──────────────────────────┘ └────────────────────────────┘
 └─────────────────────────────────────────┘
 ┌ Quality ───────────────────────────────┐ ┌ Needs you ─────────────────────────────────────────────┐
 │ Bugs open 7: 2 Critical · 1 Major · …   │ │ ADR-0004 Docs storage: proposed 2 d ago      [Open]     │
 │ Debt: deferred proposal               │ │ F-9 Costs scoped by Eden: ≈ $40–70     [Review]   │
 │ Exceptions: deferred proposal           │ │ 3 questions from ada and codex-dev            [Answer]   │
 └─────────────────────────────────────────┘ └────────────────────────────────────────────────────────┘
 Trend (8 weeks): ▁▂▃▅▆▇ done per week · cost per week · cycle time        [Table view]
 Latest report: Mon 7 Dec · On track → today At risk                       [All reports]
```

- Every tile opens the list behind it (the features, the bugs, the usage records behind the cost).
- **Charts:** the trend uses the dataviz rules (labelled values, never color alone, a table toggle for screen readers).
- **Metric consistency:** Flow, reports and future Insights use the [shared duration definitions and deferred analytics calculations](costs-and-insights-ux.md#shared-duration-definitions): exclude terminal gaps, retain unknown baselines, show sample sizes and never label status time as working time. The illustrated Flow tile is a proposal; the detailed replacement below stays deferred.
- **States:** a new project ("Not enough history yet: status appears after the first week of work". The tiles show what exists) · a project with no features (the Features tile becomes "Tasks: 12 open · 31 done") · a source unavailable (for example no usage data: "Usage incomplete: Antigravity doesn't report usage", never $0).

## 2. Flow 2 · A status report

1. **Proposed trigger:** Generate report; schedule and delivered-feature triggers remain deferred. The delivery fact is explicit feature file data, not an all-tasks-done engine.
2. **Facts** (the office, seconds): a snapshot of every number on the Status page for the period, plus **what changed** since the last report (features delivered, items done, new Critical bugs, cost added, decisions made).
3. **Narrative** (a one-off run of the Reporter role, shown as "Reporter (one-off run 31)", about a minute): a 3–5 sentence summary, the top 3 risks with their evidence, and up to 3 recommendations ("Answer ada's questions: they block F-6"). Shown while it runs: "Writing the summary… **Stop**" (Stop requests cancellation of this report turn; show cancellation complete only after work/process cleanup is confirmed. The selected StopAgent command stops active and idle main processes and creates no durable pause, future-wake or budget policy. If continued exclusion from future work is intended, use a separate explicit Pause/hold control with its own durable policy; no such scheduling/pause engine is selected by this report flow.)
4. **Saved:** `projects/<project-id>/reports/<document-id>.md` in organization Git, with stable id, schema_version, revision, project_id, null feature/release/deployment scope IDs, kind report, title, draft/current/deprecated status, owner_employee_id and common actor/time attribution. Period, health, safe facts and run tokens appear in the body pending a selected specialized report-metadata extension. The entire shared file omits amounts/limit figures; it may show tokens, incomplete-data labels and authorized Budget reached state. Proposed reviewed publication follows `decisions-and-tech-debt-ux.md` §0.2; protected amount snapshots need their own durable source outside Git.
5. **Delivered:** Home shows "Status report for Agentic Org: **At risk**" with **Read**. A change of health (On track → At risk) also goes to Needs you.

**The report page:** header (project, period, health badge with why, "Written by Reporter (one-off run 31) · ≈ $0.12"; the run's cost shows only to the owner and the project's manager *(rev 5)*) · **Summary** (Drafted until read) · **What changed** · the Status tiles as they were at that moment (amounts, for those who may see them, come from the protected durable snapshot, projected into the office cache, A40) *(rev 6)* · **Risks** · **Recommendations** (each with a one-click action where one exists: Open, Answer, Review) · **Needs you**. **Export ▾:** Markdown · PDF · Copy summary. Shared exports retain the same amount/privacy restrictions; authorized app amounts never leak through narrative, links or exports.

**All reports** (Docs › Reports) lists the reports with their health, so the project's history reads as a line of badges: ● ● ● ◐ ●.

## 3. Flow 3 · All projects at a glance (Home › Projects status)

*(rev 5)* **Who sees cost:** the This month column shows only to the owner and to each project's manager for their projects (a manager of some projects sees those rows' cost cell empty, with "Not your project" on hover and to screen readers) (document 17 and COST-7). Everyone else sees the table without it. The same rule covers the Status page's Investment tile and cost trend (§1) and the report's cost section and run cost (§2).

```
 Projects                        Health       Features         This month    Waiting on you
 Agentic Org                     ● At risk    2 in progress    ≈ $412        3
 Lumen site                      ● On track   1 in progress    ≈ $58         0
 HR                              — idle       —                ≈ $3          0
```

Sorted by health (Off track first). Each row opens the project's Status page. **Generate all reports** runs one Reporter per project with activity.

## 4. Settings (project › Settings › Status reports)

**Deferred schedule settings:** **Weekly ▾** (Daily · Weekly on <day> at <time> · After each delivered feature · Monthly · Off) · Health rules (the defaults, each editable: Critical bug open more than **1 day** → Off track · waiting on you more than **24 h** → At risk · a feature past **2×** its typical cycle time → At risk · a Budget reached item open on the project's work → At risk *(rev 5)*) · Reporter's model and runtime (default: the organization's copilot runtime) · Include a cost section ☑ (in the app, amounts follow document 17 and COST-7; the saved file keeps tokens and "Budget reached" only, A109).

**Waiting on you** *(rev 3)*, in the health rule, the Flow tile and Home › Projects status, counts what `costs-and-insights-ux.md` decision 5 counts: questions to you, Fixed bugs you reported, scoped requests, triage suggested to you and tasks Blocked waiting for your decision on a proposed ADR, only while that decision is the task's blocker (ada Q11).

## 5. UX acceptance criteria (STATUS)

- STATUS-1 · *(rev 5)* Each project has a Status page with health, features, investment, flow, quality, Needs you and an 8-week trend, computed live by the office. Investment shows amounts only to the owner and the project's manager; others see the budget state (§1, document 17 and COST-7).
- STATUS-2 · Health is On track, At risk or Off track, and always shows the reasons that produced it.
- STATUS-3 · Every number on the Status page and in a report comes from verified projections of canonical records and the durable usage ledger, with freshness/checkpoint and completeness labels, and each tile opens the items behind it *(rev 5)*: for someone who can't see amounts, the Investment tile opens the tasks and bugs stopped or held by their budget, not the usage records.
- STATUS-4 · **Deferred Reporter orchestration:** A report's summary, risks and recommendations come from a one-off run, cite the facts, and are marked Drafted until the owner opens the report.
- STATUS-5 · **Deferred scheduling and delivery trigger:** Reports run on the project's schedule (default weekly, Monday 08:00), after a delivered feature if chosen, and on demand from the Status page and ⌘K.
- STATUS-6 · **Deferred automatic no-activity optimization:** A period with no activity produces a one-line report without a run.
- STATUS-7 · *(rev 6)* Each report is saved as `projects/<project-id>/reports/<document-id>.md` with the selected document envelope (§2); period, health, safe facts and run tokens are body content until a specialized metadata extension is selected and listed in Docs › Reports with its health. The whole saved file (front matter, cost section, narrative and Needs you) has no cost amounts or budget figures: it shows tokens and "Budget reached" where it applies, with a link to the project in the app for amounts (A109).
- STATUS-8 · A new report appears on Home, and a change of health also appears in Needs you.
- STATUS-9 · *(rev 5)* Home › Projects status lists every project's health, features in progress, this month's cost and waiting-on-you count, sorted by health. The cost column shows amounts only to the owner and each project's manager (document 17 and COST-7); other members see no cost column.
- STATUS-10 · Health rules are a proposed editable presentation policy; **deferred scheduling** needs its own service/configuration contract. Reports export to Markdown and PDF.
- STATUS-11 · Charts have a table view and never rely on color alone. Missing data is shown as not available, never as zero.
- STATUS-12 · **Deferred analytics:** Flow and Insights agree for identical explicit periods/checkpoints. A Flow link preserves rolling 7d; filtering Flow never changes Progress. Median values below five completed items show “—, n items”.
- STATUS-13 · **Deferred private digest:** guessed-ID reads by another account or agent reveal no away window/digest; exports and satellites receive none. A daily budget reset cannot resolve a separate monthly problem; a release warning cap is not a problem.

## 6. Open questions

Budget state distinguishes `stop_pending` from `held`: reaching a threshold does not prove work stopped. Manual/security/other-budget holds remain independent. Release caps warn only and do not produce task Budget reached items. Reports may display authorized safe state, never private limit/history amounts in shared files. Tokens, missing/unpriced usage and captured release attribution follow document 10; moving a task or changing a release manifest does not rewrite historical consumption.

**For ada**
- A40 · Preserve a full immutable snapshot in a protected durable source outside shared Git, with an authorized SQL projection for owner/project-manager report views. Selected organization-Git files contain safe facts/tokens only. Serialization, backup, capture/revision receipts and retrieval remain implementation design; never treat an office-only row as the sole durable source. Corrections are explicit revised reports, not silent historical recomputation.
- A41 · The Reporter as a one-off run (A28) with read-only, tool-less input (smart features P2): the office passes the facts as data, and the run returns the narrative in a schema. OK? *(rev 6)* The facts it gets have no cost amounts (tokens and "Budget reached" only), so the narrative can't carry them into the saved file (A109).
- ~~A109~~ · *Resolved (Emery, 2026-11-13; rev 6):* report files leave out cost amounts and budget figures. They may say "Budget reached", show tokens, and link to the app, where document 17 and COST-7 decides who sees amounts. Repository access isn't a permission model (STATUS-7).

**For the user**
- ~~U23~~ · *Resolved (the user, 2026-11-08): as recommended.* Defaults: a weekly report on Monday at 08:00, health rules as in §4, and the costs spec's monthly report folded into this one. OK?

### Deferred Progress and Flow requirements

These requirements retain the earlier project-insights direction without selecting an analytics or planning engine. When selected, Insights follows Conversations as the last tab, with Progress, Speed, then Cost. Progress shows project completion, open-release rows followed by collapsed closed rows newest close first, a release-switchable burn-up, then open milestones. Period/team/member filters live in the URL and affect Flow only; say “Progress isn't filtered”.

**Release states are derived (owner decision 2026-11-17).** A release stores no status. Where this spec says Planned, In progress, open, closed or Released, read: *done* when the [completion rule](../design/organization-data/02-project-structure.md#completion-of-milestones-and-releases) holds; *in progress* when it is not done and at least one of its tasks has left `todo`; *planned* otherwise; *released* when it has a production deployment record ([document 07](../design/organization-data/07-release-deployments.md)).

The proposed Flow tile shows rolling-7d completed count/median cycle time, current-release completion and at most three open milestone rows, then “+N more”. Current release means earliest In progress, else earliest Planned in the release picker order; no open release leaves count/cycle only. Its link preserves 7d. Use the shared sample/population rules; no member ranking view.

Burn-up uses retained scope/history at each bucketing-day end and now today, with linked scope/done-change markers. Freeze at release/cancellation; an explicitly selected reopen resumes actual historical daily states. A release created today shows one point and “The trend starts tomorrow”. Milestones have rows; milestone burn-up, points and forecasts remain separately deferred. These scope figures never move historical ledger spend by current membership.

### Deferred account digest requirements

An away digest is a separate private account feature, not a shared project report. Its persistence, activity capture, display-time policy and authorization contract remain pending. Away/back instants, durations, digest, seen state and daily summaries stay on HQ, visible only to that account; exclude them from shared history/feed, other accounts, agent context, exports and satellite replication. Guessed-ID reads return not found. Existing coarse presence/last_seen grants no window access.

If selected, Away starts at last input across devices and is confirmed after 10m with no input or immediately when no tab is visible. Only later input in a visible tab ends it; session expiry alone changes neither boundary. HQ owns live time; delayed reports are bounded to known disconnect/reconnect instants. A closed offline tab may lose its report and leave a false window the owner can mark seen.

The qualifying Home card precedes Needs you without replacing it. Prospective thresholds are 10m/30m/1h default/4h/Off. Only Mark as seen syncs seen/hides the card; unseen qualifying windows merge, including active-gap events. Window history retains 30 days; separately selected private daily summaries retain 90 days with their original zone/bounds. Account-zone switching needs contiguous daily boundaries; it cannot rewrite old summaries or budget periods. No retention service is implemented here.

Sections are Problems, Delivered, Progress, later Decisions, Conversations, later Speed. Omit unavailable sources rather than show zero. Facts derive from durable records, not agent prose; every claim links to an authorized source or safe deleted-record evidence. Delivered uses the last completion that stands at return: completion then reopen is absent, completion again counts once. Problems retain start/resolve facts even when resolved before return, combine repeated same-kind occurrences with latest outcome/count, and preserve frozen historical text with explicitly live resolved/reopened/Needs you annotations.

Daily/monthly budget problems stay independent. Release caps and neutral resets are not problems; budget-caused blocks do not duplicate Blocked lines. Validated late events join their original window, update history with “Updated later”, and resurrect a seen Home card only for a new Problem; outside-window events join none. Do not rewrite immutable accounting timestamps using a digest clock-skew rule. A future display timestamp preserves the source evidence. Speed/usual comparisons use the cost spec's deferred minimums; exact digest elapsed spans use “{h} h {m} min” (minutes alone below 1h), separate from the aggregate duration formatter. Reporter, schedule, routine and retrospective orchestration remain deferred.
