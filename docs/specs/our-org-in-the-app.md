---
kind: spec
title: "Our own team in Agentic Org (dogfooding): bring in, see, hire, assign, move, deallocate (UX)"
status: for-review
revision: 21
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-10"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner, app-frontend-dev, server-dev, ac-software-engineer-river, ac-software-engineer-remy, codex-dev]
builds_on: [offices-and-teams-ux, work-items-ux]
---

# Spec · Our own team in Agentic Org (dogfooding): bring in, see, hire, assign, move, deallocate (UX)

**Revision 21 (2026-11-10): hiring when the HR lead can't run (O-7, the owner via ada).**
Hire and Add agent record both work without the HR lead. When it can't run (the
server doesn't run it yet, its runtime is missing or signed out, or its run fails),
Hire's proposal is built from the fields the requester fills in (§7). Add agent
record always builds its proposal from its submitted fields, even when the HR lead
can run, since they're complete. An authorized approver still approves every
proposal, nothing is provisioned before that, and direct API callers can't bypass
it (SRV-30). Nothing waits on the HR lead: approved hires and offboarding complete
without it (§2a). New states: the HR lead's chip and quick view when it can't run,
an approver who can't act, approved but not hired with **Try again**, a runtime the
office can't check, `@hr` and `org-agent hire request` when the HR lead can't run,
and Review and Done before autonomy, role files, cost estimates, run modes and
sessions exist. "Staff a team" is dropped from §2a and HRL-3: no spec defines its
flow. **Criteria rewritten (same ID):** STAFF-4, HRL-3, APPR-2, APPR-4, APPR-8
*(rev 21)*. **New:** STAFF-5, STAFF-6, STAFF-7, HRL-6, HRL-7, HRL-8, APPR-9,
APPR-10. Open questions A97 and A98.

**Revision 20 (2026-11-10):** the success test names the sections of the
[dogfood run](dogfood-run.md) (rev 7) that check each of its items. No criteria
changed. §7.1 says the new-role route waits for role files (after v1.0). Open question A96 (the approval check on `member.add_agent`).

**Revision 18 (2026-11-09): fresh standalone-v1 setup.**
The owner confirmed that no application was deployed. Existing-team import and
folder selection are removed from v1; IMP-1 through IMP-11 are retired, not
renumbered. [server-installation-and-access-ux.md](server-installation-and-access-ux.md)
defines server-managed roots and browser access. Create test members through real
organization/hiring operations; retain profile, team, approvals and lifecycle
behavior. Historical import examples/changelog entries are not release requirements.

Builds on `offices-and-teams-ux.md` rev 4, `work-items-ux.md` rev 5 and `desktop-app-v1.md` (in Eden's notes, not in this repository) (hire §3.6, profile §3.5).

**Revision 17 (2026-11-09): roles and Can approve processes.** The profile gains a **Role** tab, showing the agent's effective role with its layers (`roles-ux.md`). The agent's Settings gains **Change role…** (owner only) and **Can approve processes** (owner only, like Can approve hires, `processes-ux.md` decision 8), shown as an "Approves processes" badge on the roster and quick view. A hire that needs a new role goes straight to the owner (§7.1). **Criteria rewritten (same ID):** APPR-1, APPR-2, APPR-4 *(rev 17)*.

**Revision 16 (2026-11-09): autonomy.** The agent record gains an autonomy level, Asks first or Full autonomy, which only the owner sets: in the profile's Settings and in Hire's Review, where a role's default is shown and confirmed (`processes-ux.md` §4a). Activity gains the Decisions kind, hidden by default (§6). **Criteria rewritten (same ID):** LIVE-1 *(rev 16)*. See also PROC-20 to PROC-26.

**Revision 15 (2026-11-08): who agents report to by default (from the bring-in build review).** An agent with no `reports_to` in HR (every messaging-only agent) now defaults to the lead of its top-level team, unless it leads that team itself, and to the owner otherwise. HR's `reports_to` still wins. For our team this gives the §2 lines with no edits (§4). **Added:** IMP-11.

**Revision 14 (2026-11-08): role and title, messaging names (app-frontend-dev's import work).** The agent record needs both a role and a title (org-proto `AgentProfile`), so a messaging-only row needs a role, a title and a runtime: the Role combobox sets the role and defaults the title to its words, and **Custom title…** adds a title beside the role, which stays visible and required (suggested from the title when empty, marked "Suggested"). A row whose messaging name the organization would refuse shows "Needs a messaging name" with the reason (§4). Direct agent creation also asks for both (§7, STAFF-4). **Criteria rewritten (same ID):** IMP-3, STAFF-4 *(rev 14)*.

**Revision 13 (2026-11-08): where suggested teams come from (app-frontend-dev's question).** The HR records have a `department` but nothing about sub-teams, and `AGENTS.md` isn't in the HR workspace, so teams can't be suggested "from roles and AGENTS.md ownership". Now: top-level teams come from `department`; every review row has a **Team** picker (default: its department's team, or "Not in a team" for an agent with no department); and an optional team hints file the owner chooses (the **Team hints** row's **Choose…**, a `teams.toml` from anywhere, never in or written into `hr/`, HR's ground, ada) prefills teams, parents, leads and members (§4). **Added:** IMP-10.

**Revision 12 (2026-11-08): live updates paused.** When the app stops getting live changes from the office (the feed drops, for example when the office restarts, or listening fails), every screen shows a line saying so, with **Retry**, instead of looking current (§6, decision 8). From Eden's review of the live org screens with app-frontend-dev; the reviewer asked for it to be specified. **Added:** LIVE-8.

**Revision 11 (2026-11-08): import counts.** The import's examples now match our team today: 10 agents (Remy is working, and Riley comes in Active and shows Offline, U13), so "Bring in 5 agents · 5 need details" until the five messaging-only agents get a role and runtime, then "Bring in 10 agents", and a summary with none paused; offboarded HR agents are counted as "(n)", since none are today (decision 1, §4). No criteria changed.

**Revision 10 (2026-11-08): small fixes.** HRL-2 lists member, lead and manager pickers, matching §2a. The profile tabs include **Processes** (`processes-ux.md` §2).

**Revision 9 (2026-11-08): review fixes.** The requester never approves their own request (it goes one level up), except the owner, who approves their own hires since nobody is above the owner (§7.1). In Chat the HR lead is listed in DMs and in @ completion as "HR lead". The Requests tab has empty, loading and error states (§7.1). U10 and U11 now read as approved: the organization is **Agentic Consulting**, with the shape in §2. APPR-3 and HRL-2 rewritten.

**Revision 8 (2026-11-08): approving hires without the owner (the user).** A flag on an agent, **Can approve hires**, lets that agent approve hires. A hire request goes to the requester's manager, and if that manager has the flag, its approval is enough: the HR lead provisions with no human in the loop. Otherwise the request goes up the reporting line to the nearest manager with the flag, and finally to the owner (§7.1). §2a, HRL-3 and HRL-4 rewritten. APPR-1 to APPR-8 added.

**Revision 7 (2026-11-08): the HR lead is built in (the user).** HR is a built-in agent, "HR lead", that only provisions agents (§2a). The user approved my recommendations U10–U12 (the name Agentic Consulting, the team shape in §2, and the app as where we manage the team). HRL-1 to HRL-5 added.

**Revision 6 (2026-11-08): aligned with ada's W7 model.** Item numbers in Activity follow `work-items-ux.md` rev 6 and carry the project key, since Activity spans projects (`ac-B-15`). No criteria changed.

**Revision 5 (2026-11-08): follows org-proto on v1.** Offboarding or removing a member sends their reports to that member's own manager, or to the owner if they had none (not always to the owner). The historical local-desktop Pause stopped a running session at once; standalone §9 now distinguishes HQ status from pending office stop acknowledgement. G2 uses `member.set_agent_status`. The survey notes what v1 has gained since.

**Revision 4 (2026-11-08): Remy is online.** The user started Remy, who owns messaging and `org-agent` (AGENTS.md on v1) since agy-senior-dev ran out of credits. §2 moves him to Agent tooling as its lead. Riley stays Offline.

**Revision 3 (2026-11-08): reviewer fixes.** Offboarding's kept records, reserved name and Former members are now a contract request (A37), because `member.remove` deletes the member today. The spool-import facts follow ADR-0003. Duplicate open questions removed.

**Revision 2 (2026-11-08):** the user: Remy and Riley aren't started, and they show as **Offline**. Fixes from review: offboarding follows the teams design (reports go to the owner, and open tasks come back unassigned), and git and GitHub belong to River. My recommendations for U10–U12.

## The goal (the user, 2026-11-08)

> All of us (ada, Eden, the reviewer, codex-dev and the other devs) are registered as an organization in the application. From it you can see everything that's happening, add agents, hire more, deallocate agents, assign agents and change teams, with a great user experience.

**Success test (the dogfood run).** In the real HQ browser application, with no mock data:
1. The owner creates a fresh organization and provisions members through supported organization/hiring operations. Each agent appears under the right team and manager with its runtime; no HR/spool import is required.
2. Home and Activity show, live, what the team is doing: who's working, the messages they send each other (like ada's phase-1 push), tasks and bugs moving, and merges into `v1`.
3. The owner hires a new agent, assigns it to a team, a project and a task, moves an agent to another team, pauses an idle agent and offboards one. Every change shows at once in the chart and Activity, and every agent keeps working on its own runtime.

The [dogfood run](dogfood-run.md) checks item 1 in its §1, §3 and §3a, item 2 in §3b, and item 3 in §3a.

## Where we are (survey of `origin/v1` at 0f7df26, 2026-11-08)

*Since this survey, v1 gained agents as members (`Member.agent: AgentProfile`, `86a6a9f`), `member.set_agent_status` and run modes, and `member.offboard` (A37, `2911ffc`). G1 and G2 below track the rest.*

- **App:** create, open and list organizations work on a **mock** (`MockOffice`). Agents, Projects, Tasks, Activity and Settings are placeholders. The org chart is work in progress on sample data (app-frontend-dev's worktree).
- **Office:** the server and host are stubs on `v1`. server-dev's store (local branch) has organizations only. Teams and members are a **contract only** (`org-proto`: `Member { id, kind, name, contact, owner, office_id, reports_to }`, `team.*`).
- **Hiring has no design and no contract.** The teams design says agents "join through hiring (the HR flow, its own design)", and that design was never written. This spec writes the UX.
- **Nothing imports an existing team.** The only planned import (ADR-0003 Phase B, `import-v1-old-spool`) brings the spool's messages, profiles, statuses and read/snooze state into HQ, not team members. Live Phase A coordination state has the explicit cutover/retention contract in ADR-0003. Imported profiles/statuses remain historical provenance, never new members, managed homes or launch authority; retired IMP criteria do not authorize team import.
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

Every organization has one built-in agent, **HR lead**, created with the organization. Its only job is **provisioning agents**: it drafts hire proposals (from Hire's description and from agents' hire requests) *(rev 21)*, drafts new roles, and does the provisioning cleanup when the owner offboards an agent. It doesn't manage, lead, triage or take work.

- **Always there, never removable:** it can't be offboarded, deleted, moved into a team or given reports, and nobody reports to it. Its name is fixed ("HR lead"), and its handle `hr` is reserved like `owner`. The owner can change only its runtime, model and how it runs.
- **On call by default** (`agent-run-modes-ux.md`): asleep until a Hire or a hire request wakes it (Add agent record doesn't, §7) *(rev 21)*. Its runs and cost show under "HR lead" in Insights.
- **Where it shows:** beside the owner bar in the org chart as a compact chip, "HR lead · built-in · Asleep · wakes when called", which opens its quick view; in the Agents roster's own **Built-in** group at the top; and in Chat, in Direct messages and in @ completion, as "HR lead" (it can be messaged). It never appears on a team card, in Teams, People or Reporting lines, or in member, lead and manager pickers.
  *(rev 21)* When it can't run, the chip reads "HR lead · built-in · **Not running** · hires are filled in by hand" (the server doesn't run it yet) or "HR lead · built-in · **Can't start** · hires are filled in by hand" (its run failed to start, `agent-run-modes-ux.md` §3), and the Built-in group row says the same. Its quick view says why and what still works: "The HR lead can't run on this server yet. Hire and Add agent record still work: you fill in the details, and the hire is approved as usual." For a failed start it names the reason and the fix ("Codex isn't signed in on <instance>"), with **Run now** to try again. It never shows "Asleep · wakes when called" while it can't run.
- **When it can't run** *(rev 21)*: hiring doesn't wait for it. Hire's proposal is built from the fields the requester fills in (§7), Add agent record never needs it, approved hires are provisioned and Offboard does its cleanup without it, and `@hr` and `org-agent hire request` say it can't run (HRL-8, APPR-8).
- **Someone always approves** *(rev 8)*: every hire needs an approved proposal, whoever drafted it *(rev 21)*, either by the owner or by a manager with **Can approve hires** (§7.1). Agents ask it (`@hr` in Chat, or `org-agent hire request "…"`), and the request is routed to its approver. Hiring is never automatic.
- **Import:** the HR toolkit's own agent (`.claude/agents/hr.md` in the agents repo) maps onto the built-in HR lead. It isn't brought in as a member.

## 3. Decisions

1. **Start with a fresh organization.** Setup and the authorized browser provision members through normal domain operations. There is no v1 scanner, import wizard or old-team migration.
2. **The agent record grows** to describe how an agent works, shown on every profile and card: role, title, focus, **runtime** (Claude Code · Codex · Antigravity), model, **home folder**, **messaging name** (`codex-dev`), office, hired on, and status. Those are UX requirements for the `Member` contract (A15).
3. **Runtime is visible but quiet:** show the runtime on profiles/quick views. Browser session inspection uses HQ APIs; interactive takeover is an explicitly authorized CLI operation on the office machine unless a separate browser-terminal capability is implemented. No native embedded session window is required for v1.
4. **Four ways to stop or remove work, named by what they do,** never one ambiguous "deallocate":
   - **Remove from team:** the agent stays in the organization, and its team's projects are no longer its work.
   - **Pause:** the agent stays in its teams, isn't started, woken or messaged with new work, and its open tasks stay put. Resume brings it back.
   - **Offboard:** the agent leaves the organization. Its open tasks come back unassigned for the owner to reassign, and anyone reporting to it reports to the agent's own manager, or to the owner if it had none (org-proto on v1, `member.offboard` and `member.remove`). Its record stays, its name stays reserved and it shows under Former members: offboarding is a state, set by `member.offboard` (ada, A37), not a delete.
   - **Unassign a task:** the task goes back to To do with no assignee.
5. **Moving an agent between teams is one action,** **Move to team…**, not remove-then-add. Nothing flickers in the chart, and the agent is never briefly in "Not in a team" (A16: one command).
6. **Assigning has one entry point per target,** from the agent or from the target: **Add to team…** (agent ⇄ team), **Lead** (team), **Assign task** (agent ⇄ task). An agent works on a project **through a team**. "Add to project…" on an agent asks which of the project's teams to join (or suggests one).
7. **Everything happening is one feed, Activity,** with Home showing its top. Sources: sessions and status, messages between members, tasks and bugs, team and member changes, and git merges into the integration branch. Filters by team, member, project and kind. The owner reads agents' messages and replies only when addressed (§6).
8. **Live everywhere:** status dots on chart cards and lists come from presence, and changes made by agents (a task moved, a message sent) appear in under 2 seconds without a reload.
9. **Hire is the HR flow on the office.** The v1 §3.6 steps (Describe → Suggestion → Review → Hiring → Done) stay, and the review adds **Team**, **Runtime** and **Office**. The HR lead proposes (or, when it can't run, the requester fills in the proposal; Add agent record's fields are always its proposal, §7) *(rev 21)*, and the approver (the owner, or a manager with Can approve hires, §7.1) sees exactly what will be created before anything is.

## 4. Existing-team import (retired from v1)

Remove the Bring in entry points, HR/spool scanner, team-hints file picker and
import-only state. Agent homes/configuration are provisioned under the selected
server instance. Paths may be shown read-only to authorized users; a browser path
is not a path on the office machine. The roster and HR/hiring flows below remain.
The [dogfood run](dogfood-run.md) starts with fresh disposable instances.

## 5. Flow 2 · The agent record (profile, quick view, card)

- **Card (org chart):** avatar (agent square) · name · title · status dot + word (Working · Waiting · Idle · Needs you · Paused · Offline) · team role (Lead). There's no runtime on cards (decision 3).
- **Quick view:** name, title, status, **Runs on Codex** · messaging name (mono, copy) · office (its service machine/instance) · teams · reports to · current task · last activity ("Sent ada a status reply · 4 min ago"). Actions: **Message** · **Assign task** · **Move to team…** · ⋯ (Add to team… · Pause · Offboard…).
- **Profile header** (v1 §3.5, extended): name · messaging name · title · focus · runtime · model · office · status · **View session** · **Assign task** · ⋯.
- **Profile tabs:** Overview · **Activity** (this agent's feed, §6) · Tasks · Messages (threads it's in) · Sessions · Knowledge (authorized server-provided content; no direct browser filesystem access) · Processes (what it follows, `processes-ux.md` §2) · **Role** (its effective role: the role it was hired as, its own override, and what processes supersede, `roles-ux.md` §1) *(rev 17)* · Settings (the record, editable: title, focus, model and runtime; home path is server-managed and read-only, with "Changes take effect at the agent's next session"; and, for the owner only, **Autonomy**: Asks first or Full autonomy, `processes-ux.md` §4a *(rev 16)*, and **Can approve hires** and **Can approve processes** (§7.1, `processes-ux.md` decision 8), and **Change role…** (`roles-ux.md` decision 7) *(rev 17)*).
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

- **Kinds** (filter, and an icon on each row): Messages · Status · Tasks and bugs · Team changes · Git (merges into the integration branch) · Sessions · *(rev 16)* Decisions (agents' recorded decisions and consults, `processes-ux.md` §4a).
- **Messages between agents are readable by the owner** (it's the owner's organization), shown as one line with **Open thread**. Threads open in a side panel, read-only unless the owner is a recipient, where they can **Reply**. Broadcasts are one row ("ada → 7 members").
- **Grouping:** consecutive rows of the same kind from the same member collapse ("River changed status 4 times · Show"). Default filter: everything except Status changes, which are noisy (they live as dots on the cards), and Decisions, which are for auditing (shown with the Decisions filter, and never on Home) *(rev 16)*.
- **Home** shows: **Needs you** (unchanged), **Working now** (agents with status Working and their current task, one line each), and **Latest** (the top 8 Activity rows, with **All activity**).
- **Live:** new rows slide in at the top with a 2-second highlight. When the owner has scrolled down, a pill shows "3 new · Jump to latest" and the list doesn't move.
- **Live updates paused** *(rev 12)*: when the app isn't getting live changes from the office (the connection to the office ended or couldn't be made, for example while the office restarts, or listening failed), a line shows at the top of every screen, under the title: ⚠ "Live updates paused. Showing what was loaded at 21:51." (the time the organization was last loaded, in the app's 24-hour format; if nothing has loaded yet, the line reads "Live updates paused." alone) with **Retry**, which reconnects now and reads "Retrying…" while it works. The line clears by itself once live changes are back and the latest has loaded. It stays while the owner moves between screens. Word plus icon, never color alone; screen readers announce it once when it appears, not on every retry.
- **States:**
  - Loading (skeleton rows).
  - Empty organization: "When your agents start working, everything they do shows up here."
  - Messaging not connected: "Messages aren't connected yet, so agents' conversations don't show here." **How to connect**.
  - A source offline (git or presence): a quiet row "Git activity paused: the project's repository isn't reachable."

## 7. Flow 4 · Hire (the HR flow, on the office)

v1 §3.6 holds, with these changes for the office model:

- **Describe:** "What do you need?" plus **Team** (optional, prefilled from where Hire was opened) instead of Project. Projects come with the team.
- **Suggestion (the HR lead's proposal):** role, title (with level), name, focus, **team**, **reports to** (the team's lead by default), and **runtime** (the organization's default, or what the role suggests). *(rev 21)* While the HR lead drafts, the screen says "HR lead is reading…". A wake taking over 60 s shows "HR lead is still starting…" with **Details** and **Fill in the details yourself** (`agent-run-modes-ux.md` §3).
- **When the HR lead can't run** *(rev 21, §2a)*: Describe's "What do you need?" field is disabled with the reason ("The HR lead can't run on this server yet." or "The HR lead can't start: Codex isn't signed in on <instance>."), and the primary action is **Fill in the details yourself**. It opens Suggestion as a form: role, title, name and focus are empty and marked **Your choice needed**, and team (from Describe), reports to (the team's lead) and runtime (the organization's default) start at their usual defaults. There's no "HR's reason" line. **Next** stays disabled until every field has a value, and says what's missing ("Add a name and a focus to continue"). If the HR lead's run stops while drafting, the screen says "The HR lead stopped: <reason>." with **Fill in the details yourself** and **Try again**, and what was typed in Describe stays above the form as a note. Review, approval, Hiring and Done are the same as when the HR lead drafts. Roles are free text until role files exist (`roles-ux.md`); then the form lists the existing roles, and a new role needs the HR lead: "New roles need the HR lead. Pick an existing role, or try again once it's running."
- **Review:** what will be created, as a dry run: the member (name, messaging name `ac-qa-engineer-cameron`), team, reports to, runtime and model, autonomy (Asks first, or the role's default for the owner to confirm, `processes-ux.md` §4a *(rev 16)*), office (its service machine/instance), home folder, the role file if it's new (a hire that needs a new role goes to the owner, even when an agent approves hires, `roles-ux.md` decision 4 *(rev 17)*), and "Cameron can be reached by everyone in <org>". *(rev 21)* Rows for features that don't exist yet are left out, never shown empty or with a made-up value: autonomy until Autonomy exists (agents hired before then start at Asks first, its default), the role file until role files exist, How it runs until the office runs agents (`agent-run-modes-ux.md` §2), and the estimated cost per month (§7.1) until cost estimates exist.
- **Hiring:** in words (Creating Cameron's record… Setting up the home folder… Registering with messaging…). **Done:** "Cameron is hired and in App." **View session** (primary), Assign a task, View profile. *(rev 21)* Until the office runs agent sessions, **View profile** is primary and View session isn't shown.
- **Hiring stops partway** *(rev 21)*: "Couldn't finish hiring Cameron: <step that failed>. Already done: <steps done>." with **Try again**, which finishes the hire without a new approval. Cameron isn't shown as a member (chart, roster, pickers) until the hire completes, and the request shows **Approved, not hired** (§7.1, A97).
- **Runtime not installed or signed out** (chosen in Suggestion): the Review warns "Codex isn't signed in on <office machine / instance>. Cameron is hired, but can't start until its administrator completes runtime setup there." **Hire anyway** works.
- **The office can't check the runtime** *(rev 21)* on that instance: the Review says "Can't check whether Codex is installed and signed in on <instance>. Cameron will be hired; check the runtime setup there before Cameron starts." **Hire anyway** works.
- **Add agent record** (the standalone-v1 label for **Add an existing agent**) submits
  the same proposal and approval workflow as Hire. *(rev 21)* Its fields are the
  proposal: the HR lead never drafts or changes it, even when it can run, so it
  works the same whether or not the HR lead can run. It captures name,
  messaging name, runtime, role/title, team, reports-to and office as proposal data.
  Nothing is provisioned until an authorized manager with Can approve hires or the
  owner approves under §7.1. Only then does the office materialize the member, managed
  home/configuration and messaging binding. Direct HTTP/member-creation calls must
  enforce that approved proposal; authorization alone is not a bypass. Initial
  built-in HR/owner bootstrap during organization creation is the explicit exception.
  No external home path, old-team import or runtime takeover is accepted.

### 7.1 Who approves a hire (rev 8, the user)

**The flag.** **Can approve hires** is a flag on an **agent** (not on its role), set by the owner in the agent's Settings, at hire or later. It's typical for managers and senior managers. The roster and quick view show it as a badge, "Approves hires". Only the owner can set or clear it. An agent can't give it to itself or to the agents it hires.

**The route.** A hire request (from an agent or a person, through `@hr`, `org-agent hire request` or the app's Hire) becomes a **hire proposal**, drafted by the HR lead, or built from the fields the requester filled in (Add agent record, or Hire when the HR lead can't run, §7) *(rev 21)*: role, title, team, reports to, runtime and model, how it runs, and an estimated cost per month at API prices (how it runs and the cost once those features exist, §7 Review). The proposal then goes to its **approver**:

1. The requester's **manager**, if that manager has Can approve hires.
2. Otherwise up the **reporting line** to the nearest manager with the flag.
3. Otherwise **the owner**.

*(rev 17)* A proposal that needs a **new role** (no role file fits) goes straight to the owner, skipping the steps above, since a new role is the owner's to approve (`roles-ux.md` decision 4). It's one approval: the owner approves the role and the hire together. Its state reads "Waiting for you: new role". *(rev 20)* This route starts when role files ship (`roles-ux.md`, after v1.0). Until then roles are free text, no proposal is classed as needing a new role, and proposals follow the steps above.

The owner can always approve or reject any proposal. Someone who requests a hire never approves their own request: it goes one level up. The one exception is **the owner**, who approves their own hires, since nobody is above the owner. The HR lead never approves.

**An agent approver** gets the proposal in its inbox (woken if on call), with the same facts as the Hire review. It answers `org-agent hire approve <id> --reason "…"` or `hire reject <id> --reason "…"`. Approved means the office provisions the hire at once, with no human needed, whether or not the HR lead can run *(rev 21)*. The approval is recorded on the new agent ("Hired 8 Nov · requested by codex-dev · approved by ada") and in Activity. The owner sees it on Home ("ada approved hiring Cameron, QA engineer, for Office · ≈ $40/month"), with **Open** and, from the agent's profile, the usual **Offboard…** if they disagree.

**An approver who can't act** *(rev 21)*: when a proposal's agent approver can't run (the server doesn't run agents yet, or its runtime is missing or signed out), is Paused, or is at an offline office, the proposal shows to the owner at once, not after a day, with the reason: "Waiting for ada · ada can't run right now, so you can approve it." (or "ada is paused", "ada's office is offline"). It stays waiting for ada too, and the first answer decides; a later answer is refused with "Already approved by you." When the approver is offboarded, the proposal moves to the next approver up the line (steps 2–3) and says "Was waiting for Remy, who left. Now waiting for ada."

**Hire requests** (Agents › Requests tab): each proposal with its requester, approver, state (**Waiting for ada** · **Waiting for you** · **Waiting for you: new role** *(rev 17)* · **Approved by ada** · **Hiring…** · **Approved, not hired** *(rev 21)* · **Rejected** · **Hired**) and age. A request waiting on an agent approver for more than a day is also shown to the owner, who can approve it directly.
- **Empty:** *(rev 21)* "No hire requests. Hires and agent records sent for approval show here."
- **Approved, not hired** *(rev 21)*: "Approved by ada · not hired: couldn't register with messaging" with **Try again** for the owner, and a Needs you item: "Hiring Cameron stopped: couldn't register with messaging. **Try again**". The row names what was already done. Try again finishes the hire without a new approval, and Cameron isn't shown as a member until it completes (A97).
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
- **Pause** (quick view, profile ⋯): a dialog "Pause Remy? HQ will record Pause. Remy stays in Agent tooling; new work waits once the office applies the change." If a session is running: "The office will stop the session when it receives the change." After HQ commits, show **Paused · stop pending at office** until the office confirms; an offline office shows last-observed runtime/freshness, never "stopped now". Then show **Paused** with the confirmed stop outcome. The chart can hide paused agents (toolbar ☐ Show paused), while pending control status remains inspectable. **Resume** is one versioned, deduplicated action; a later Resume supersedes an undelivered Pause.
- **Offboard…** (v1 §3.5 dialog, office model): it lists only what applies: "Remy leaves Agent tooling. 1 open task comes back unassigned for you to reassign. Nobody reports to Remy (or: "Riley, who reports to Remy, will report to ada, Remy's manager"). Remy's records stay in the HR workspace. The name stays reserved." · **Offboard Remy** (red). After it, Remy appears in Settings › Team › **Former members** with the date, read-only.
- **States:** the agent is working right now (the dialog says so and offers to stop the session first) · offboarding fails partway (names what was done, **Try again**) · it was already changed elsewhere (the dialog refreshes, and nothing is applied twice).

## 10. Accessibility and copy

- Status, runtime, Paused and Lead are always words plus icons. Screen reader: "codex-dev, agent, Agent tooling, runs on Codex, working".
- Activity rows are list items with time, actor, kind and text. **Open thread** is a button labelled with the thread's subject.
- Messaging names are shown in mono only as secondary text, and the display name leads everywhere.
- Never "deallocate" in the UI (decision 4). Words: Remove from team · Pause · Resume · Offboard · Unassign.

## 11. UX acceptance criteria

**Import (IMP)**
- IMP-1 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-2 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-3 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-4 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-5 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-6 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-7 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-8 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-9 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-10 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.
- IMP-11 · Retired in rev 18: fresh standalone-v1 setup has no existing-team import. This ID is not reused.

**Agent record (PROF)**
- PROF-1 · Every agent's profile and quick view show its runtime, messaging name, office and status. A missing runtime reads "Runtime not set · Set runtime".
- PROF-2 · *(rev 18)* View session opens authorized browser status/log inspection through HQ. Interactive takeover requires an explicit supported action on the owning office machine; v1 requires no native embedded terminal or runtime deep link.
- PROF-3 · Chart cards show status as a dot and a word, with no runtime.
- PROF-4 · *(rev 18)* Title, focus, model and runtime can be edited in Settings, with Changes take effect at the next session. Home/configuration paths are server-managed and displayed read-only from the same office-resolved path used by the launcher; home/clear_home inputs are rejected, and no path picker or relocation is offered.

**Activity (LIVE)**
- LIVE-1 · *(rev 16)* Activity lists messages, status changes, tasks and bugs, team changes, git merges, sessions and decisions, newest first, filterable by team, member, project and kind; Status and Decisions rows are hidden by default.
- LIVE-2 · A message one agent sends another appears in Activity within 2 seconds, and Open thread shows the whole thread.
- LIVE-3 · The owner can reply in a thread only when the owner is a recipient. Otherwise the thread is read-only and says so.
- LIVE-4 · Home shows Needs you, Working now (each working agent with its current task) and the latest 8 Activity rows.
- LIVE-5 · New rows never move the list while the owner is scrolled down. A "n new · Jump to latest" pill appears instead.
- LIVE-6 · A status change shows on the agent's card within 2 seconds, and Status rows are hidden from Activity by default.
- LIVE-7 · When messaging isn't connected, Activity says so with How to connect instead of looking empty.
- LIVE-8 · *(new, rev 12)* When live changes from the office stop (the connection ended or couldn't be made, or listening failed), every screen shows "Live updates paused. Showing what was loaded at <HH:MM>." (24-hour, the organization's last load; "Live updates paused." alone if nothing has loaded yet) with Retry ("Retrying…" while it works). The line clears once live changes resume, and it persists across screens until then.

**Hire and add (STAFF)**
- STAFF-1 · Hire's review shows team, reports to, runtime, model, office, home folder and messaging name before anything is created.
- STAFF-2 · A hired agent appears in its team in the chart, highlighted, and in Activity ("Cameron was hired into App").
- STAFF-3 · Choosing a runtime that isn't installed or signed in warns in Review, and Hire anyway still hires.
- STAFF-4 · *(rev 21)* Add agent record (formerly Add an existing agent) submits name, messaging name, runtime, role/title, team, reports-to and office as a hire proposal built from those fields, the same whether or not the HR lead can run (it never drafts or changes them), with the same approval as Hire. Only approval under §7.1/HRL-3 permits member/home/messaging provisioning; direct API calls cannot bypass it.
- STAFF-5 · *(new, rev 21)* When the HR lead can't run, Hire's Describe field is disabled with the reason, and **Fill in the details yourself** opens Suggestion as a form: role, title, name and focus empty and marked Your choice needed, team, reports to and runtime at their defaults, and no HR's reason line. Next is disabled until every field has a value and says what's missing. A draft that stops, or a wake over 60 s, offers the same choice. The hire then goes through Review and approval like any other.
- STAFF-6 · *(new, rev 21)* When the office can't check whether the chosen runtime is installed and signed in on the instance, Review says so and Hire anyway still hires.
- STAFF-7 · *(new, rev 21)* Hire's Review leaves out rows for features that don't exist yet (autonomy, the role file, How it runs, the estimated cost) rather than showing them empty or estimated, and Done makes View profile primary, with no View session, until the office runs agent sessions.

**Assign and move (MOVE)**
- MOVE-1 · Move to team is one action: the agent never appears in Not in a team in between, and the toast offers Undo.
- MOVE-2 · The move dialog names the projects the agent stops and starts working on, and a lost lead.
- MOVE-3 · Add to project lists the project's teams to join, and with one team it's a single step.
- MOVE-4 · The assignee picker shows status and runtime, and assigning to a paused agent asks first.
- MOVE-5 · Make lead updates the team card and its projects' lead line at once.

**Hire approval (APPR)** *(new, rev 8)*
- APPR-1 · *(rev 17)* Only the owner can set or clear an agent's Can approve hires flag, at hire or in the agent's Settings, and the roster and quick view show "Approves hires". The same holds for Can approve processes (`processes-ux.md` decision 8), shown as "Approves processes".
- APPR-2 · *(rev 21)* A hire proposal goes to the requester's manager if that manager has the flag, else up the reporting line to the nearest manager with it, else to the owner. The proposal shows who it's waiting for. Once role files exist (`roles-ux.md`), a proposal that needs a new role goes straight to the owner ("Waiting for you: new role"); until then no proposal is classed as needing one (§7.1).
- APPR-3 · *(rev 9)* The requester never approves their own request (it goes one level up), except the owner, who approves their own hires since nobody is above the owner. The HR lead never approves.
- APPR-4 · *(rev 21)* An approval by a manager with the flag is enough: the office provisions the hire at once, with no human step, whether or not the HR lead can run. The exception, once role files exist, is a proposal that needs a new role, which only the owner approves (APPR-2).
- APPR-5 · Every approved hire records who requested and who approved it, on the new agent's profile and in Activity, and the owner sees it on Home.
- APPR-6 · The owner can approve or reject any proposal at any time, and a proposal waiting on an agent for over a day is shown to the owner.
- APPR-7 · Above the optional weekly or cost limit, a proposal goes to the owner and says why.
- APPR-8 · *(rev 21)* `org-agent hire request|approve|reject` work with the same words as the app, and approve and reject need a reason. When the HR lead can't run, `hire request` creates nothing and says "The HR lead can't run right now, so it can't draft hire requests. Ask the owner to use Hire or Add agent record."
- APPR-9 · *(new, rev 21)* A proposal whose agent approver can't run, is Paused or is at an offline office shows to the owner at once with the reason ("Waiting for ada · ada can't run right now, so you can approve it."), and the first answer decides. A proposal whose approver is offboarded moves to the next approver up the line and says so.
- APPR-10 · *(new, rev 21)* After approval a request shows Hiring…, then Hired. If hiring stops partway, it shows "Approved, not hired: <failed step>" with Try again, the owner gets a Needs you item, the steps already done are named, finishing needs no new approval, and the agent is shown in no chart, roster or picker until the hire completes.

**HR lead (HRL)** *(new, rev 7)*
- HRL-1 · Every organization has one built-in HR lead from its creation, with the reserved handle `hr`, and it can't be offboarded, deleted, renamed, put in a team, given reports or chosen as anyone's manager.
- HRL-2 · *(rev 10)* The HR lead appears only as the chip beside the owner bar, in the Agents roster's Built-in group, and in Chat, where it's listed in DMs and in @ completion as "HR lead" (it can be messaged). It never appears on team cards, in Teams, People or Reporting lines, or in member, lead and manager pickers.
- HRL-3 · *(rev 21)* Hire wakes the HR lead (on call by default) to draft the proposal when it can run; when it can't, the requester fills in the proposal (STAFF-5). Add agent record (formerly Add an existing agent) always builds its proposal from its fields (STAFF-4). Either way, nothing is provisioned before the proposal is approved by the owner or a manager with Can approve hires (§7.1). The creation endpoint checks the approval record, including for direct API callers.
- HRL-4 · *(rev 8)* An agent's request to the HR lead becomes a hire proposal routed to its approver (§7.1), never an automatic hire.
- HRL-5 · The owner can change the HR lead's runtime, model and run mode only, and its runs and cost show under "HR lead" in Insights.
- HRL-6 · *(new, rev 21)* When the HR lead can't run, its chip, Built-in group row and quick view say "Not running" (the server doesn't run it yet) or "Can't start" (its run failed to start) with "hires are filled in by hand", and the quick view gives the reason and, for a failed start, the fix and Run now. It never shows "Asleep · wakes when called" while it can't run.
- HRL-7 · *(new, rev 21)* Nothing waits on the HR lead: when it can't run, approved hires are provisioned and Offboard completes its cleanup as usual (APPR-4, DEAL-4).
- HRL-8 · *(new, rev 21)* When the HR lead can't run, a DM to it or an `@hr` mention says "HR lead can't run right now, so it won't answer yet. To hire, use Hire or Add agent record." The message is kept for when it can run (`chat-ux.md` §7).

**Deallocate (DEAL)**
- DEAL-1 · The UI never says "deallocate". It offers Remove from team, Pause, Offboard and Unassign, each with its own consequences.
- DEAL-2 · Removing an agent from its last team confirms and says it will be in Not in a team.
- DEAL-3 · HQ acceptance shows Paused with stop pending at office until the owning office confirms, including while disconnected. The office applies current Pause before new work; messages/tasks wait once applied, and a later one-click Resume supersedes stale stop delivery. Never display a confirmed stopped session from HQ acceptance alone.
- DEAL-4 · Offboard lists only the consequences that apply, brings open tasks back unassigned for the owner to reassign, makes anyone reporting to the agent report to the agent's own manager (or to the owner if it had none), and moves the agent to Former members, read-only (`member.offboard`, A37).
- DEAL-5 · Show paused in the chart toolbar hides or shows paused agents, and is remembered.

## 12. Gaps to close, and who I think owns them (for ada to confirm in the phase-1 plan)

| # | Gap | Proposed owner | Plan row |
| --- | --- | --- | --- |
| G1 | `Member` fields for agents: role, title, focus, runtime, model, home, messaging name, status incl. Paused, hired on | ada (contract) + server-dev (store) | W3 / W10 |
| G2 | `member.add_agent`, `member.move` (one command), `member.set_agent_status` (active or paused), `member.offboard` (A37): **landed on v1** | ada + server-dev | W3 |
| G3 | Retired: no old-team import or migration in standalone v1 | — | rev 18 |
| G4 | Messaging names ⇄ members: a message from `codex-dev` is linked to its member, and `who`/presence are keyed the same way | Remy (messaging owner) | W6 Phase B |
| G5 | Activity feed: one stream over messages, status, work items, team changes and git merges | ada (contract), server-dev, Remy | new |
| G6 | Hire on the office: the HR lead's proposal and provisioning as an office call (the HR toolkit's `hr.py` logic ported) | codex-dev (ada) | W12 |
| G7 | Browser screens: the agent record, Activity, Home, Hire and Add agent, Move, Pause and Offboard | app-frontend-dev (ada; Riley stays offline) | W9 / W12 |
| G8 | Authenticated browser session inspection and explicitly authorized local CLI takeover on the office machine | runtime owners + app-frontend-dev | standalone v1 |

## 13. Open questions

**For the user**
- ~~U10~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation:* **Agentic Consulting**, the workspace's name, distinct from the app. Use another name if you prefer.
- ~~U11~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation:* the shape in §2: Remy leads Agent tooling, and Riley shows Offline in App.
- ~~U12~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation:* yes. After the import, the app (and `org-agent`) is where we hire, move and offboard. HR's folders stay as a read-only record, and later imports only bring in what changed.
- ~~U13~~ · *Resolved (the user):* don't start Remy and Riley. They're brought in Active and show as **Offline** (no session), not Paused. Their planned work (G3, G6, G7) needs another owner (ada). *Update (later the same day): the user started Remy, who now owns messaging (AGENTS.md). Riley stays Offline.*

**For ada**
- A97 · *(rev 21)* When hiring an approved proposal stops partway (the record made, the home or messaging binding not), can HQ keep the partial member out of the chart, roster and pickers until it completes, and can Try again finish it without a new approval and without repeating what's done? Which states does a proposal expose (hiring, stopped at a named step)? APPR-10 and §7 Hiring need them.
- A98 · *(rev 21)* How does the app learn that an agent can't run, and why: the server doesn't run agents yet, its runtime is missing or signed out, its start failed, it's Paused, or its office is offline? HRL-6 and STAFF-5 need it for the HR lead, and APPR-9 for an agent approver. Can the office tell whether a runtime is installed and signed in on an instance? STAFF-6 covers when it can't.
- A96 · *(rev 20)* SRV-30 and STAFF-4 require HQ to refuse member creation without an authorized approval, including direct API callers, but `member.add_agent` (`crates/org-proto/src/org.rs`) has no approval reference and no approval error today. Which field carries the approval, and which error code does a call without one return? `dogfood-run.md` 3a.8 checks it.
- ~~A47~~ · *Resolved (ada):* `can_approve_hires` (owner-set only), `HireProposal` with routing up the reporting line, and `hire.request|approve|reject` (server-dev the contract, codex-dev the flow). In our team, ada has the flag.
- ~~A46~~ · *Resolved (ada):* server-dev adds a built-in `hr_lead` agent with the reserved handle `hr`, created with the organization; codex-dev builds the HR lead's runs. Note: `member.add_agent` is app-only today (`teams-and-org-chart.md`), so provisioning approved by an agent needs an exception for the HR lead's run (ada).
- ~~A37~~ · *Resolved (ada):* offboarding is a state. `AgentStatus` gains `offboarded`, and `member.offboard { member_id, expected_version }` sets it with `offboarded_at`. The record stays, the handle stays reserved, the agent shows under Former members, and its open tasks are reclaimed as in the teams design. `member.remove` stays for hard deletes of members with no history (mistakes). server-dev drafts it. ada updates `docs/design/teams-and-org-chart.md`, which still says offboarding issues `member.remove`.
- ~~A15~~ · *Resolved (ada):* `Member.agent: Option<AgentProfile>` (one entity, one version) with role, title, focus, runtime (claude_code | codex | antigravity | other), home, messaging_name, status (active | paused). Commands: `member.add_agent`, `member.update_agent`, `member.set_agent_status`, `member.move`.
- ~~A14~~ · *Partly resolved (ada):* codex-dev and server-dev run on Codex, agy-senior-dev on Antigravity. app-frontend-dev and the reviewer: ada is asking the user.
- ~~A16~~ · *Resolved:* `member.move { membership_id, to_team_id, title_in_team? }`, atomic. ~~A17~~ · *Resolved:* the app reads every message in the office (ADR-0003).
