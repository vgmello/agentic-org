---
kind: spec
title: "Project status: a visual status and a periodic report for each project (UX)"
status: not-in-v1
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
requested_by: owner
builds_on: [features-ux, work-items-ux, costs-and-insights-ux, decisions-and-tech-debt-ux, agent-run-modes-ux]
---

# Spec · Project status: a visual status and a periodic report for each project (UX)

The Status page and status reports are not in v1. In v1 a project's Overview is the
one described in [work items](work-items-ux.md#3-flow-1--project-page): brief, counts
and recent activity. This spec records the planned design so that v1 does not
contradict it.

## The request (the user, 2026-11-08)

> Every once in a while I want a **status**, a **visualization of the status of my project**. One of those ephemeral (one-off) runs, using the smartness of the system, calculates it for us and **creates a report for each project**.

## What v1 must not contradict

- A report is a supporting document in organization Git
  ([project documents](../design/organization-data/02-project-structure.md)):
  required project, at most one same-project feature, release or deployment scope,
  stable UUIDv7 ID, schema and revision, title, `kind: report`,
  `status: draft | current | deprecated`, owner and attribution. A project report is
  at `projects/<project-id>/reports/<document-id>.md`. Dates belong in metadata, not
  identity.
- A shared report file has no cost amounts or budget figures. It may say "Budget
  reached", show tokens and link to the app, where document 17 and COST-7 decide who
  sees amounts. Repository access is not a permission model.
- Every number comes from projections of canonical records and the durable usage
  ledger ([document 10](../design/organization-data/10-usage-and-cost-accounting.md)),
  with freshness and completeness labels. Missing data is shown as not available,
  never zero. Moving a task or changing a release manifest does not rewrite
  historical consumption.
- Budget state distinguishes stop pending from held; reaching a threshold does not
  prove work stopped. Release caps only warn
  ([document 17](../design/organization-data/17-budgets-and-controls.md)).
- Releases store no status. Done is derived by the
  [completion rule](../design/organization-data/02-project-structure.md#completion-of-milestones-and-releases);
  in progress means not done with at least one task out of `todo`; planned otherwise;
  released means a succeeded production deployment record
  ([document 07](../design/organization-data/07-release-deployments.md)).

## The plan

1. **Two things, one source:** a live **Status** page per project, and a periodic
   **status report**, a dated snapshot with a written summary, saved as a report
   document. The report is the page frozen at a moment, plus what changed and what
   to do next. The costs spec's monthly report is this report with a month period.
2. **Numbers are facts, words are drafted.** The office computes every number the
   same way as Insights. A Reporter writes only the narrative (summary, top risks,
   recommendations), each citing the facts, from facts that contain no cost amounts.
   The narrative is marked **Drafted** until the owner reads it and cannot change a
   number.
3. **Health in three words:** **On track · At risk · Off track**, from plain rules
   (Off track when a Critical bug is open more than a day; At risk when a feature in
   progress is past twice its typical cycle time, the owner has been waited on more
   than 24 h, or a task's Budget reached item is open) and the Reporter's reasoning
   second. The badge always shows why.
4. **Waiting on you** counts what `costs-and-insights-ux.md` decision 5 counts.
5. **It ends with what needs the owner:** decisions and questions waiting, each a
   link.

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
 │                                         │ │ F-9 Costs scoped by Mira: ≈ $40–70     [Review]   │
 │                                         │ │ 3 questions from sam and codex-dev            [Answer]   │
 └─────────────────────────────────────────┘ └────────────────────────────────────────────────────────┘
 Trend (8 weeks): ▁▂▃▅▆▇ done per week · cost per week · cycle time        [Table view]
 Latest report: Mon 7 Dec · On track → today At risk                       [All reports]
```

The Investment tile and cost trend show amounts only to the owner and the project's
manager; others see the budget state only. Charts follow the dataviz rules (labelled
values, never color alone, a table view). Metrics use the
[shared duration definitions](costs-and-insights-ux.md#shared-duration-definitions).
Home › Projects status lists every project's health, features in progress, this
month's cost (amounts under the same rule) and waiting-on-you count, sorted by
health.

Later still: progress and flow charts (burn-up per release, rolling 7-day flow,
milestone rows), and a private away digest per account (what happened while the
person was away, visible only to that account and never in shared history,
exports, agent context or satellite replication).

## Acceptance criteria (STATUS)

- STATUS-1 · Not in v1: a Status page per project with health, features, investment, flow, quality, Needs you and an 8-week trend. Investment shows amounts only to the owner and the project's manager.
- STATUS-2 · Not in v1: health On track, At risk or Off track, always with its reasons.
- STATUS-3 · Not in v1: every number on the Status page and in a report comes from projections of canonical records and the durable usage ledger, with freshness and completeness labels, and each tile opens the items behind it; for someone who can't see amounts, the Investment tile opens the tasks stopped or held by their budget.
- STATUS-4 · Not in v1: a Reporter narrative that cites facts and is marked Drafted until read.
- STATUS-5 · Not in v1: scheduled and on-demand reports.
- STATUS-6 · Not in v1: a one-line report without a run for a period with no activity.
- STATUS-7 · Not in v1: each report saved as a report document with no cost amounts or budget figures anywhere in the file.
- STATUS-8 · Not in v1: new reports on Home and health changes in Needs you.
- STATUS-9 · Not in v1: Home › Projects status, with cost amounts only for the owner and each project's manager.
- STATUS-10 · Not in v1: editable health rules and report export to Markdown and PDF.
- STATUS-11 · Not in v1: charts with a table view that never rely on color alone, and missing data shown as not available.
- STATUS-12 · Not in v1: Flow and Insights agree for identical periods; a median below five completed items shows "—, n items".
- STATUS-13 · Not in v1: a private away digest that no other account, agent, export or satellite can read.

## Open questions

- Protected amount snapshots for reports need a durable source outside shared Git,
  with an authorized projection for owner and project-manager views. Its format,
  backup and retrieval are not designed.
