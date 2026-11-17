---
kind: spec
title: "Retrospectives: learn from each delivery, and apply it to every agent (UX)"
status: for-review
revision: 7
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-15"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [features-ux, decisions-and-tech-debt-ux, work-items-ux, costs-and-insights-ux, agent-run-modes-ux]
---

# Spec · Retrospectives: learn from each delivery, and apply it to every agent (UX)

Extends `features-ux.md` (delivered features), `feature-requests-ux.md`, `decisions-and-tech-debt-ux.md` (docs, agent guidance §6.2, templates §3.2), `work-items-ux.md` (checklists §9.1, time §9.2), `costs-and-insights-ux.md` and `agent-run-modes-ux.md`.

## The request (the user, 2026-11-08)

> From time to time, after a larger delivery, the agents run a **retrospective**: what could we have done better, what did we do well that we should keep doing, what should we **stop** doing because it doesn't work, and why did problems happen, so they don't happen next time. Agents work fast, so retros are more frequent: at the end of each feature, or every X hours (every hour, three or four hours), **configurable**, **whichever comes first**. On a time interval, only the features **finished** in that period are included, never the ones in flight. After the retro, the **action items** become **tasks** and updated **procedures**, so next time every agent applies them. For example, if running tests always takes too long, the process for running tests (say, on a single branch) is improved and applied to all agents.

**Revision 7 (2026-11-15):** retains the later routine-based retrospective requirements as explicitly deferred, with selected report/task/private-source contracts still authoritative.

**Revision 6 (2026-11-15): deferred proposal reconciled with selected storage.** The retrospective engine, its automatic triggers, participant wakes, synthesis, action application and next-run comparisons remain **unselected/deferred**. The earlier RETRO criteria below remain deferred historical alternatives; the later routine-based proposal below supersedes their trigger, participation and owner-approval behavior. Neither is launch scope. Selected feature lifecycle is `planned | in_progress | live | retired`, with explicit `delivered_at`; it does not supply an automatic delivery trigger. This proposal must use [project documents](../design/organization-data/02-project-structure.md), [tasks/features](../design/organization-data/03-features-and-work.md), [record contracts](../design/organization-data/08-record-contracts.md), [usage](../design/organization-data/10-usage-and-cost-accounting.md) and [budget controls](../design/organization-data/17-budgets-and-controls.md). The absent costs-and-budgets-v1 PRD is historical context, not authority.

If selected later, a retro's persistent write-up is a `kind: report` supporting document in organization Git, not a new selected entity/kind/counter. It requires stable ID (ULID for new records), schema/revision, title, project, status `draft | current | deprecated`, owner and attribution; current requires an active owner. Use `projects/<project-id>/reports/<document-id>.md`, or feature `reports/<document-id>.md` for a single-feature report. Period, participants, evidence and proposed actions can be explained in the body pending a separately selected workflow schema. Imported IDs stay unchanged. Task actions use the one typed task entity with accountable team, project, immutable creator and retained history; office caches are not their sole source.

## Deferred routine-based retrospective proposal

If a routine engine is separately selected, a routine has exactly one team/project scope, kind Retro/Check-in/Review/Custom, one valid agent lead, participants resolved at fire time, schedule with IANA zone, instructions, skip rules, grace and enabled/paused state. Only the authenticated owner configures it; the lead may run/start/close/skip where allowed. People-leading/configuration awaits sign-in; team scope means direct home-team members, not descendants. Project participation/manager authorization must use the selected project relationship, never “any attached team's lead”. Routine data IDs/storage require their own reviewed contract; the proposed RT-/RI- numbering is not selected by this UX.

A retro runs by schedule or Run now. Feature-delivery triggers, automatic evidence gathering, owner approval of all actions, and automatic procedure application are later extensions. The lead closes with a structured outcome (required summary; reviewed/closed/carried/linked work items; optional decisions/rules/actions/notes), with ordinary typed task actions admitted through the task service rather than automatic owner approval. Invalid close input applies nothing. A persistent published write-up uses the selected report envelope/path; private items/cost amounts cannot enter shared Git or synthesis. No new operational persistence scheme is inferred.

Retain the following conditional acceptance groups (CER/RET IDs are from the later source):

- CER-1/2: Every N is elapsed time, 15 min–7 days, anchored to Starting at (default creation time rounded up to the quarter hour); first slot is strictly after now. Local-time days/5-field cron follow IANA wall time, minimum 15-minute density; DST gaps fire at the first valid minute, folds once at the first instance. Display schedule date/zone/offset and preview three next slots. Grace is 5–240 minutes.
- CER-3/4/5/11: fire each slot once with duplicate-safe retained IDs/receipts and at most one Announced/Running occurrence. Snapshot lead/participants/instructions/grace at announcement. System sends one lead message and participant messages excluding lead/offboarded/excluded agents; no routine message to a person. Busy agents finish their turn; paused agents wait; hand-run agents await message checks. Only the announced lead starts/closes; no-start at slot+grace becomes Missed; four hours Running becomes Closed without outcome. Runtime admission/wake is separately gated, never guaranteed from data alone.
- CER-6/7/12: skip precedence is no lead, lead paused, previous occurrence still open, no open retro items. Outage beyond grace wins as server offline and coalesces lost slots; within grace fire once late with the original deadline. Run now consumes the nearest eligible slot within half the interval (or same local day for wall-time schedules). Losing an announced lead cancels an Announced occurrence, or closes Running without outcome, with reason lead left; both are neutral for consecutive misses.
- CER-8/12 and IA D44: exactly one non-dismissible Problem per routine for missed twice or needs a lead. Skips/cancels/lead-left outcomes are neutral for misses. A successful outcome or owner pause/edit/delete resets misses; assigning a valid lead/pausing/deleting/rejoining clears needs-a-lead. Paused routines raise no needs-a-lead item until resumed. Routine outcomes are Activity, never Retro ready in Inbox. Defer Routines tabs/pages on teams/projects; never put routines in New. Their exact route/filter/tab placement awaits design selection.
- RET-1/2/3/5: authenticated agents raise items without choosing a project; resolve first an open participating retro, then current-task project, then home team, then an eligible staffed project; otherwise save Unscheduled in current-task project or required home team. Never lose an item because scheduling is unavailable. Deterministically choose the earliest enabled next slot then stable tiebreaker; attach to the target's open occurrence or next Scheduled one. Pause/edit/kind-change/delete/run-now preserve both open and already-closed attached items. Increment carry-over only on Closed/Closed-without-outcome, not Missed/Skipped/Cancelled. List open by default and all/closed explicitly; preserve server-record order and raised-during labels.
- RET-4/6: allow item close/reopen only to its authenticated author, the authorized current/announced lead, authorized scope manager or owner under a selected grant. Require close note. Item text is 1–2,000 Unicode code points after trimming; reject control/format/line separators. Render as plain untrusted text, no executable markup or clickable arbitrary links; linked references remain visibility-filtered. Private task notes never become retro evidence.
- RET-7: migration from the interim tool remains provisional; source format/import trigger/identity/retention/attachment need a reviewed migration contract before any import or old-file deletion. Attendance from message read state remains unresolved; omit it until validated rather than fabricate it.



## 0. Earlier deferred alternative

1. **A retro runs on whichever comes first:** a **feature is delivered** (a feature has an explicit delivery fact, `features-ux.md`; trigger/deduplication semantics remain deferred), or **the interval** passes (default every 4 h, from 1 h to 1 week). Both are configurable per project, and either can be turned off. Running a retro resets the clock.
2. **A retro covers finished work only.** Its scope is the features (and their tasks and bugs) that finished since the last retro. Work still in flight waits for the next one. A period with nothing finished is **skipped** and noted ("Nothing finished since 14:00. No retro"), so it costs nothing.
3. **Four questions, in plain words:** **Keep doing** (what worked) · **Do better** (what could improve) · **Stop doing** (what doesn't work) · **Problems** (what went wrong, **why**, and how to prevent it next time).
4. **Evidence first, opinions second.** Before anyone answers, the tool gathers the period's facts: the features and their cycle times, waiting on the owner, blocked time, review rounds, reopened bugs, failed runs, tests and their time (when reported), debt taken, exceptions, and cost. Every answer can cite them ("T-17 needed 5 review rounds").
5. **The agents who did the work take part.** Everyone with a finished task in the scope is asked for input (woken if on call), plus the **facilitator**, by default the project's lead, who merges the answers, groups duplicates, and proposes action items. The owner can add input any time and is never required to.
6. **Action items are changes, not wishes.** Each has a **kind**: **Task** (work to do) · **Guidance** (a change to agent guidance, §6.2 of the decisions spec) · **Checklist** (a change to a checklist template) · **Template** (a doc template) · **Decision** (a new ADR) · **Rule** (AGENTS.md or the project brief) · **Process** (a deferred change workflow for selected structural process content, `processes-ux.md`) *(rev 3)*. Each has an owner and, for non-tasks, the exact proposed change (a diff). A Process item lands through the merge flow once the owner approves the retro's items. *(rev 4)* That approval is also the process's level approval: the owner can approve a process change at every level (`processes-ux.md` decision 8), so no second approval is needed. Selected organization/team/project processes live together in organization Git at `processes/<id>.md`, recursively nested team `processes/<id>.md`, and `projects/<project-id>/processes/<id>.md`. Review changes there against exact revisions. Private process storage/privacy and approvals remain unresolved deferred proposals; do not use shared Git to promise private visibility.
7. **The owner approves, and then it's applied everywhere.** **Approve** creates the tasks and proposes every procedure change through the normal branch and merge flow (docs land through branches): *(rev 4)* organization-Git review branches for selected scoped process/report documents; code-rule changes may additionally require a branch in each affected code repository (decision 6). The retro shows "Applied to all agents" once every branch it produced is merged. Once merged, every agent gets it at its next start or wake: the guidance moment, the checklist or the rule. The owner can turn on **"Apply procedure changes without asking"** for low-risk kinds (guidance and checklists). That skips only the owner's approval: the changes still land through review, go and merge (`decisions-and-tech-debt-ux.md` A36).
8. **The next retro checks the last one's actions:** "Last time: run tests on a single branch. Did it help?" with the before and after numbers when they exist (median test time 14 min → 6 min). Items that didn't help can be reverted from there.
9. **A retro is a persistent record:** `projects/<project-id>/reports/<document-id>.md` with the selected report envelope; period, features, participants, evidence and proposed action IDs stay in body content pending a specialized workflow metadata contract. *(rev 5)* The file carries no cost amounts or budget figures (A109): its evidence gives tokens and "Budget reached" where it applies, with a link to the retro in the app. The evidence that agents and the facilitator get, in the app, by `agent-os retro show` and with the input request, has no cost amounts either (tokens and "Budget reached" only), so their answers and the synthesis can't carry amounts into the file, as `project-status-ux.md` A41 does for the Reporter. The action-item tasks are durable typed tasks linked to it.

## 1. Flow 1 · Settings (project › Settings › Retrospectives)

```
 Retrospectives
 Run a retrospective  ☑ after each delivered feature
                      ☑ every [4 hours ▾]  (only work finished in that time)
                      whichever comes first
 Facilitator          [ada (project lead) ▾]
 Who takes part       (•) Everyone with finished work in the period  ( ) Choose…
 Approval             (•) I approve action items   ( ) Apply guidance and checklist changes without asking
 Last retro 13:10 · next: when "Costs per project" is delivered, or at 17:10
                                                           [Run a retro now]
```

- Presets for the interval: every hour · 2 h · 4 h · daily · weekly · custom. Turning both triggers off shows "Retros only run when you start one".
- **Run a retro now** covers everything finished since the last one.

## 2. Flow 2 · A retro runs

1. **Gathering** (automatic, about a minute): the scope and its evidence (decision 4).
2. **Input** (asynchronous, up to 30 min by default): each participant gets "Retro for 13:10–17:10: 3 features. Your input: keep doing, do better, stop doing, problems." Agents answer with `agent-os retro input` (or in the app), and each item can cite evidence.
3. **Synthesis** (the facilitator): groups duplicates, counts how many raised each one, writes the root cause for each problem, and proposes action items with kinds, owners and diffs.
4. **Review** (the owner): one Needs you item, "Retro ready: 4 action items to approve".
5. **Applied:** tasks created, procedure changes proposed through branches, and the retro doc committed.

The retro's page shows its stage at the top: Gathering · Waiting for input (3 of 5) · Synthesizing · Ready for you · Applied.

## 3. Flow 3 · The retro page (owner's review)

This is the authorized owner/project-manager app view. Agents, including the facilitator, receive tokens and safe budget state only; no amount-bearing evidence enters their synthesis or shared files. Cost includes explicit completeness/freshness labels from the ledger.

```
 Retro · Agentic Org · 8 Nov 13:10–17:10 · 3 features · 5 agents · ≈ $2.40      [Approve 4 items]
 ┌ Evidence ────────────────────────────────────────────────────────────────────────────────┐
 │ F-3 Org chart views  cycle 3 h 10 m · 2 review rounds · 0 bugs                       │
 │ F-4 Offices (HQ)     cycle 5 h 40 m · 5 review rounds ⚠ · waiting on you 1 h 50 m ⚠  │
 │ F-5 Create org       cycle 2 h 05 m · 1 review round                                 │
 │ Tests: median 14 min per run ⚠ (38 runs)   Debt taken: 2   Reopened bugs: 0   Cost ≈ $61   │
 └───────────────────────────────────────────────────────────────────────────────────────────┘
 Keep doing                     Do better                         Stop doing
 • Independent review before    • Check the accepted designs       • Running the full test suite
   the reviewer (4 agents)        before writing a spec (Eden,       for one-crate changes
                                  ada) → F-4: 5 rounds           (3 agents)
 Problems
 • Specs contradicted the git design twice. Why: the designs weren't in the spec checklist.
   Prevent: add "check docs/design" to the spec checklist.
 ───────────────────────────────────────────────────────────────────────────────────────────
 Action items                                           kind        owner      change
 ☑ Run only the changed crate's tests before review     Guidance    ada        [diff]
 ☑ Add "checked docs/design/*" to the Spec checklist     Checklist   Eden       [diff]
 ☑ Ask the owner's questions in one batch per hour       Guidance    ada        [diff]
 ☑ Cache cargo builds per worktree                       Task        server-dev [T-88]
 Last retro's actions: "Docs land through branches" helped: review rounds 5 → 2 ✓
```

- **Each answer** shows who raised it (avatars, with a count), and its evidence as links.
- **Action items:** tick to include. **Edit** changes the text, kind, owner or diff. **Add item…** adds one, and **Discard** removes one with a reason (kept in the record).
- **[diff]** opens the exact change: the guidance moment's text, the checklist template, the AGENTS.md lines or the process file, before and after. *(rev 4)*
- **Approve n items:** tasks appear in the project (To do, "from Retro 8 Nov"). *(rev 4)* Selected process/report changes become review branches in organization Git; code-repository instruction changes remain separate explicitly targeted branches. Private process changes await a storage/access decision. The owner's approval here covers every level, so none of them waits for a second approval (decision 6). Each item shows "Waiting to merge" until its branch is merged. When every branch the retro produced is merged, the retro shows "Applied to all agents" with the date.
- **States:** an agent didn't answer in time (the retro continues without it, noted "4 of 5 answered") · nothing to approve ("No action items. Keep doing what you're doing.") · a procedure change conflicts with one waiting to merge (shown on the item, with both diffs) · a separately authorized task/agent budget reaches its limit (show stop pending until current-turn completion, then held; no new retro budget scope is selected).

## 4. Where retros show

- Project **Docs › Retrospectives** (a proposed presentation group over project report documents) lists each retro with its period, features, action-item counts and status.
- **Activity:** "Retro for 13:10–17:10 ready: 4 action items" and "Retro actions applied to all agents".
- **Insights** (costs spec): retro cost appears as kind "Retro". A trend chart of the evidence numbers the retros cite (review rounds, test time, waiting on you) shows whether retros help.
- **Agents:** at their next start or wake, a one-line notice: "New from the 8 Nov retro: run only the changed crate's tests before review." This is the guidance moment **Procedure changed**.

## 5. CLI (agents)

These are retained `agent-os` command proposals, not implemented `agent-os` commands or a new wire API.

`agent-os retro show [<id>]` · `retro input <id> --keep "…" --better "…" --stop "…" --problem "…" --why "…" --prevent "…" [--evidence T-17]` · `retro synthesize <id>` (facilitator) · `retro list`. The words match the app's.

## 6. Earlier deferred acceptance criteria (RETRO)

- RETRO-1 · **Deferred retrospective engine:** Project settings let the owner run retros after each delivered feature, every chosen interval, or both (whichever comes first), and show the next expected retro.
- RETRO-2 · **Deferred retrospective engine:** A retro includes only features (and their tasks and bugs) finished since the last retro. In-flight work is excluded, and a period with nothing finished is skipped and noted.
- RETRO-3 · **Deferred retrospective engine:** *(rev 5)* Each retro starts with an evidence panel: features with cycle time, review rounds, waiting on the owner and blocked time, plus tests, debt, reopened bugs, failed runs and cost when available. Cost amounts show only to the owner and the project's manager in the app (document 17 and COST-7); others, every agent (the facilitator included) and the CLI see tokens.
- RETRO-4 · **Deferred retrospective engine:** Every agent with finished work in the scope is asked for Keep doing, Do better, Stop doing and Problems (with why and prevention), and on-call agents are woken for it.
- RETRO-5 · **Deferred retrospective engine:** The facilitator's synthesis groups duplicates with a count and links each item to its evidence.
- RETRO-6 · **Deferred retrospective engine:** Each action item has a kind (Task, Guidance, Checklist, Template, Decision, Rule, Process), an owner, and for non-tasks the exact change as a diff. A Process item proposes a diff to a process file and lands through the merge flow (organization Git for selected process scope; private processes await a storage/privacy decision), with the owner's retro approval as the level approval (RETRO-7).
- RETRO-7 · **Deferred retrospective engine:** Approve creates the tasks and proposes the procedure changes through the merge flow: selected process/report changes on organization-Git review branches, with explicitly separate code-repository branches only for code rules, with the owner's retro approval as the level approval (no second approval). Each item shows "Waiting to merge" until its branch is merged, and the retro shows "Applied to all agents" once every branch it produced is merged.
- RETRO-8 · **Deferred retrospective engine:** After the merge, each agent sees the change at its next start or wake.
- RETRO-9 · **Deferred retrospective engine:** The next retro shows the previous action items with "Did it help?" and before/after numbers when they exist, and offers to revert one.
- RETRO-10 · **Deferred retrospective engine:** *(rev 5)* Each retro is saved as `projects/<project-id>/reports/<document-id>.md` with front matter, and listed in Docs › Retrospectives. The file has no cost amounts or budget figures: tokens and "Budget reached" instead, with a link to the retro in the app (A109). Agents' input and the synthesis are written from evidence without amounts (decision 9).
- RETRO-11 · **Deferred retrospective engine:** The owner can turn on applying guidance and checklist changes without their own approval (they still land through review, go and merge). Other kinds always need approval.
- RETRO-12 · **Deferred retrospective engine:** `agent-os retro input|show|synthesize|list` work like the app, with the same words.

## 7. Open questions

**For ada**
- A31 · Superseded lifecycle vocabulary: an explicit delivered_at fact is separate from selected Live status. The proposed retro trigger remains deferred and needs a transition/deduplication policy.
- ~~A32~~ · *Resolved (ada):* a retro write-up maps to selected report content; the proposed retrospective run and action lifecycle remain deferred (the facilitator's run, recorded like any run). Its action items become tasks or doc changes through the merge flow. It isn't a separate work-item kind, so the stages in §2 are the run's progress. Plan row W17.
- A33 · Who's the facilitator by default: the project's lead, or a scheduled "retro" role (a one-off run, `agent-run-modes-ux.md`)?

**For the user**
- ~~U22~~ · *Resolved (the user, 2026-11-08): as recommended.* Defaults: after each feature **or every 4 h**, whichever comes first, 30 min for input, ada facilitating, and you approve all action items. OK?
