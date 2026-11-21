---
id: doc-60
title: 'Spec: Retrospectives and Routines (UX — Not in v1)'
type: specification
created_date: '2026-11-21T00:21:00Z'
updated_date: '2026-11-21T00:21:00Z'
tags:
  - spec
  - ux
  - retrospectives
  - routines
  - v2
  - not-in-v1
---
---
kind: spec
title: "Retrospectives: learn from each delivery, and apply it to every agent (UX)"
status: not-in-v1
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
requested_by: owner
builds_on: [features-ux, decisions-and-tech-debt-ux, work-items-ux, costs-and-insights-ux, agent-run-modes-ux]
---

> [!NOTE] Review note (2026-11-21)
> **Not in v1 (`status: not-in-v1`).** Preserved verbatim as `v2` / post-v1 UX specification so `v1` records (`report` docs, typed tasks, private task note boundaries, cost visibility rules) do not conflict with future scheduled routines and retrospectives.

# Spec · Retrospectives (not in v1)

Retrospectives are not in v1. Nothing in this spec is built for v1. It records the
plan so that v1 does not build something that conflicts with it.

## The request (the user, 2026-11-08)

> From time to time, after a larger delivery, the agents run a **retrospective**: what could we have done better, what did we do well that we should keep doing, what should we **stop** doing because it doesn't work, and why did problems happen, so they don't happen next time. Agents work fast, so retros are more frequent: at the end of each feature, or every X hours (every hour, three or four hours), **configurable**, **whichever comes first**. On a time interval, only the features **finished** in that period are included, never the ones in flight. After the retro, the **action items** become **tasks** and updated **procedures**, so next time every agent applies them. For example, if running tests always takes too long, the process for running tests (say, on a single branch) is improved and applied to all agents.

## What v1 must not contradict

- A retro's write-up is a `kind: report` supporting document in organization Git
  ([project documents](../organization-data/doc-76%20-%20Organization-Data-02-—-Project-Structure.md)): stable
  UUIDv7 ID, schema and revision, title, project, status `draft | current | deprecated`,
  owner and attribution, in the project's documentation tree under `docs/reports/` (or the
  feature's `reports/` folder for a single-feature report). It is not a new entity,
  kind or counter.
- Action items that are work are ordinary typed tasks
  ([document 03](../organization-data/doc-77%20-%20Organization-Data-03-—-Features-and-Work.md)) with an
  accountable team, project, immutable creator and history.
- A feature's `delivered_at` fact and `live` status do not trigger anything.
- Shared files and agent-visible evidence carry no cost amounts or budget figures
  ([document 17](../organization-data/doc-92%20-%20Organization-Data-17-—-Budgets-and-Controls.md)); private task
  notes never become retro evidence.

## The plan: routines

A **routine** has exactly one team or project scope, a kind (Retro, Check-in,
Review or Custom), one agent lead, participants resolved when it fires, a schedule
with an IANA time zone, instructions, skip rules, a grace period and an enabled or
paused state. Only the authenticated owner configures it; the lead may run, start,
close or skip where allowed. Team scope means direct home-team members. A retro runs
on its schedule or by Run now. The lead closes it with a structured outcome
(required summary; reviewed, closed, carried and linked items; optional decisions,
rules, actions and notes). Task actions go through the task service. A published
write-up uses the report document above.

Later extensions, after routines: feature-delivery triggers, automatic evidence
gathering, synthesis of participants' input, action items that change guidance,
checklists, templates or processes through review branches, and the next retro
checking whether the last one's actions helped.

## Acceptance criteria for routines (CER, RET)

- CER-1/2 · Every N is elapsed time, 15 minutes to 7 days, anchored to Starting at (default: creation time rounded up to the quarter hour); the first slot is strictly after now. Local-time days and 5-field cron follow IANA wall time with at most one slot per 15 minutes; a DST gap fires at the first valid minute and a fold fires once, at the first instance. The schedule shows date, zone and offset and previews the next three slots. Grace is 5 to 240 minutes.
- CER-3/4/5/11 · Each slot fires once, with duplicate-safe IDs and receipts, and at most one Announced or Running occurrence exists. Lead, participants, instructions and grace are fixed at announcement. The system sends one message to the lead and one to each participant except the lead and offboarded or excluded agents; no routine message goes to a person. Busy agents finish their turn first; paused agents wait; agents run by hand get it at their next message check. Only the announced lead starts or closes; no start by slot plus grace makes it Missed; four hours Running closes it without outcome. Whether an agent can run is decided by execution admission, never assumed from the routine's data.
- CER-6/7/12 · Skip precedence is: no lead, lead paused, previous occurrence still open, no open retro items. An outage beyond grace is recorded as server offline and coalesces lost slots; within grace it fires once, late, with the original deadline. Run now consumes the nearest eligible slot within half the interval (or the same local day for wall-time schedules). Losing an announced lead cancels an Announced occurrence, or closes a Running one without outcome, with reason "lead left"; neither counts as a miss.
- CER-8/12 · Exactly one non-dismissible Problem per routine for "missed twice" or "needs a lead". Skips, cancellations and lead-left outcomes do not count as misses. A successful outcome or the owner pausing, editing or deleting resets misses; assigning a valid lead, pausing, deleting or rejoining clears "needs a lead". A paused routine raises no "needs a lead" item until it is resumed. Routine outcomes are Activity, never an Inbox item. Routines never appear in New.
- RET-1/2/3/5 · Authenticated agents raise items without choosing a project. An item goes to an open retro the agent takes part in, else the current task's project, else the home team, else an eligible staffed project; otherwise it is saved Unscheduled in the current task's project or the home team. An item is never lost because scheduling is unavailable. The target is the earliest enabled next slot, with a stable tiebreaker, and the item attaches to that routine's open occurrence or its next scheduled one. Pausing, editing, changing kind, deleting or Run now keeps open and closed attached items. Carry-over increments only on Closed or Closed without outcome, never on Missed, Skipped or Cancelled. Lists show open items by default and all or closed explicitly, in server-record order with "raised during" labels.
- RET-4/6 · An item can be closed or reopened only by its author, the current or announced lead, an authorized scope manager or the owner, with a close note; the service checks the right. Item text is 1 to 2,000 Unicode code points after trimming, with no control, format or line-separator characters, and is rendered as plain untrusted text with no executable markup or clickable arbitrary links; linked references are filtered by visibility. Private task notes never become retro evidence.
- RET-7 · Nothing is imported. Attendance is not derived from message read state.

## Retrospective criteria (RETRO)

- RETRO-1 · Withdrawn: replaced by routine schedules (CER-1).
- RETRO-2 · Withdrawn: replaced by routine occurrences and their attached items (RET-1).
- RETRO-3 · Not in v1: an evidence panel gathered automatically for each retro.
- RETRO-4 · Withdrawn: replaced by routine participants (CER-3).
- RETRO-5 · Not in v1: a synthesis that groups duplicate input with counts and evidence.
- RETRO-6 · Not in v1: action items of kinds Guidance, Checklist, Template, Decision, Rule and Process, each with an exact diff.
- RETRO-7 · Withdrawn: owner approval of every action item is replaced by the lead's structured close.
- RETRO-8 · Not in v1: a merged change reaching each agent at its next start.
- RETRO-9 · Not in v1: checking whether the previous retro's actions helped.
- RETRO-10 · Not in v1: the retro write-up as a report document without cost amounts.
- RETRO-11 · Withdrawn: there is no owner approval to skip.
- RETRO-12 · Not in v1: `agent-os` commands for retros.

## Open questions

- Routine storage and IDs need their own design before routines are built.
- Where routines appear on team and project pages is not designed.
