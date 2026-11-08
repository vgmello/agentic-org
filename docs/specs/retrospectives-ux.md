---
kind: spec
title: "Retrospectives: learn from each delivery, and apply it to every agent (UX)"
status: for-review
revision: 2
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-08"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [features-ux, decisions-and-tech-debt-ux, work-items-ux, costs-and-insights-ux, agent-run-modes-ux]
---

# Spec · Retrospectives: learn from each delivery, and apply it to every agent (UX)

Extends `features-ux.md` (delivered features), `feature-requests-ux.md`, `decisions-and-tech-debt-ux.md` (docs, agent guidance §6.2, templates §3.2), `work-items-ux.md` (checklists §9.1, time §9.2), `costs-and-insights-ux.md` and `agent-run-modes-ux.md`.

## The request (the user, 2026-11-08)

> From time to time, after a larger delivery, the agents run a **retrospective**: what could we have done better, what did we do well that we should keep doing, what should we **stop** doing because it doesn't work, and why did problems happen, so they don't happen next time. Agents work fast, so retros are more frequent: at the end of each feature, or every X hours (every hour, three or four hours), **configurable**, **whichever comes first**. On a time interval, only the features **finished** in that period are included, never the ones in flight. After the retro, the **action items** become **tasks** and updated **procedures**, so next time every agent applies them. For example, if running tests always takes too long, the process for running tests (say, on a single branch) is improved and applied to all agents.

**Revision 2 (2026-11-08): aligned with ada's W7 model.** Task numbers follow `work-items-ux.md` rev 6: `T-17` inside the project (`ac-T-17` elsewhere). No criteria changed.

## 0. Decisions

1. **A retro runs on whichever comes first:** a **feature is delivered** (a feature reaches Delivered, `features-ux.md`), or **the interval** passes (default every 4 h, from 1 h to 1 week). Both are configurable per project, and either can be turned off. Running a retro resets the clock.
2. **A retro covers finished work only.** Its scope is the features (and their tasks and bugs) that finished since the last retro. Work still in flight waits for the next one. A period with nothing finished is **skipped** and noted ("Nothing finished since 14:00. No retro"), so it costs nothing.
3. **Four questions, in plain words:** **Keep doing** (what worked) · **Do better** (what could improve) · **Stop doing** (what doesn't work) · **Problems** (what went wrong, **why**, and how to prevent it next time).
4. **Evidence first, opinions second.** Before anyone answers, the tool gathers the period's facts: the features and their cycle times, waiting on the owner, blocked time, review rounds, reopened bugs, failed runs, tests and their time (when reported), debt taken, exceptions, and cost. Every answer can cite them ("T-17 needed 5 review rounds").
5. **The agents who did the work take part.** Everyone with a finished task in the scope is asked for input (woken if on call), plus the **facilitator**, by default the project's lead, who merges the answers, groups duplicates, and proposes action items. The owner can add input any time and is never required to.
6. **Action items are changes, not wishes.** Each has a **kind**: **Task** (work to do) · **Guidance** (a change to agent guidance, §6.2 of the decisions spec) · **Checklist** (a change to a checklist template) · **Template** (a doc template) · **Decision** (a new ADR) · **Rule** (AGENTS.md or the project brief). Each has an owner and, for non-tasks, the exact proposed change (a diff).
7. **The owner approves, and then it's applied everywhere.** **Approve** creates the tasks and proposes every procedure change through the normal branch and merge flow (docs land through branches). Once merged, every agent gets it at its next start or wake: the guidance moment, the checklist or the rule. The owner can turn on **"Apply procedure changes without asking"** for low-risk kinds (guidance and checklists). That skips only the owner's approval: the changes still land through review, go and merge (`decisions-and-tech-debt-ux.md` A36).
8. **The next retro checks the last one's actions:** "Last time: run tests on a single branch. Did it help?" with the before and after numbers when they exist (median test time 14 min → 6 min). Items that didn't help can be reverted from there.
9. **A retro is a persistent record:** `docs/retros/<date>-<slug>.md` with front matter (period, features, participants, action-item ids, status). The action-item tasks are database work items linked to it.

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
2. **Input** (asynchronous, up to 30 min by default): each participant gets "Retro for 13:10–17:10: 3 features. Your input: keep doing, do better, stop doing, problems." Agents answer with `org-agent retro input` (or in the app), and each item can cite evidence.
3. **Synthesis** (the facilitator): groups duplicates, counts how many raised each one, writes the root cause for each problem, and proposes action items with kinds, owners and diffs.
4. **Review** (the owner): one Needs you item, "Retro ready: 4 action items to approve".
5. **Applied:** tasks created, procedure changes proposed through branches, and the retro doc committed.

The retro's page shows its stage at the top: Gathering · Waiting for input (3 of 5) · Synthesizing · Ready for you · Applied.

## 3. Flow 3 · The retro page (owner's review)

```
 Retro · Agentic Org · 8 Nov 13:10–17:10 · 3 features · 5 agents · ≈ $2.40      [Approve 4 items]
 ┌ Evidence ────────────────────────────────────────────────────────────────────────────────┐
 │ FEAT-0003 Org chart views  cycle 3 h 10 m · 2 review rounds · 0 bugs                       │
 │ FEAT-0004 Offices (HQ)     cycle 5 h 40 m · 5 review rounds ⚠ · waiting on you 1 h 50 m ⚠  │
 │ FEAT-0005 Create org       cycle 2 h 05 m · 1 review round                                 │
 │ Tests: median 14 min per run ⚠ (38 runs)   Debt taken: 2   Reopened bugs: 0   Cost ≈ $61   │
 └───────────────────────────────────────────────────────────────────────────────────────────┘
 Keep doing                     Do better                         Stop doing
 • Independent review before    • Check the accepted designs       • Running the full test suite
   the reviewer (4 agents)        before writing a spec (Eden,       for one-crate changes
                                  ada) → FEAT-0004: 5 rounds           (3 agents)
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
- **[diff]** opens the exact change: the guidance moment's text, the checklist template or the AGENTS.md lines, before and after.
- **Approve n items:** tasks appear in the project (To do, "from Retro 8 Nov"). Procedure changes become one docs branch, shown as "Waiting to merge". When it's merged, the retro shows "Applied to all agents" with the date.
- **States:** an agent didn't answer in time (the retro continues without it, noted "4 of 5 answered") · nothing to approve ("No action items. Keep doing what you're doing.") · a procedure change conflicts with one waiting to merge (shown on the item, with both diffs) · retro cost limit reached (the facilitator stops and synthesizes what it has).

## 4. Where retros show

- Project **Docs › Retrospectives** (a doc kind, `docs/retros/`) lists each retro with its period, features, action-item counts and status.
- **Activity:** "Retro for 13:10–17:10 ready: 4 action items" and "Retro actions applied to all agents".
- **Insights** (costs spec): retro cost appears as kind "Retro". A trend chart of the evidence numbers the retros cite (review rounds, test time, waiting on you) shows whether retros help.
- **Agents:** at their next start or wake, a one-line notice: "New from the 8 Nov retro: run only the changed crate's tests before review." This is the guidance moment **Procedure changed**.

## 5. CLI (agents)

`org-agent retro show [<id>]` · `retro input <id> --keep "…" --better "…" --stop "…" --problem "…" --why "…" --prevent "…" [--evidence T-17]` · `retro synthesize <id>` (facilitator) · `retro list`. The words match the app's.

## 6. UX acceptance criteria (RETRO)

- RETRO-1 · Project settings let the owner run retros after each delivered feature, every chosen interval, or both (whichever comes first), and show the next expected retro.
- RETRO-2 · A retro includes only features (and their tasks and bugs) finished since the last retro. In-flight work is excluded, and a period with nothing finished is skipped and noted.
- RETRO-3 · Each retro starts with an evidence panel: features with cycle time, review rounds, waiting on the owner and blocked time, plus tests, debt, reopened bugs, failed runs and cost when available.
- RETRO-4 · Every agent with finished work in the scope is asked for Keep doing, Do better, Stop doing and Problems (with why and prevention), and on-call agents are woken for it.
- RETRO-5 · The facilitator's synthesis groups duplicates with a count and links each item to its evidence.
- RETRO-6 · Each action item has a kind (Task, Guidance, Checklist, Template, Decision, Rule), an owner, and for non-tasks the exact change as a diff.
- RETRO-7 · Approve creates the tasks and proposes all procedure changes on one docs branch through the merge flow. The retro shows "Applied to all agents" once merged.
- RETRO-8 · After the merge, each agent sees the change at its next start or wake.
- RETRO-9 · The next retro shows the previous action items with "Did it help?" and before/after numbers when they exist, and offers to revert one.
- RETRO-10 · Each retro is saved as `docs/retros/<date>-<slug>.md` with front matter, and listed in Docs › Retrospectives.
- RETRO-11 · The owner can turn on applying guidance and checklist changes without their own approval (they still land through review, go and merge). Other kinds always need approval.
- RETRO-12 · `org-agent retro input|show|synthesize|list` work like the app, with the same words.

## 7. Open questions

**For ada**
- ~~A31~~ · *Resolved (ada):* delivered = a feature reaching Delivered.
- ~~A32~~ · *Resolved (ada):* a retro is a doc kind (`docs/retros/`, front matter) produced by a **run** (the facilitator's run, recorded like any run). Its action items become tasks or doc changes through the merge flow. It isn't a separate work-item kind, so the stages in §2 are the run's progress. Plan row W17.
- A33 · Who's the facilitator by default: the project's lead, or a scheduled "retro" role (a one-off run, `agent-run-modes-ux.md`)?

**For the user**
- ~~U22~~ · *Resolved (the user, 2026-11-08): as recommended.* Defaults: after each feature **or every 4 h**, whichever comes first, 30 min for input, ada facilitating, and you approve all action items. OK?
