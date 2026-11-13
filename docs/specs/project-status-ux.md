---
kind: spec
title: "Project status: a visual status and a periodic report for each project (UX)"
status: for-review
revision: 4
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-08"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [features-ux, work-items-ux, costs-and-insights-ux, decisions-and-tech-debt-ux, retrospectives-ux, agent-run-modes-ux]
---

# Spec · Project status: a visual status and a periodic report for each project (UX)

Extends `features-ux.md` (feature progress), `work-items-ux.md` (tasks, bugs, time), `costs-and-insights-ux.md` (investment, Insights, `docs/reports/`), `decisions-and-tech-debt-ux.md` (debt, exceptions, ADRs), `retrospectives-ux.md`, `agent-run-modes-ux.md` (one-off and scheduled runs) and the smart-features candidates.

## The request (the user, 2026-11-08)

> Every once in a while I want a **status**, a **visualization of the status of my project**. One of those ephemeral (one-off) runs, using the smartness of the system, calculates it for us and **creates a report for each project**.

**Revision 4 (2026-11-08): review fixes.** Revision 3's note on waiting on you now states the narrow rule for decisions, as §4 does: only a task Blocked waiting for your decision on a proposed ADR, while it's the blocker. No criteria changed.

**Revision 3 (2026-11-08): aligned with ada's W7 model.** "Waiting on you" (the Flow tile, the health rule and Home › Projects status) counts what `costs-and-insights-ux.md` decision 5 counts: questions to you, Fixed bugs you reported, scoped requests, triage suggested to you and a task Blocked waiting for your decision on a proposed ADR, only while it's the blocker (§4). No criteria changed.

**Revision 2 (2026-11-08): review fixes.** Starting a report on demand is app-only (no `org-agent report status`). Example dates and figures are set in December 2026. Health reasons cite only the rules. The Stop on a running report cancels that run.

## 0. Decisions

1. **Two things, one source:** a live **Status** page per project (always current, no run needed) and a periodic **status report**, a dated snapshot with a written summary, saved in the repository (`docs/reports/<date>-status.md`). The report is the page frozen at a moment, plus what changed and what to do next.
2. **Numbers are facts, words are drafted.** The office computes every number (progress, cycle time, cost, bugs, debt) the same way as Insights. A **one-off run** (role "Reporter", `agent-run-modes-ux.md`) writes only the narrative: the summary, the risks and the recommendations, each citing the facts. The narrative is marked **Drafted** until the owner reads it, and nothing in it can change a number.
3. **Health in three words:** **On track · At risk · Off track**, from plain rules first (the defaults in §4: Off track when a Critical bug is open for more than a day; At risk when a feature in progress is past twice its typical cycle time, you've been waited on for more than 24 h, or the project is over budget) and the Reporter's reasoning second. The badge always shows **why** ("At risk: you've been waited on for 26 h, and FEAT-0006 is past twice its typical cycle time").
4. **When:** on a schedule per project (default: **weekly, Monday 08:00**, and also daily, after each delivered feature, or off) and **on demand** (**Generate report** on the Status page or ⌘K. Starting a one-off is app-only, so agents ask the owner instead.). A period with no activity produces a one-line report ("No changes since 30 Nov") without a run, so it costs nothing.
5. **It ends with what needs the owner:** decisions waiting (proposed ADRs and features, scoped requests, retro actions, expiring exceptions) and questions waiting, each a link. A report is useful only if it's actionable.
6. **The costs spec's monthly report becomes this report** with a month period (`costs-and-insights-ux.md` §3), so there's one report type, not two.

## 1. Flow 1 · The Status page (the project's **Overview** tab)

*The mock-ups in this spec and in `costs-and-insights-ux.md` are set in December 2026. The project started on 7 Nov.*

The project's Overview tab (`work-items-ux.md` decision 1) **is** this Status page. Its earlier tiles (needs triage, open bugs, tasks, brief) are part of it, so there's no separate Status tab.

```
 Agentic Org · Status                                   ● At risk             [Generate report]
 Why: you've been waited on for 26 h (3 questions) · FEAT-0006 is past twice its typical cycle time
 ┌ Features ──────────────────────────────┐ ┌ Investment ─────────────┐ ┌ Flow (last 7 d) ───────────┐
 │ FEAT-0007 Quick capture  ▓▓▓▓░░ 4/6     │ │ ≈ $1,240 to date         │ │ 31 tasks done · 9 bugs fixed│
 │ FEAT-0006 Run modes      ▓░░░░░ 1/7     │ │ ≈ $412 this month ▲18%   │ │ cycle time median 3 h 10 m │
 │ FEAT-0008 Retros         ░░░░░░ 0/9     │ │ budget $600 ▓▓▓▓▓░       │ │ waiting on you 26 h ⚠       │
 │ 5 delivered · 1 proposed                │ └──────────────────────────┘ └────────────────────────────┘
 └─────────────────────────────────────────┘
 ┌ Quality ───────────────────────────────┐ ┌ Needs you ─────────────────────────────────────────────┐
 │ Bugs open 7: 2 Critical · 1 Major · …   │ │ ADR-0004 Docs storage: proposed 2 d ago      [Open]     │
 │ Tech debt 14 (3 High) · 2 paid this week│ │ FEAT-0009 Costs scoped by Eden: ≈ $40–70     [Review]   │
 │ Exceptions: EXC-0002 due for review     │ │ 3 questions from ada and codex-dev            [Answer]   │
 └─────────────────────────────────────────┘ └────────────────────────────────────────────────────────┘
 Trend (8 weeks): ▁▂▃▅▆▇ done per week · cost per week · cycle time        [Table view]
 Latest report: Mon 7 Dec · On track → today At risk                       [All reports]
```

- Every tile opens the list behind it (the features, the bugs, the sessions behind the cost).
- **Charts:** the trend uses the dataviz rules (labelled values, never color alone, a table toggle for screen readers).
- **States:** a new project ("Not enough history yet: status appears after the first week of work". The tiles show what exists) · a project with no features (the Features tile becomes "Tasks: 12 open · 31 done") · a source unavailable (for example no usage data: "Cost not available for Antigravity runs", never $0).

## 2. Flow 2 · A status report

1. **Trigger:** the schedule, a delivered feature, or **Generate report**.
2. **Facts** (the office, seconds): a snapshot of every number on the Status page for the period, plus **what changed** since the last report (features delivered, items done, new Critical bugs, cost added, decisions made).
3. **Narrative** (a one-off run of the Reporter role, shown as "Reporter (one-off run 31)", about a minute): a 3–5 sentence summary, the top 3 risks with their evidence, and up to 3 recommendations ("Answer ada's questions: they block FEAT-0006"). Shown while it runs: "Writing the summary… **Stop**" (Stop cancels this run. It isn't the agent Stop, which means Pause.)
4. **Saved:** `docs/reports/2026-12-07-status.md` (Markdown with front matter: `kind: report`, `type: status`, `period`, `health`, `facts` (the snapshot), `run` (its cost)), committed through the docs flow (`decisions-and-tech-debt-ux.md` §0.2).
5. **Delivered:** Home shows "Status report for Agentic Org: **At risk**" with **Read**. A change of health (On track → At risk) also goes to Needs you.

**The report page:** header (project, period, health badge with why, "Written by Reporter (one-off run 31) · ≈ $0.12") · **Summary** (Drafted until read) · **What changed** · the Status tiles as they were at that moment · **Risks** · **Recommendations** (each with a one-click action where one exists: Open, Answer, Review) · **Needs you**. **Export ▾:** Markdown · PDF · Copy summary.

**All reports** (Docs › Reports) lists the reports with their health, so the project's history reads as a line of badges: ● ● ● ◐ ●.

## 3. Flow 3 · All projects at a glance (Home › Projects status)

```
 Projects                        Health       Features         This month    Waiting on you
 Agentic Org                     ● At risk    2 in progress    ≈ $412        3
 Lumen site                      ● On track   1 in progress    ≈ $58         0
 HR                              — idle       —                ≈ $3          0
```

Sorted by health (Off track first). Each row opens the project's Status page. **Generate all reports** runs one Reporter per project with activity.

## 4. Settings (project › Settings › Status reports)

Schedule: **Weekly ▾** (Daily · Weekly on <day> at <time> · After each delivered feature · Monthly · Off) · Health rules (the defaults, each editable: Critical bug open more than **1 day** → Off track · waiting on you more than **24 h** → At risk · a feature past **2×** its typical cycle time → At risk · over budget → At risk) · Reporter's model and runtime (default: the organization's copilot runtime) · Include a cost section ☑.

**Waiting on you** *(rev 3)*, in the health rule, the Flow tile and Home › Projects status, counts what `costs-and-insights-ux.md` decision 5 counts: questions to you, Fixed bugs you reported, scoped requests, triage suggested to you and tasks Blocked waiting for your decision on a proposed ADR, only while that decision is the task's blocker (ada Q11).

## 5. UX acceptance criteria (STATUS)

- STATUS-1 · Each project has a Status page with health, features, investment, flow, quality, Needs you and an 8-week trend, computed live by the office.
- STATUS-2 · Health is On track, At risk or Off track, and always shows the reasons that produced it.
- STATUS-3 · Every number on the Status page and in a report comes from office data, and each tile opens the items behind it.
- STATUS-4 · A report's summary, risks and recommendations come from a one-off run, cite the facts, and are marked Drafted until the owner opens the report.
- STATUS-5 · Reports run on the project's schedule (default weekly, Monday 08:00), after a delivered feature if chosen, and on demand from the Status page and ⌘K.
- STATUS-6 · A period with no activity produces a one-line report without a run.
- STATUS-7 · Each report is saved as `docs/reports/<date>-status.md` with front matter (period, health, the facts snapshot and the run's cost) and listed in Docs › Reports with its health.
- STATUS-8 · A new report appears on Home, and a change of health also appears in Needs you.
- STATUS-9 · Home › Projects status lists every project's health, features in progress, this month's cost and waiting-on-you count, sorted by health.
- STATUS-10 · Health rules and the schedule are editable per project. Reports export to Markdown and PDF.
- STATUS-11 · Charts have a table view and never rely on color alone. Missing data is shown as not available, never as zero.

## 6. Open questions

**For ada**
- A40 · The facts snapshot: one office query set shared with Insights (W14), with the report's front matter storing the snapshot so old reports stay true even if later data changes?
- A41 · The Reporter as a one-off run (A28) with read-only, tool-less input (smart features P2): the office passes the facts as data, and the run returns the narrative in a schema. OK?

**For the user**
- ~~U23~~ · *Resolved (the user, 2026-11-08): as recommended.* Defaults: a weekly report on Monday at 08:00, health rules as in §4, and the costs spec's monthly report folded into this one. OK?
