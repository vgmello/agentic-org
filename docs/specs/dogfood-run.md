---
kind: spec
title: "Dogfood run: the walkthrough and what counts as passing"
status: for-review
revision: 1
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-08"
requested_by: ac-staff-engineer-ada
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [our-org-in-the-app, offices-and-teams-ux, work-items-ux, agent-run-modes-ux]
---

# Spec · Dogfood run: the walkthrough and what counts as passing

The script for `our-org-in-the-app.md`'s success test, for ada to turn into the end-to-end checklist. Builds on `our-org-in-the-app.md` (the flows and criteria), `offices-and-teams-ux.md` (offices, status), `work-items-ux.md` (tasks) and `agent-run-modes-ux.md` (pause, how it runs).

## Changelog

**Revision 1 (2026-11-08):** first version, from ada's list of what's left (8 Nov).

## The request (ada, 2026-11-08)

> A walkthrough script for the run: the steps, what to check at each one, and what counts as pass/fail, mapped to the success test, as a short doc I can turn into the e2e checklist.

The success test (`our-org-in-the-app.md`): in the real app on this Mac, with no mock data, (1) the owner brings our existing team in, once, in under 5 minutes, with every agent in the org chart under the right team and manager and with its runtime; (2) Home and Activity show live what the team is doing; (3) the owner hires, assigns, moves, pauses and offboards, and every change shows at once while every agent keeps working on its own runtime.

## 0. Before the run

**Ready when** (ada's order, 8 Nov): the host gives the app its token and live office status (River, host contract); the office server's live events and agent commands are on `v1` (server-dev); the org screens use the real HQ, not the mock (app-frontend-dev); the import exists (codex-dev, W12); presence comes from `org-agent who` and status; and, for the steps that need agents' messages or presence in the office (2.2, 2.3, 2.6, 2.7, the message part of 3.6, and ada getting the proposal in her inbox in 3.3), Remy's messaging Phase B, which brings agents' messages and presence into the office (ADR-0003). Without Phase B, steps 1 and 3 can run, and 2.2, 2.3, 2.6, 2.7, the message part of 3.6 and the inbox part of 3.3 are N/A until it lands.

**Setup:**
- A clean build of `v1` on this Mac, with no sample data. The run fails at once if anything shows "Sample data".
- A **new** organization, "Agentic Consulting" (U10), created in the app. Never the development organization.
- The HR workspace (`~/projects/agentic-consulting/agents`) and the messaging spool as they are today. The **import** reads them and must not change them. Agents keep working, so their own changes (their home folders, the messages they send) don't count. Before step 1, note what only the import could touch: in the agents repo, the HR records (`projects/*/agents/*/agent.json`) and the role files (`hr/roles/`), with the current commit as the snapshot point; and the registered messaging names and inboxes in the messaging store in use (the spool before Phase B's cutover, the office after it). The agents commit to that repo all the time, so the repo's last commit isn't a check. Compare at 1.7. Step 3 changes them on purpose: each throwaway hire registers a messaging name, gets a home folder (in a scratch folder, outside the agents repo) and maybe a new role file. Those are expected.
- Agents keep running as usual during the run. Nobody is stopped for it.
- A stopwatch for step 1, and a note of the time at each step.

**Record for each step:** pass or fail, the time, and for a fail a screenshot and what was seen. One fail doesn't stop the run unless the step says so.

## 1. Bring our team in (success test 1, IMP)

| # | Do | Check | Pass when |
| --- | --- | --- | --- |
| 1.1 | Start the stopwatch. After **Create organization**, choose **Bring in an existing team**. | Both sources are prefilled (`our-org-in-the-app.md` §4) and **Scan** is offered (IMP-1). | Both folders found without typing a path. |
| 1.2 | **Scan**. | One row per agent, merged by name, with its sources: ada, Eden, River and Remy "HR + messaging"; server-dev, app-frontend-dev, agy-senior-dev, codex-dev and reviewer "messaging only"; Riley "HR only" with "No messages from Riley yet" (IMP-2, IMP-4). The HR toolkit's own agent isn't a row (§2a), and neither is the owner's own inbox (`vgmello`): the owner is already in the organization. | All 10 listed once, sources right, nothing written yet (the import-only snapshot unchanged). |
| 1.3 | Fill role and runtime for the messaging-only rows (server-dev Codex, codex-dev Codex, agy-senior-dev Antigravity; app-frontend-dev and reviewer as the user says, A14). | **Bring in** counts only complete rows (IMP-3). | The count rises as rows are completed. |
| 1.4 | Check the suggested teams, leads, reporting lines and project against `our-org-in-the-app.md` §2. Edit where needed. | Engineering (ada) with App, Office, Agent tooling, Quality; Design (Eden); ada → owner, Eden → owner, Engineering → ada; project Agentic Org (`ac`), shared (IMP-5). "What happens" names the counts and says nothing changes in HR or messaging (IMP-6). | Matches §2 before Bring in. Edits are allowed (the messaging-only agents' managers default to the owner, so at least five need ada), and their time counts toward the 5 minutes. |
| 1.5 | **Bring in**. Stop the stopwatch at **Done**. | Progress in words, then the org chart with the new members highlighted and the summary (IMP-7). | **Under 5 minutes** from 1.1. The summary says "10 agents, 6 teams, 1 project." with no "agents paused" line (Riley comes in Active and shows Offline, U13). Every agent in its team, under its manager. |
| 1.6 | Open three quick views: ada, codex-dev and Riley. | Runtime, messaging name, office and status show; codex-dev says "Runs on Codex"; Riley is Offline, not Paused (PROF-1, U13). | All three right, and no field blank: a missing value reads plainly (§5). "Runtime not set" is a fail here. Riley has no messaging inbox, so its messaging name may read as not set, which passes. |
| 1.7 | Compare the import-only snapshot with now: `git log <snapshot>.. -- 'projects/*/agents/*/agent.json' hr/roles` and a diff of those files in the agents repo, and the registered names and inboxes in the messaging store in use. | The import changed no HR record or role file and registered no name or inbox. | No difference in those paths. Changes from the agents' own work elsewhere don't count. |
| 1.8 | Run import again. | "Nothing new" (IMP-8). | No duplicates. |

**Stop the run** if 1.5 fails (the other steps need the team in).

## 2. See what's happening, live (success test 2, LIVE)

| # | Do | Check | Pass when |
| --- | --- | --- | --- |
| 2.1 | Open Home. | Needs you, **Working now** (each working agent with its task) and the latest 8 Activity rows (LIVE-4). | Real agents and tasks, no sample rows. |
| 2.2 | Ask one agent (say ada) to send another (say Eden) a short message with `org-agent msg send`. | The message appears in Activity, and **Open thread** shows it (LIVE-2). | Within **2 seconds**, without a reload. |
| 2.3 | Have an agent start a turn (send it a message or start a task), then let it finish. | The card's status word changes with presence (Working, then back), LIVE-6 and PROF-3. | Within 2 seconds of the turn starting and ending. |
| 2.4 | Move a task (an agent runs `org-agent task …`, or the owner on the board). | A Tasks row in Activity, and the board updates. | Within 2 seconds. |
| 2.5 | Merge a reviewed branch into `v1`: the next real merge, or a trivial reviewed one made for the run if none comes within the run. | A Git row in Activity ("merged … into v1"). | Shows once the office has fetched. If no merge is possible during the run, mark N/A and check it at the next merge; the run can still pass. |
| 2.6 | Scroll Activity down, then send another message. | The list doesn't move, and "1 new · Jump to latest" appears (LIVE-5). | As described. |
| 2.7 | Open a thread the owner isn't in. Then have an agent message the owner, and open that thread. | Read-only, and it says so (LIVE-3). Reply works in a thread addressed to the owner. | As described. |

## 3. Manage the team (success test 3, STAFF, MOVE, DEAL, APPR)

Use throwaway hires for the destructive steps, with names that say so ("Dogfood QA 1", "Dogfood QA 2"), and choose a home folder **outside** the agents repo for them in Review (for example a scratch folder). Never offboard a real teammate in the run.

| # | Do | Check | Pass when |
| --- | --- | --- | --- |
| 3.1 | **Hire**: "a QA engineer for the app", team App; name it Dogfood QA 1 and set its home folder to the scratch folder. | The HR lead wakes and proposes; Review shows team, reports to (App's lead), runtime, model, office, home folder and messaging name before anything is created (STAFF-1, HRL-3). | Nothing is created before approval. |
| 3.2 | Approve as the owner. | The hire appears in App, highlighted, and in Activity (STAFF-2, APPR-5); its profile shows it works on Agentic Org through App (decision 6: work comes through a team). | At once, with "requested by … approved by you". |
| 3.3 | First, as the owner, turn on **Can approve hires** for ada (her Settings; the import doesn't carry the flag, APPR-1). Then have an agent who reports to ada (codex-dev) request a second hire (`org-agent hire request "A QA engineer named Dogfood QA 2 for Office, for the dogfood run; home folder in a scratch folder outside the agents repo"`), and ada approve it (`org-agent hire approve <id> --reason "…"`), with the id from the proposal she gets in her inbox (before Phase B: the runner gives her the id from Agents › Requests, and the inbox check is N/A). | The proposal shows "Waiting for ada"; no human step after her approval; Home shows "ada approved hiring …" (APPR-1, APPR-2, APPR-4, APPR-5). | As described. |
| 3.4 | From the board, open a task and assign it to Dogfood QA 1. | The assignee picker shows status and runtime (MOVE-4); the task shows on the agent. | At once, and one Activity row. |
| 3.5 | **Move to team…**: move Dogfood QA 1 from App to Office. | One action; never in "Not in a team"; the dialog says "Moves Dogfood QA 1 from App to Office. Dogfood QA 1 stops working on App's projects (Desktop app) and starts on Office's (Office server)." (both teams share Agentic Org, so it names the areas, §8); the toast offers Undo (MOVE-1, MOVE-2). | Card moves at once; Activity has one row. |
| 3.6 | **Pause** Dogfood QA 1. While it's paused, have an agent send it a message (`org-agent msg send --to <its messaging name> "…"`; N/A before Phase B). Then **Resume**. | Paused on the card and not woken; the message waits until Resume (DEAL-3); Resume in one click. | Each change on the card at once, and one Activity row each. |
| 3.7 | **Offboard** both throwaway hires (Dogfood QA 1 and 2), then try **Add person** with Dogfood QA 1's messaging name. | Only the consequences that apply; Dogfood QA 1's task comes back unassigned; both show under Former members (DEAL-4); the name is refused as taken. | At once, one Activity row each, and the name stays reserved. |
| 3.8 | Throughout 3.1–3.7, watch the other agents. | They keep working on their own runtimes; nothing stops or restarts them. | No agent interrupted. |
| 3.9 | Words check across the run. | Never "deallocate"; status, runtime, Paused and Lead are words plus icons (DEAL-1, §10). | No exceptions seen. |

## 4. Result

- **Pass:** 1.5 under 5 minutes (strict: 5:00 or more is a fail), and every step in 1–3 passes (2.5 may be N/A, and 2.2, 2.3, 2.6, 2.7, the message part of 3.6 and the inbox part of 3.3 before Phase B; the success test is met only once they pass too).
- **Pass with notes:** every step passes, except that some 2-second live updates in step 2 took up to 4 s. This allowance never applies to 1.5. Each note becomes a bug in Agentic Org (`ac`), Major.
- **Fail:** any other failed step. Each becomes a bug, Critical if it blocks the success test, Major otherwise, with the step number in the title.
- The run's notes (times, screenshots, the snapshot diff) go in `docs/research/dogfood-run-<date>.md`, so the next run can compare.
