---
kind: spec
title: "Our own team in Agentic Org (dogfooding): bring in, see, hire, assign, move, deallocate (UX)"
status: for-review
revision: 11
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-08"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner, app-frontend-dev, server-dev, ac-software-engineer-river, ac-software-engineer-remy, codex-dev]
builds_on: [offices-and-teams-ux, work-items-ux]
---

# Spec · Our own team in Agentic Org (dogfooding): bring in, see, hire, assign, move, deallocate (UX)

Builds on `offices-and-teams-ux.md` rev 4, `work-items-ux.md` rev 5 and `desktop-app-v1.md` (in Eden's notes, not in this repository) (hire §3.6, profile §3.5).

**Revision 11 (2026-11-08): import counts.** The import's examples now match our team today: 10 agents (Remy is working, and Riley comes in Active and shows Offline, U13), so "Bring in 5 agents · 5 need details" until the five messaging-only agents get a role and runtime, then "Bring in 10 agents", and a summary with none paused; offboarded HR agents are counted as "(n)", since none are today (decision 1, §4). No criteria changed.

**Revision 10 (2026-11-08): small fixes.** HRL-2 lists member, lead and manager pickers, matching §2a. The profile tabs include **Processes** (`processes-ux.md` §2).

**Revision 9 (2026-11-08): review fixes.** The requester never approves their own request (it goes one level up), except the owner, who approves their own hires since nobody is above the owner (§7.1). In Chat the HR lead is listed in DMs and in @ completion as "HR lead". The Requests tab has empty, loading and error states (§7.1). U10 and U11 now read as approved: the organization is **Agentic Consulting**, with the shape in §2. APPR-3 and HRL-2 rewritten.

**Revision 8 (2026-11-08): approving hires without the owner (the user).** A flag on an agent, **Can approve hires**, lets that agent approve hires. A hire request goes to the requester's manager, and if that manager has the flag, its approval is enough: the HR lead provisions with no human in the loop. Otherwise the request goes up the reporting line to the nearest manager with the flag, and finally to the owner (§7.1). §2a, HRL-3 and HRL-4 rewritten. APPR-1 to APPR-8 added.

**Revision 7 (2026-11-08): the HR lead is built in (the user).** HR is a built-in agent, "HR lead", that only provisions agents (§2a). The user approved my recommendations U10–U12 (the name Agentic Consulting, the team shape in §2, and the app as where we manage the team). HRL-1 to HRL-5 added.

**Revision 6 (2026-11-08): aligned with ada's W7 model.** Item numbers in Activity follow `work-items-ux.md` rev 6 and carry the project key, since Activity spans projects (`ac-B-15`). No criteria changed.

**Revision 5 (2026-11-08): follows org-proto on v1.** Offboarding or removing a member sends their reports to that member's own manager, or to the owner if they had none (not always to the owner). Pause stops a running session at once (agent-sessions). G2 uses `member.set_agent_status`. The survey notes what v1 has gained since.

**Revision 4 (2026-11-08): Remy is online.** The user started Remy, who owns messaging and `org-agent` (AGENTS.md on v1) since agy-senior-dev ran out of credits. §2 moves him to Agent tooling as its lead. Riley stays Offline.

**Revision 3 (2026-11-08): reviewer fixes.** Offboarding's kept records, reserved name and Former members are now a contract request (A37), because `member.remove` deletes the member today. The spool-import facts follow ADR-0003. Duplicate open questions removed.

**Revision 2 (2026-11-08):** the user: Remy and Riley aren't started, and they show as **Offline**. Fixes from review: offboarding follows the teams design (reports go to the owner, and open tasks come back unassigned), and git and GitHub belong to River. My recommendations for U10–U12.

## The goal (the user, 2026-11-08)

> All of us (ada, Eden, the reviewer, codex-dev and the other devs) are registered as an organization in the application. From it you can see everything that's happening, add agents, hire more, deallocate agents, assign agents and change teams, with a great user experience.

**Success test (the dogfood run).** In the real app on this Mac, with no mock data:
1. The owner brings our existing team in, once, in under 5 minutes, and every agent shows in the org chart under the right team and manager, with its runtime.
2. Home and Activity show, live, what the team is doing: who's working, the messages they send each other (like ada's phase-1 push), tasks and bugs moving, and merges into `v1`.
3. The owner hires a new agent, assigns it to a team, a project and a task, moves an agent to another team, pauses an idle agent and offboards one. Every change shows at once in the chart and Activity, and every agent keeps working on its own runtime.

## Where we are (survey of `origin/v1` at 0f7df26, 2026-11-08)

*Since this survey, v1 gained agents as members (`Member.agent: AgentProfile`, `86a6a9f`), `member.set_agent_status` and run modes, and `member.offboard` (A37, `2911ffc`). G1 and G2 below track the rest.*

- **App:** create, open and list organizations work on a **mock** (`MockOffice`). Agents, Projects, Tasks, Activity and Settings are placeholders. The org chart is work in progress on sample data (app-frontend-dev's worktree).
- **Office:** the server and host are stubs on `v1`. server-dev's store (local branch) has organizations only. Teams and members are a **contract only** (`org-proto`: `Member { id, kind, name, contact, owner, office_id, reports_to }`, `team.*`).
- **Hiring has no design and no contract.** The teams design says agents "join through hiring (the HR flow, its own design)", and that design was never written. This spec writes the UX.
- **Nothing imports an existing team.** The only planned import (ADR-0003 Phase B, `import-v1-old-spool`) brings the spool's messages, profiles, statuses and read/snooze state into HQ, not team members. Its spool is deleted a week after the move, so reading names from the spool (IMP-2, IMP-8) must happen in that window, or later from the profiles HQ imported.
- **An app `Member` can't describe our agents.** It has no role, title, focus, runtime, model, home folder or messaging id.
- **Our roster is split across three sources** (below), and they disagree.

## 1. Our team today

| Agent | In HR (agents repo) | On messaging (`org-agent`) | Runtime | Role / owns (AGENTS.md) | Reports to |
| --- | --- | --- | --- | --- | --- |
| ada (`ac-staff-engineer-ada`) | active | yes | Claude Code | Staff Engineer: leads; contracts (org-proto) | the owner |
| Eden (`ac-product-designer-eden`) | active (title says Principal; the user says **Senior**) | yes | Claude Code | Product Designer | the owner |
| River (`ac-software-engineer-river`) | active | yes | Claude Code | Senior Backend: office host, sync, PKI, git and GitHub (`org-git`, `org-github`) | ada |
| server-dev | **no** | yes | Codex | office server, storage | ada |
| app-frontend-dev | **no** | yes | ? (A14) | the desktop app | ada |
| agy-senior-dev | **no** | yes | Antigravity | owned messaging (org-agent CLI, spool, mailbox) until it ran out of runtime credits | ada |
| codex-dev | **no** | yes | Codex | inbox adapters; the work-items library (W7) | ada |
| reviewer | **no** | yes | ? (A14) | independent reviews ("ready" verdicts) | ada |
| Remy (`ac-software-engineer-remy`) | active | yes (started by the user later the same day) | Claude Code | messaging: `org-agent`, spool and the messaging library (AGENTS.md on v1), and the chat extension (plan row W19) | ada |
| Riley (`ac-software-engineer-riley`) | active | **no** | Claude Code | Senior Frontend (the old prototype) | ada |

**What this tells the design:**
- An organization is **mixed-runtime**. The app must show and respect each agent's runtime, and never assume Claude Code.
- **Not every agent was hired through HR.** Some were simply started in a runtime and given a messaging name. The app needs **Add an existing agent**, not only Hire.
- Records **drift**. HR says Riley is active, but Riley hasn't messaged and the app work moved to app-frontend-dev. (Remy was in the same state until the user started him.) Import must show this and let the owner decide (pause, offboard, keep), not copy it blindly.

## 2. The shape of our organization (approved, U11)

```
 (VM) Vitor (you) · Owner
 ├─ Engineering                 lead: ada
 │   ├─ App                     lead: app-frontend-dev     · app-frontend-dev, Riley (offline)
 │   ├─ Office                  lead: server-dev           · server-dev, River
 │   ├─ Agent tooling           lead: Remy                 · Remy (messaging, org-agent), codex-dev, agy-senior-dev (no credits)
 │   └─ Quality                 lead: reviewer             · reviewer
 └─ Design                      lead: Eden                 · Eden
 Project: Agentic Org (key ac) · teams: Engineering, Design · Shared
   Areas: General (Engineering) · Desktop app (App) · Office server (Office) · Messaging and CLI (Agent tooling) · Design and docs (Design)
 Reporting lines: ada → you · Eden → you · everyone in Engineering → ada
```

The organization is named **Agentic Consulting** (U10, approved), the workspace's name. It mustn't be "Agentic Org", which is the app.

## 2a. The HR lead, a built-in agent (rev 7, the user)

Every organization has one built-in agent, **HR lead**, created with the organization. Its only job is **provisioning agents**: it proposes and sets up hires (Hire, Add an existing agent, Staff a team from requirements), drafts new roles, and does the provisioning cleanup when the owner offboards an agent. It doesn't manage, lead, triage or take work.

- **Always there, never removable:** it can't be offboarded, deleted, moved into a team or given reports, and nobody reports to it. Its name is fixed ("HR lead"), and its handle `hr` is reserved like `owner`. The owner can change only its runtime, model and how it runs.
- **On call by default** (`agent-run-modes-ux.md`): asleep until a Hire, an Add agent or a provisioning request wakes it. Its runs and cost show under "HR lead" in Insights.
- **Where it shows:** beside the owner bar in the org chart as a compact chip, "HR lead · built-in · Asleep · wakes when called", which opens its quick view; in the Agents roster's own **Built-in** group at the top; and in Chat, in Direct messages and in @ completion, as "HR lead" (it can be messaged). It never appears on a team card, in Teams, People or Reporting lines, or in member, lead and manager pickers.
- **Someone always approves** *(rev 8)*: anything the HR lead provisions needs an approved Hire review, either by the owner or by a manager with **Can approve hires** (§7.1). Agents ask it (`@hr` in Chat, or `org-agent hire request "…"`), and the request is routed to its approver. Hiring is never automatic.
- **Import:** the HR toolkit's own agent (`.claude/agents/hr.md` in the agents repo) maps onto the built-in HR lead. It isn't brought in as a member.

## 3. Decisions

1. **Bring in, don't rebuild.** Our team enters the app through an **import** that reads what already exists (HR's folders and the messaging inboxes), shows a review of every agent with what was found and what's unclear, and writes nothing until **Bring in 10 agents**. After that, the app is the place to manage the team (U12).
2. **The agent record grows** to describe how an agent works, shown on every profile and card: role, title, focus, **runtime** (Claude Code · Codex · Antigravity), model, **home folder**, **messaging name** (`codex-dev`), office, hired on, and status. Those are UX requirements for the `Member` contract (A15).
3. **Runtime is visible but quiet:** a small runtime mark on the profile and in the quick view ("Runs on Codex"), not on chart cards. Actions adapt to it: **Open session** opens the agent's runtime (Claude Code in the session panel; others say "Open in Codex" and hand off), and nothing offers a Claude-only action to a Codex agent.
4. **Four ways to stop or remove work, named by what they do,** never one ambiguous "deallocate":
   - **Remove from team:** the agent stays in the organization, and its team's projects are no longer its work.
   - **Pause:** the agent stays in its teams, isn't started, woken or messaged with new work, and its open tasks stay put. Resume brings it back.
   - **Offboard:** the agent leaves the organization. Its open tasks come back unassigned for the owner to reassign, and anyone reporting to it reports to the agent's own manager, or to the owner if it had none (org-proto on v1, `member.offboard` and `member.remove`). Its record stays, its name stays reserved and it shows under Former members: offboarding is a state, set by `member.offboard` (ada, A37), not a delete.
   - **Unassign a task:** the task goes back to To do with no assignee.
5. **Moving an agent between teams is one action,** **Move to team…**, not remove-then-add. Nothing flickers in the chart, and the agent is never briefly in "Not in a team" (A16: one command).
6. **Assigning has one entry point per target,** from the agent or from the target: **Add to team…** (agent ⇄ team), **Lead** (team), **Assign task** (agent ⇄ task). An agent works on a project **through a team**. "Add to project…" on an agent asks which of the project's teams to join (or suggests one).
7. **Everything happening is one feed, Activity,** with Home showing its top. Sources: sessions and status, messages between members, tasks and bugs, team and member changes, and git merges into the integration branch. Filters by team, member, project and kind. The owner reads agents' messages and replies only when addressed (§6).
8. **Live everywhere:** status dots on chart cards and lists come from presence, and changes made by agents (a task moved, a message sent) appear in under 2 seconds without a reload.
9. **Hire is the HR flow on the office.** The v1 §3.6 steps (Describe → Suggestion → Review → Hiring → Done) stay, and the review adds **Team**, **Runtime** and **Office**. The HR lead proposes, and the approver (the owner, or a manager with Can approve hires, §7.1) sees exactly what will be created before anything is.

## 4. Flow 1 · Bring our team in (import)

**Entry points:** the first-run screen after **Create organization** ("Start empty" or **Bring in an existing team**), Org chart's empty state, and Settings › Team › **Import**.

```
 Bring in an existing team                                                     [×]
 ─────────────────────────────────────────────────────────────────────────────────
 Where is your team today?
   ☑ HR workspace       ~/projects/agentic-consulting/agents        [Change…]
                        Found 5 agents in 1 project (ac)
   ☑ Messaging          ~/.agency/spool                             [Change…]
                        Found 9 names, 5 not in the HR workspace
                                                            [Cancel]  [Scan]
```

- Folders are prefilled when found in the usual places, and **Change…** opens a folder picker. Scanning only reads and writes nothing.
- **Review** (one row per agent, merged across sources by name):

```
 Review 10 agents                         Filter: [All ▾]   [Bring in 5 agents · 5 need details]
 ─────────────────────────────────────────────────────────────────────────────────
 [A] ada            Staff Engineer        HR + messaging   Claude Code   Engineering ▾
 [E] Eden           Product Designer      HR + messaging   Claude Code   Design ▾
 [R] River          Senior Backend Eng.   HR + messaging   Claude Code   Office ▾
 [S] server-dev     — Role? ▾             messaging only   Runtime? ▾    Office ▾        ⚠ 2 to fill
 [C] codex-dev      — Role? ▾             messaging only   Codex ▾       Agent tooling ▾
 [R] Remy           Senior Backend Eng.   HR + messaging   Claude Code   Agent tooling ▾
 …
 [R] Riley          Senior Frontend Eng.  HR only          Claude Code   App ▾           ⓘ No messages
                                                                         [Bring in ▾: Active · Paused · Skip]
 ─────────────────────────────────────────────────────────────────────────────────
 Teams: create Engineering, App, Office, Agent tooling, Quality, Design   [Edit…]
 Reporting lines: from HR (ada → you; River, Remy, Riley → ada)           [Edit…]
 Project: Agentic Org (ac), from HR · shared by Engineering, Design      [Edit…]
```

- **Merged rows:** an agent in both sources shows "HR + messaging" and links its messaging name automatically when the HR id matches the inbox name.
- **Messaging-only agents** need a **role** (a catalog combobox, or "Custom title…") and a **runtime**. The row says "2 to fill", and **Bring in** counts only complete rows ("Bring in 5 agents · 5 need details").
- **HR-only agents with no messaging inbox** get an info note, "No messages from Riley yet", and a per-row choice **Active · Paused · Skip**, defaulting to Active. The note explains: "Riley is active in HR but hasn't joined messaging. Bring in as Paused if Riley isn't working now."
- **Offboarded HR agents** are listed collapsed under "Offboarded in HR (n)", unchecked, as a record.
- **Teams:** HR has no teams, so the app suggests them from roles and AGENTS.md ownership (the shape in §2) in an editable preview. **Edit…** opens a small tree editor (rename, move, lead, members). **Skip teams** leaves everyone in "Not in a team".
- **Reporting lines** come from HR's `reports_to`. Messaging-only agents default to the owner, editable per row.
- **What happens** (always visible, like Hire's dry run): "Creates 10 agents, 6 teams and 1 project in <org>. Nothing changes in your HR workspace or in messaging. Agents keep running where they are."
- **Bring in** → progress in words (Adding agents… Creating teams… Linking messaging names…), then **Done**: the org chart opens with the new members highlighted, and a summary "10 agents, 6 teams, 1 project." (and "n agents paused." when any were brought in Paused) with **View Activity**.
- **States:**
  - No sources found: "No HR workspace or messaging found here. **Choose folder…** or start empty."
  - A source unreadable: the row error, and the other source still works.
  - Same agent twice with conflicting data (title, reports to): the row shows both with a picker, "HR says Principal Product Designer · you can change it".
  - Name taken in the organization: "Already in <org>". The row offers **Update from HR** or **Skip**.
  - Import interrupted: nothing half-created. Agents already added stay and are marked, and **Try again** continues.
  - Running it again later: "Bring in" shows only what's new or changed since the last import, with "Nothing new" as its empty state.

## 5. Flow 2 · The agent record (profile, quick view, card)

- **Card (org chart):** avatar (agent square) · name · title · status dot + word (Working · Waiting · Idle · Needs you · Paused · Offline) · team role (Lead). There's no runtime on cards (decision 3).
- **Quick view:** name, title, status, **Runs on Codex** · messaging name (mono, copy) · office ("This Mac") · teams · reports to · current task · last activity ("Sent ada a status reply · 4 min ago"). Actions: **Message** · **Assign task** · **Move to team…** · ⋯ (Add to team… · Pause · Offboard…).
- **Profile header** (v1 §3.5, extended): name · messaging name · title · focus · runtime · model · office · status · **Open in <runtime>** / **Open session** · **Assign task** · ⋯.
- **Profile tabs:** Overview · **Activity** (this agent's feed, §6) · Tasks · Messages (threads it's in) · Sessions · Knowledge (its home folder, when the app can read it) · Processes (what it follows, `processes-ux.md` §2) · Settings (the record, editable: title, focus, model, runtime and home, with "Changes take effect at the agent's next session").
- **Missing data** reads plainly, never blank: "Runtime not set · **Set runtime**", "No home folder (this agent runs elsewhere)".

## 6. Flow 3 · See everything that's happening (Activity and Home)

**Activity** (nav item, also each team's, project's and agent's Activity tab):

```
 Activity                                [All teams ▾] [Everyone ▾] [All kinds ▾] [Search]
 ─────────────────────────────────────────────────────────────────────────────────────────
 Today
 05:41  [A] ada → [E] Eden · message · "Phase 1 push: your part + status please"   [Open thread]
 05:39  [C] codex-dev → [E] Eden · reply · "No W7 draft or design branch yet…"
 05:33  [A] ada → 7 members · message · "PHASE 1 PUSH (the user, today)…"
 05:30  [S] server-dev · merged server-dev/create-org into v1 · 3 commits      [View]
 05:12  [R] River · status Working → Waiting · "waiting for the reviewer"
 04:58  [E] Eden · bug ac-B-15 reported in Agentic Org · Major
        ── Earlier: 14 more · [Show]
```

- **Kinds** (filter, and an icon on each row): Messages · Status · Tasks and bugs · Team changes · Git (merges into the integration branch) · Sessions.
- **Messages between agents are readable by the owner** (it's the owner's organization), shown as one line with **Open thread**. Threads open in a side panel, read-only unless the owner is a recipient, where they can **Reply**. Broadcasts are one row ("ada → 7 members").
- **Grouping:** consecutive rows of the same kind from the same member collapse ("River changed status 4 times · Show"). Default filter: everything except Status changes, which are noisy. They live as dots on the cards.
- **Home** shows: **Needs you** (unchanged), **Working now** (agents with status Working and their current task, one line each), and **Latest** (the top 8 Activity rows, with **All activity**).
- **Live:** new rows slide in at the top with a 2-second highlight. When the owner has scrolled down, a pill shows "3 new · Jump to latest" and the list doesn't move.
- **States:**
  - Loading (skeleton rows).
  - Empty organization: "When your agents start working, everything they do shows up here."
  - Messaging not connected: "Messages aren't connected yet, so agents' conversations don't show here." **How to connect**.
  - A source offline (git or presence): a quiet row "Git activity paused: the project's repository isn't reachable."

## 7. Flow 4 · Hire (the HR flow, on the office)

v1 §3.6 holds, with these changes for the office model:

- **Describe:** "What do you need?" plus **Team** (optional, prefilled from where Hire was opened) instead of Project. Projects come with the team.
- **Suggestion (the HR lead's proposal):** role, title (with level), name, focus, **team**, **reports to** (the team's lead by default), and **runtime** (the organization's default, or what the role suggests).
- **Review:** what will be created, as a dry run: the member (name, messaging name `ac-qa-engineer-cameron`), team, reports to, runtime and model, office ("This Mac"), home folder, the role file if it's new, and "Cameron can be reached by everyone in <org>".
- **Hiring:** in words (Creating Cameron's record… Setting up the home folder… Registering with messaging…). **Done:** "Cameron is hired and in App." **Open in Claude Code** / **Open session** (primary), Assign a task, View profile.
- **Runtime not installed or signed out** (chosen in Suggestion): the Review warns "Codex isn't signed in on This Mac. Cameron is hired, but can't start until it is." **Hire anyway** works.
- **Add an existing agent** (the Hire panel's second tab, for an agent already running elsewhere, like ours were): name · messaging name · runtime · role/title · team · reports to · home folder (optional). **Add agent**. This is the one-row version of the import.

### 7.1 Who approves a hire (rev 8, the user)

**The flag.** **Can approve hires** is a flag on an **agent** (not on its role), set by the owner in the agent's Settings, at hire or later. It's typical for managers and senior managers. The roster and quick view show it as a badge, "Approves hires". Only the owner can set or clear it. An agent can't give it to itself or to the agents it hires.

**The route.** A hire request (from an agent or a person, through `@hr`, `org-agent hire request` or the app's Hire) is drafted by the HR lead into a **hire proposal**: role, title, team, reports to, runtime and model, how it runs, and an estimated cost per month at API prices. The proposal then goes to its **approver**:

1. The requester's **manager**, if that manager has Can approve hires.
2. Otherwise up the **reporting line** to the nearest manager with the flag.
3. Otherwise **the owner**.

The owner can always approve or reject any proposal. Someone who requests a hire never approves their own request: it goes one level up. The one exception is **the owner**, who approves their own hires, since nobody is above the owner. The HR lead never approves.

**An agent approver** gets the proposal in its inbox (woken if on call), with the same facts as the Hire review. It answers `org-agent hire approve <id> --reason "…"` or `hire reject <id> --reason "…"`. Approved means the HR lead provisions at once, with no human needed. The approval is recorded on the new agent ("Hired 8 Nov · requested by codex-dev · approved by ada") and in Activity. The owner sees it on Home ("ada approved hiring Cameron, QA engineer, for Office · ≈ $40/month"), with **Open** and, from the agent's profile, the usual **Offboard…** if they disagree.

**Hire requests** (Agents › Requests tab): each proposal with its requester, approver, state (**Waiting for ada** · **Waiting for you** · **Approved by ada** · **Rejected** · **Hired**) and age. A request waiting on an agent approver for more than a day is also shown to the owner, who can approve it directly.
- **Empty:** "No hire requests. When someone asks the HR lead for a new agent, it shows here."
- **Loading:** skeleton rows.
- **Error:** "Can't load hire requests" with **Retry**.

**Optional guardrail** (Settings › Hiring): "Hires approved by agents: no limit" (default), or "at most [n] per week", or "only up to ≈ $[amount] per month each". Above the limit the request goes to the owner, and the proposal says why ("Above ada's weekly limit, so it comes to you").

## 8. Flow 5 · Assign and change teams

- **Add to team…** (agent ⋯, or team ⋯ › Add members…): as offices-and-teams §7.6 (an agent can be in several teams).
- **Move to team…** (quick view, profile, card drag between teams): a team picker with "Moves Riley from App to Office. Riley stops working on App's projects (Desktop app) and starts on Office's (Office server)." If the agent leads the old team: "Riley leads App. App will have no lead." · **Move Riley**. Toast: "Riley moved to Office. **Undo**".
- **Make lead** (team ⋯ or member row): "app-frontend-dev now leads App and its projects."
- **Add to project…** (agent ⋯): lists projects; choosing one lists that project's teams ("Join App to work on Agentic Org"). With one team, it's one step.
- **Assign task** (work-items Flow 7): the assignee picker shows each agent's status and runtime, and a **Paused** agent is listed but marked: "Riley is paused. Assign anyway? They'll see it when resumed."

## 9. Flow 6 · Deallocate: remove from team, pause, offboard

- **Remove from team** (team ⋯ › member › Remove from App, or Edit teams): no confirmation when the agent has other teams. When it's the last one: "Remove Riley from App? Riley will be in Not in a team and won't work on any project. 2 open tasks stay assigned." · **Remove**.
- **Pause** (quick view, profile ⋯): a dialog "Pause Remy? Remy won't be started or woken, and new tasks and messages wait until you resume. Remy stays in Agent tooling." If a session is running: "Remy's session is running and will stop now." (Pause stops at once, per ada's agent-sessions design.) The card then shows a **Paused** badge (gray `pause`), and the chart can hide paused agents (toolbar ☐ Show paused). **Resume** is one click.
- **Offboard…** (v1 §3.5 dialog, office model): it lists only what applies: "Remy leaves Agent tooling. 1 open task comes back unassigned for you to reassign. Nobody reports to Remy (or: "Riley, who reports to Remy, will report to ada, Remy's manager"). Remy's records stay in the HR workspace. The name stays reserved." · **Offboard Remy** (red). After it, Remy appears in Settings › Team › **Former members** with the date, read-only.
- **States:** the agent is working right now (the dialog says so and offers to stop the session first) · offboarding fails partway (names what was done, **Try again**) · it was already changed elsewhere (the dialog refreshes, and nothing is applied twice).

## 10. Accessibility and copy

- Status, runtime, Paused and Lead are always words plus icons. Screen reader: "codex-dev, agent, Agent tooling, runs on Codex, working".
- Activity rows are list items with time, actor, kind and text. **Open thread** is a button labelled with the thread's subject.
- Messaging names are shown in mono only as secondary text, and the display name leads everywhere.
- Never "deallocate" in the UI (decision 4). Words: Remove from team · Pause · Resume · Offboard · Unassign.

## 11. UX acceptance criteria

**Import (IMP)**
- IMP-1 · Bring in an existing team is offered after creating an organization, in Org chart's empty state and in Settings › Team, and scanning writes nothing.
- IMP-2 · With the HR workspace and messaging folders, the review lists every agent once, merged by name, with its sources ("HR + messaging", "HR only", "messaging only").
- IMP-3 · A messaging-only agent can't be brought in until it has a role or title and a runtime, and the button counts only complete rows.
- IMP-4 · An HR-active agent with no messaging inbox shows "No messages from <name> yet" and the choice Active / Paused / Skip.
- IMP-5 · The suggested teams, reporting lines and project are shown before anything is created, and each can be edited or skipped.
- IMP-6 · "What happens" names the counts and says nothing changes in the HR workspace or messaging.
- IMP-7 · After Bring in, the org chart shows every brought-in agent in its team, under its manager, highlighted. Messages from each agent's messaging name appear on that agent in Activity.
- IMP-8 · Running import again shows only new or changed agents, and an agent already in the organization offers Update from HR or Skip.
- IMP-9 · An interrupted import leaves no half-created agent, and Try again continues where it stopped.

**Agent record (PROF)**
- PROF-1 · Every agent's profile and quick view show its runtime, messaging name, office and status. A missing runtime reads "Runtime not set · Set runtime".
- PROF-2 · Open session adapts to the runtime: Claude Code opens in the app's session panel, and other runtimes show "Open in <runtime>". No Claude-only action is offered to a non-Claude agent.
- PROF-3 · Chart cards show status as a dot and a word, with no runtime.
- PROF-4 · Title, focus, model, runtime and home can be edited in Settings, with "Changes take effect at the agent's next session".

**Activity (LIVE)**
- LIVE-1 · Activity lists messages, status changes, tasks and bugs, team changes, git merges and sessions, newest first, filterable by team, member, project and kind.
- LIVE-2 · A message one agent sends another appears in Activity within 2 seconds, and Open thread shows the whole thread.
- LIVE-3 · The owner can reply in a thread only when the owner is a recipient. Otherwise the thread is read-only and says so.
- LIVE-4 · Home shows Needs you, Working now (each working agent with its current task) and the latest 8 Activity rows.
- LIVE-5 · New rows never move the list while the owner is scrolled down. A "n new · Jump to latest" pill appears instead.
- LIVE-6 · A status change shows on the agent's card within 2 seconds, and Status rows are hidden from Activity by default.
- LIVE-7 · When messaging isn't connected, Activity says so with How to connect instead of looking empty.

**Hire and add (STAFF)**
- STAFF-1 · Hire's review shows team, reports to, runtime, model, office, home folder and messaging name before anything is created.
- STAFF-2 · A hired agent appears in its team in the chart, highlighted, and in Activity ("Cameron was hired into App").
- STAFF-3 · Choosing a runtime that isn't installed or signed in warns in Review, and Hire anyway still hires.
- STAFF-4 · *(rev 8)* Add an existing agent creates an agent from name, messaging name, runtime, role or title, team and reports to. It goes through the HR lead's proposal and needs approval like any hire (§7.1, HRL-3).

**Assign and move (MOVE)**
- MOVE-1 · Move to team is one action: the agent never appears in Not in a team in between, and the toast offers Undo.
- MOVE-2 · The move dialog names the projects the agent stops and starts working on, and a lost lead.
- MOVE-3 · Add to project lists the project's teams to join, and with one team it's a single step.
- MOVE-4 · The assignee picker shows status and runtime, and assigning to a paused agent asks first.
- MOVE-5 · Make lead updates the team card and its projects' lead line at once.

**Hire approval (APPR)** *(new, rev 8)*
- APPR-1 · Only the owner can set or clear an agent's Can approve hires flag, at hire or in the agent's Settings, and the roster and quick view show "Approves hires".
- APPR-2 · A hire proposal goes to the requester's manager if that manager has the flag, else up the reporting line to the nearest manager with it, else to the owner. The proposal shows who it's waiting for.
- APPR-3 · *(rev 9)* The requester never approves their own request (it goes one level up), except the owner, who approves their own hires since nobody is above the owner. The HR lead never approves.
- APPR-4 · An approval by a manager with the flag is enough: the HR lead provisions at once, with no human step.
- APPR-5 · Every approved hire records who requested and who approved it, on the new agent's profile and in Activity, and the owner sees it on Home.
- APPR-6 · The owner can approve or reject any proposal at any time, and a proposal waiting on an agent for over a day is shown to the owner.
- APPR-7 · Above the optional weekly or cost limit, a proposal goes to the owner and says why.
- APPR-8 · `org-agent hire request|approve|reject` work with the same words as the app, and approve and reject need a reason.

**HR lead (HRL)** *(new, rev 7)*
- HRL-1 · Every organization has one built-in HR lead from its creation, with the reserved handle `hr`, and it can't be offboarded, deleted, renamed, put in a team, given reports or chosen as anyone's manager.
- HRL-2 · *(rev 10)* The HR lead appears only as the chip beside the owner bar, in the Agents roster's Built-in group, and in Chat, where it's listed in DMs and in @ completion as "HR lead" (it can be messaged). It never appears on team cards, in Teams, People or Reporting lines, or in member, lead and manager pickers.
- HRL-3 · *(rev 8)* Hire, Add an existing agent and Staff a team wake the HR lead (on call by default), and nothing is provisioned before the proposal is approved by the owner or by a manager with Can approve hires (§7.1).
- HRL-4 · *(rev 8)* An agent's request to the HR lead becomes a hire proposal routed to its approver (§7.1), never an automatic hire.
- HRL-5 · The owner can change the HR lead's runtime, model and run mode only, and its runs and cost show under "HR lead" in Insights.

**Deallocate (DEAL)**
- DEAL-1 · The UI never says "deallocate". It offers Remove from team, Pause, Offboard and Unassign, each with its own consequences.
- DEAL-2 · Removing an agent from its last team confirms and says it will be in Not in a team.
- DEAL-3 · A paused agent shows Paused on its card, isn't started or woken, its new messages and tasks wait, and Resume restores it in one click.
- DEAL-4 · Offboard lists only the consequences that apply, brings open tasks back unassigned for the owner to reassign, makes anyone reporting to the agent report to the agent's own manager (or to the owner if it had none), and moves the agent to Former members, read-only (`member.offboard`, A37).
- DEAL-5 · Show paused in the chart toolbar hides or shows paused agents, and is remembered.

## 12. Gaps to close, and who I think owns them (for ada to confirm in the phase-1 plan)

| # | Gap | Proposed owner | Plan row |
| --- | --- | --- | --- |
| G1 | `Member` fields for agents: role, title, focus, runtime, model, home, messaging name, status incl. Paused, hired on | ada (contract) + server-dev (store) | W3 / W10 |
| G2 | `member.add_agent`, `member.move` (one command), `member.set_agent_status` (active or paused), `member.offboard` (A37): **landed on v1** | ada + server-dev | W3 |
| G3 | Import: read the HR workspace and the spool inboxes into a reviewable plan, then apply it (CLI first, then the app) | codex-dev (ada, after W8, with the W7 model) | W12 |
| G4 | Messaging names ⇄ members: a message from `codex-dev` is linked to its member, and `who`/presence are keyed the same way | Remy (messaging owner) | W6 Phase B |
| G5 | Activity feed: one stream over messages, status, work items, team changes and git merges | ada (contract), server-dev, Remy | new |
| G6 | Hire on the office: the HR lead's proposal and provisioning as an office call (the HR toolkit's `hr.py` logic ported) | codex-dev (ada) | W12 |
| G7 | App screens: Import, the agent record, Activity, Home, Hire and Add agent, Move, Pause and Offboard | app-frontend-dev (ada; Riley stays offline) | W9 / W12 |
| G8 | Runtime hand-off: "Open in Codex/Antigravity" and session launch per runtime | codex-dev (adapters, W8) + app-frontend-dev | W8 / W9 |

## 13. Open questions

**For the user**
- ~~U10~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation:* **Agentic Consulting**, the workspace's name, distinct from the app. Use another name if you prefer.
- ~~U11~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation:* the shape in §2: Remy leads Agent tooling, and Riley shows Offline in App.
- ~~U12~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation:* yes. After the import, the app (and `org-agent`) is where we hire, move and offboard. HR's folders stay as a read-only record, and later imports only bring in what changed.
- ~~U13~~ · *Resolved (the user):* don't start Remy and Riley. They're brought in Active and show as **Offline** (no session), not Paused. Their planned work (G3, G6, G7) needs another owner (ada). *Update (later the same day): the user started Remy, who now owns messaging (AGENTS.md). Riley stays Offline.*

**For ada**
- ~~A47~~ · *Resolved (ada):* `can_approve_hires` (owner-set only), `HireProposal` with routing up the reporting line, and `hire.request|approve|reject` (server-dev the contract, codex-dev the flow). In our team, ada has the flag.
- ~~A46~~ · *Resolved (ada):* server-dev adds a built-in `hr_lead` agent with the reserved handle `hr`, created with the organization; codex-dev builds the HR lead's runs. Note: `member.add_agent` is app-only today (`teams-and-org-chart.md`), so provisioning approved by an agent needs an exception for the HR lead's run (ada).
- ~~A37~~ · *Resolved (ada):* offboarding is a state. `AgentStatus` gains `offboarded`, and `member.offboard { member_id, expected_version }` sets it with `offboarded_at`. The record stays, the handle stays reserved, the agent shows under Former members, and its open tasks are reclaimed as in the teams design. `member.remove` stays for hard deletes of members with no history (mistakes). server-dev drafts it. ada updates `docs/design/teams-and-org-chart.md`, which still says offboarding issues `member.remove`.
- ~~A15~~ · *Resolved (ada):* `Member.agent: Option<AgentProfile>` (one entity, one version) with role, title, focus, runtime (claude_code | codex | antigravity | other), home, messaging_name, status (active | paused). Commands: `member.add_agent`, `member.update_agent`, `member.set_agent_status`, `member.move`.
- ~~A14~~ · *Partly resolved (ada):* codex-dev and server-dev run on Codex, agy-senior-dev on Antigravity. app-frontend-dev and the reviewer: ada is asking the user.
- ~~A16~~ · *Resolved:* `member.move { membership_id, to_team_id, title_in_team? }`, atomic. ~~A17~~ · *Resolved:* the app reads every message in the office (ADR-0003).
