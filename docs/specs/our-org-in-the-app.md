---
kind: spec
title: "Our own team in Agentic Org (dogfooding): bring in, see, hire, assign, move, deallocate (UX)"
status: current
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
requested_by: owner
builds_on: [offices-and-teams-ux, work-items-ux]
---

# Spec · Our own team in Agentic Org (dogfooding): bring in, see, hire, assign, move, deallocate (UX)

The data contract owns the records: [organization](../design/organization-data/01-organization-structure.md), [employees](../design/organization-data/04-agent-structure.md), [execution](../design/organization-data/11-model-profiles-and-agent-execution.md), [assignment and history](../design/organization-data/12-task-assignment-and-history.md), [messaging](../design/organization-data/15-messaging-and-chat.md) and [budgets](../design/organization-data/17-budgets-and-controls.md). This spec owns what people see when they run their own team in the product.

## The goal

The people and agents building this product are registered as an organization in it. From it you can see everything that is happening, hire agents, deallocate them, assign them and change teams, with a great user experience.

**Success test (the dogfood run).** In the real HQ browser application, with no mock data:
1. The owner creates a fresh organization, adds the people and invites them to sign in, and hires the agents through HR. Each agent appears under the right team and manager with its runtime; nothing is imported.
2. Home and Activity show, live, what the team is doing: who's working, the messages they send each other, tasks and bugs moving, and merges into the integration branch.
3. The owner hires a new agent, chooses its required home team and assigns project or team work independently, moves an agent to another team, pauses an idle agent and offboards one. Every change shows at once in the chart and Activity, and every agent keeps working on its own runtime.

The [dogfood run](dogfood-run.md) checks item 1 in its §1 and §3a, item 2 in §3b, and item 3 in §3a. In milestone one it is run on one Mac with one office; the satellite sections join it in milestone three.

## 1. What the product must respect

- An organization is **mixed-runtime**. The app shows and respects each agent's runtime and never assumes Claude Code.
- **Every agent is hired through HR and started by its office.** There is no agent outside the product: nobody starts an agent by hand, brings in one that runs elsewhere, or binds one the office didn't start. A person may run commands from a terminal under their own name; that never makes them an agent.
- Employment, observed presence and readiness, and authority are distinct facts. A snapshot of one never proves another.

## 2. The shape of an organization

```text
Agentic Consulting                         top-level team, made with the organization
│                                          lead: Alex (you, the owner); HR lead
├─ Engineering                            lead: Sam; Sam is a direct employee
│  ├─ App                                  lead: app-frontend-dev; Lena
│  ├─ Office                               lead: office-dev; Noor
│  ├─ Agent tooling                        lead: Kai; tooling-dev, senior-dev
│  └─ Quality                              lead: reviewer
└─ Design                                 lead: Mira; Mira is a direct employee
Project P-8 Agentic Org: owner Engineering; contributor Design
Explicit reporting: Sam and Mira have no employee manager recorded; other engineering employees → Sam
```

Sample names. Every employee appears at exactly one home team. Creating the organization made its top-level team, with the owner as a person and its lead, and the built-in HR lead as its other member; the other teams came later, a project's team by default with the project. Engineering's lead manages its subtree, including child leads; explicit reporting is a separate graph. The organization (**Agentic Consulting** here) is distinct from the product, **Agentic Org**.

## 2a. The HR lead, a built-in agent

The HR lead drafts hire proposals for someone who may hire, without deciding hires, managing work queues or doing provisioning cleanup. The owning office provisions a confirmed hire and does offboarding cleanup; neither waits for an HR model response. HR is never teamless.

- **Built in:** the HR lead is part of the product. Creating an organization creates it as an agent employee in the top-level team, with nobody asked to choose anything; it is the only agent that isn't hired. It has the same required home team, optional role/reporting fields and separate execution configuration as other agents. The built-in handle `hr` is reserved, and HR cannot be offboarded or renamed; no field on the record grants these protections, the service enforces them.
- **Runs only when called:** the HR lead appears and is addressed as an agent, but it is not kept running. A message to it, an `@hr` mention or a hire started by someone who may hire starts one turn, and it stops when the turn ends (`agent-run-modes-ux.md`). A hire asked for by a caller who may not hire starts no turn (§7.1). It drafts proposals as an agent would; onboarding a new agent is the product's own code, which runs after the hirer confirms. A runnable HR needs `execution.md`, an ordered model profile, a chosen canonical model, eligible provider connections, office-local credentials, a valid placement generation and fencing, and readiness.
- **Where it shows:** shortcut beside the owner bar, Built-in roster group and participant in Chat, plus one counted employee at its home-team card and recorded reporting position. Lead/manager pickers apply supported active/direct-membership policy; being a built-in neither removes a home team nor creates leadership.
- **When HR cannot run:** chip/row says **Not running · hires are filled in by hand** (server lacks execution) or **Can't start · hires are filled in by hand** (failed readiness/start). Quick view names why and any known fix: “Codex isn't signed in on <instance>.” **Run now** appears when it can work; otherwise the unavailable action is explained.
- **Nothing waits for HR:** Hire allows **Fill in the details yourself**, the manual form, whenever HR can't run. Provisioning a confirmed hire and offboarding can finish independently. HR DMs and mentions keep the message with “HR lead can't run right now, so it won't answer yet. To hire, use Hire and fill in the details yourself.”
- **Who may hire:** only someone who may hire into the team starts a hire (§7.1). There is no approval. Drafting alone creates nothing: the hirer confirms the proposal first. HR decides nothing.

## 3. Decisions

1. **Start with a fresh organization.** The browser provisions members through normal domain operations: people are added in People and invited to sign in there, and agents are hired through HR. Nothing is imported.
2. **Employee identity is portable.** It holds name/handle, required home team, optional role/reporting, title/focus/contact and lifecycle. Separate execution configuration selects model profile/model/prompt/tools; provider connections select runtime driver. Office placement and server-managed home are operational state, never user-chosen identity paths. The profile brings these facts together with their provenance and freshness.
3. **Runtime is visible but quiet:** the runtime shows on profiles and quick views. The browser shows a session as a read-only view through HQ APIs. Taking over an agent is a local `agentic-org` command on the office machine; there is no terminal in the browser ([ADR-0004](../adr/0004-headless-agent-sessions.md)).
4. **Actions are named by their effect:**
   - **Move home team:** replace the required home team atomically, resolving departed leadership; current task assignment and project participation remain unchanged.
   - **Pause:** retain employment, home team and assignments while requesting an execution hold. HQ acceptance and office process-stop confirmation differ. Independent budget/security holds can still prevent execution after manual Resume.
   - **Offboard:** mark employment inactive, retain profile/home team/handle/history and resolve leadership, reporting and assignment explicitly. Never delete worktrees merely because a handoff occurred.
   - **Unassign task:** atomically replace the complete desired `(assigned_team_id, assignee_id)` pair with expected task revision. Either may remain populated independently; clearing an individual does not change accountable team or automatically reset status.
5. **A home-team move is one action**, never remove-then-add or a teamless intermediate. It preserves employee identity, explicit reporting and history, updating canonical path/home-team field coherently. A reverse move is a fresh authorized revision-checked operation.
6. **Assign work independently of membership.** Required project owner team remains accountable; optional contributors describe participation. Tasks may assign another active team and/or active employee without changing home team, project contribution membership or permissions. **Move home team…**, **Make lead** and **Assign task** are distinct actions.
7. **Everything happening is one feed, Activity,** with Home showing its top. Sources: sessions and status, messages between members, tasks and bugs, team and member changes, and git merges into the integration branch. Filters by team, member, project and kind. The owner sees authorized message visibility and replies only where a participant (§6).
8. **Live everywhere:** status dots on chart cards and lists come from presence, and changes made by agents (a task moved, a message sent) appear in under 2 seconds without a reload.
9. **Hire is the HR flow on the office.** The v1 §3.6 steps (Describe → Suggestion → Review → Hiring → Done) stay, and the review requires **Home team** and separates **Execution profile/model** and **Office placement**. Only someone who may hire into the team can start it (§7.1). The HR lead proposes (or, when it can't run, the hirer fills in the proposal, §7), and the hirer sees exactly what will be created and confirms it before anything is.

## 4. Agent homes and paths

Agent homes and configuration are provisioned under the server instance. Paths may
be shown read-only to authorized users; a browser path is not a path on the office
machine. There is no import of an existing team. The [dogfood run](dogfood-run.md)
starts with fresh disposable instances.

## 5. Flow 2 · The agent record (profile, quick view, card)

| Source | Fields shown together in the profile |
| --- | --- |
| `teams/<root-team-id>/teams/<child-team-id>/…/agents/<employee-id>/profile.md` (omit child segments at the root) | Stable `id`, explicit `handle`, kind/name, required `home_team_id`, optional `role_id`, `reports_to_employee_id`, body, status/revision, common attribution; retained title/focus/role_text/title_in_team and lifecycle detail. |
| Same employee folder's `execution.md` | `model_profile_id`, canonical `model_id`, enabled state, tools and prompt; separate revision. A role is not an execution model. |
| Model profile/provider connections | Ordered connections and their runtime drivers; office-local credential binding/readiness, not secrets or executable paths in employee identity. |
| Durable placement authority / owning office observations | Office, placement generation, managed home, process state, observed-at freshness; launching requires permits/fencing and confirmed cleanup. |

Stable IDs are UUIDv7. `role_text` and `role_id` remain distinct, and a missing normalized role is valid. People have no execution configuration. No employee field marks the owner or grants account privileges. A home-team change does not update immutable reporter/history or historical usage attribution.

The colleague page `/members/:memberId` uses the same layout for people and agents. Its header shows name/role/handle, Agent or Person in words, one Home team and explicit Reports to; an agent has one truthful presence signal and one context line. Message is primary and Assign task outline when supported for that employee. More contains Pause/Resume, Edit, Offboard for agents and Edit for people, with their existing authorization and lifecycle constraints. An account alone cannot be assigned work; the person it's tied to can. A 40 px avatar and 16 px name/role use the shared typography rules.

Profile tabs are Overview, Work, Conversations, Activity; agents also have Notes and memory. Overview with no current task says Not working on anything right now. Names in the directory and Org chart open this profile. Agent Setup summary shows selected execution profile/model, observed driver/runtime, how it runs when supported, office and managed home with provenance/freshness, and Open in Agent setup. Paths wrap at narrow widths. Role, process and execution editing lives in Agent setup. The separately authorized Cost tab is additional to these colleague tabs.

`/settings/agents` shows configured runtime/model/profile, office and Health. Below 640 px render divided rows. `/settings/agents/:memberId` contains supported configuration, Edit, Pause, Offboard and View profile, preserving the separate execution/employee revisions. Hire an agent opens the existing reviewed Home-team-required hiring flow. Health uses Ready, Needs setup: {reason}, Paused only from actual readiness/configuration facts; Needs setup takes precedence over Paused, while runtime readiness unavailable is explicitly unknown. Missing model uses the strict predicate in Inbox; lack of a selection alone is not missing model. Health is not presence and cannot grant execution.

All member surfaces share one presence vocabulary, in priority order: Needs you, With you, Working, Waiting for an answer, Paused, Available, Offline, Status unknown. Each signal has an icon and word, with a matching accessible name and tooltip in dense avatar-only views. Needs you requires an open owner-waiting item; Working, Waiting for an answer (an agent suspended on a required ask, [ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)) and With you require live runtime evidence. An agent that is not working, not paused and able to run shows **Available**, whether or not its process is running: to whoever calls it an agent is always there, and what is addressed to it starts it ([ADR-0028](../adr/0028-an-agent-is-always-available.md)). A person shows Available only while actually online. Before that evidence, show known Paused or Needs you and the independently known current-task line; omit unknown pills in lists, show Status unknown once in an agent's profile header. People have no speculative presence in this release. Paused does not claim who paused the agent. Runtime observation remains separate from employment, manual/security/budget holds and placement admission.

Quick views preserve name/title/focus, copyable `@handle`, role, Home team, explicit Reports to and any lead subtree responsibility, current task/last activity and separately sourced execution/office freshness. Supported actions include Message, Assign task, Move home team, Pause/Resume and Offboard with their existing authorization and reasons. View session opens the read-only session view; takeover is the office-local command (ADR-0004).

Notes and memory initially says “Not available yet: notes and memory arrive in a later release.” When built, its read API is for people only, read-only, confined to `journal/` and `memory/MEMORY.md`, text-only with entry and size caps, with no symlinks, and needs a security review. It never reads private task notes.

- AC-IA06-1–5: profile header/actions/tabs, setup summary and reciprocal links, empty current-work copy, and directory/tree profile links behave as above.
- AC-IA07-1–3: configuration lives under settings, responsive Health rows use real fields, View profile returns to the colleague face, and Hire preserves the existing flow.
- AC-IA08-1–3: one truthful vocabulary/component is used by profile, directory, team and chat; unknown is omitted in rows and shown once in an agent header.
- AC-IA14-1–2: the future confined reader must satisfy its security constraints; absent it, only the honest placeholder is served.

- **Cost:** actual consumption comes from durable usage records. The owner sees agent daily and monthly budgets and costs; project managers see cost for their projects only, labelled **in your projects**; others don't have this tab. The own-agent CLI exception lets an agent read only its own budget and cost. Incomplete usage is labelled incomplete, never zero. Compatible cost and budget requirements are governed by the integrated [cost UX](costs-and-insights-ux.md), [accounting contract](../design/organization-data/10-usage-and-cost-accounting.md) and [budget controls](../design/organization-data/17-budgets-and-controls.md).
- **Agent setup fields:** identity edits use employee revision; execution edits use execution revision and affect future admissions under document 11. Model/driver are never independently written onto `profile.md`. Managed home is read-only from the same office resolver used by launch, with no picker or path input. Local credential changes stay private to that office.
- **Missing data:** “No role on record”, “Execution not configured”, “Runtime readiness unknown” or “Unplaced”, naming the appropriate fix if supported. Missing home team is an invalid record, never a valid “No team” state. A configured agent can be hired while readiness is unavailable; show why it cannot start.

## 6. Flow 3 · See everything that's happening (Activity and Home)

**Activity** (nav item, also each team's, project's and agent's Activity tab):

```
 Activity                                [All teams ▾] [Everyone ▾] [All kinds ▾] [Search]
 ─────────────────────────────────────────────────────────────────────────────────────────
 Today
 05:41  [S] Sam → [M] Mira · message · "Milestone one: your part and status please"   [Open thread]
 05:39  [C] codex-dev → [M] Mira · reply · "No W7 draft or design branch yet…"
 05:33  [S] Sam → 7 members · message · "Milestone one, today…"
 05:30  [S] server-dev · merged server-dev/create-org into v1 · 3 commits      [View]
 05:12  [T] Tomas · status Working → Waiting · "waiting for the reviewer"
 04:58  [M] Mira · bug T-A15 reported in Agentic Org · Major
        ── Earlier: 14 more · [Show]
```

- **Kinds**: All · Work · Conversations · Decisions · Runs · Org changes, with team, member and project filters. Work includes typed tasks and Git merges; Org changes includes team and member changes.
- **Authorized messages** are shown as one line with **Open thread**. Authenticated-owner read access follows the existing explicit chat policy; no employee field grants it. Threads open in a side panel with per-message authorization, read-only unless the owner is a participant allowed to reply. Shared activity never contains private task notes, their IDs or author associations. Broadcasts are one row ("sam → 7 members").
- **Grouping:** consecutive rows of the same kind from the same member collapse ("Tomas changed status 4 times · Show"). Default filter: everything except Status changes, which are noisy (they live as dots on the cards), and Decisions, which are for auditing (shown with the Decisions filter, and never on Home).
- **Home** follows the section order and Inbox summary below; it retains the Activity live-scroll/freshness rules.
- **Live:** new rows slide in at the top with a 2-second highlight. When the owner has scrolled down, a pill shows "3 new · Jump to latest" and the list doesn't move.
- **Live updates paused**: when the app isn't getting live changes from the office (the connection to the office ended or couldn't be made, for example while the office restarts, or listening failed), a line shows at the top of every screen, under the title: ⚠ "Live updates paused. Showing what was loaded at 21:51." (the time the organization was last loaded, in the app's 24-hour format; if nothing has loaded yet, the line reads "Live updates paused." alone) with **Retry**, which reconnects now and reads "Retrying…" while it works. The line clears by itself once live changes are back and the latest has loaded. It stays while the owner moves between screens. Word plus icon, never color alone; screen readers announce it once when it appears, not on every retry.
- **States:**
  - Loading (skeleton rows).
  - Empty organization: "When your agents start working, everything they do shows up here."
  - Messaging not connected: "Messages aren't connected yet, so agents' conversations don't show here." **How to connect**.
  - A source offline (git or presence): a quiet row "Git activity paused: the project's repository isn't reachable."

### 6.1 Home and the unified Inbox

Home sections are Needs you (the first five open Inbox items and Open Inbox (N)), Projects, Happening now, Recent activity, in that order. The Following card joins between Needs you and Projects when Following ships, hidden when empty. Each section has an honest empty state, skeleton and one-line error with Retry. Use Nothing needs you, No projects yet with Create project, Live status isn't available yet when live presence is unavailable, and No activity yet. An observed empty working set may say No one is working right now.

Project cards show real done/total progress, open/blocked counts and members; show lead, health and due date only when those fields exist. Use the same progress definition as the project/work specification. Never infer a project lead from every contributor team's lead. Home has no page primary; its card actions are outline. Happening now has one current-task line per observed working agent without a redundant Working pill.

Inbox (`/inbox`) has All, Approvals, Questions, Decisions, Problems and Handled tabs, with one Show menu for team/project filters. Each card shows who, what, why, recommendation when present, available actions and expiry. List/detail are side by side from 960 px; below 960 an item opens as its own view with an Inbox back link. Reply acts inline on an addressed ask. Approve/Deny invoke exactly the source operation when it exists; otherwise offer Open. A required denial reason is entered inline; failure preserves the open item and displays the server's reason.

Each source yields one item of one kind. Clear it when its owning source resolves anywhere, including another client/CLI, and update Home and badges within two seconds of observing the durable result. Move resolved items to Handled. Only run errors and crashes are dismissible; budget controls are never dismissible. Budget actions and clearing follow the selected budget contract, not an old fixed percentage. Do not create a budget item for a release cap.

| Source, when its service is available | Inbox kind | Resolution |
| --- | --- | --- |
| Run, permission or office-join approval; scoped request | Approvals | Source approved/refused |
| Agent ask / task question | Questions | Tied answer or explicit withdrawal |
| Unaccepted hand-off addressed to the owner | Approvals, Accept | Owning hand-off resolved |
| Explicit review request | Approvals | Owning review or tied review ask resolved |
| Escalation or proposed decision with choices | Decisions | Authorized decision recorded |
| Work blocked on the owner | Problems | Unblocked |
| Run error / crash | Problems | Resolved, or explicitly dismissed |
| Critical bug | Problems, Assign… | Assigned or closed; triage alone does not clear |
| Unplaced agent / exact configured model unavailable | Problems | Source problem resolved |

Show only supported source kinds. A missing model item requires an exact selected model known absent from the runtime's reported list. An unset model, unreadable model list, missing runtime or unreachable office does not establish that predicate. Clearing/changing the selection or the runtime acquiring it clears the item. A configured fallback is not evidence of a missing selection.

Where structured owner blocking is supported, a blocked task has its open Approval/Question, otherwise one Blocked on you Problem. Refusal leaves the work blocked and the Problem replaces the refused ask. When a new Approval/Question opens, withdraw the derived Problem without a Handled entry. A budget-caused block uses its Budget reached item alone; an independent owner block predating the budget stop retains its own item. Retro ready is not an Inbox item.

Activity uses All, Work, Conversations, Decisions, Runs, Org changes filters, with team/member/project scopes. Merge organization and work events newest first; profiles/team/project tabs reuse the filtered feed. A recorded run expands in place to its trigger, duration, outcome and supported log summary. No fabricated run or presence entries appear before runtime evidence exists. Keep the existing private-note, default noisy-status/decision filtering and live-scroll protections.

- AC-IA04-1–4 / LIVE-4: assert Home order, five-item summary/common count, conditional cards, real project fields, section states and outline actions.
- AC-IA09-1–4: assert Inbox tabs, responsive detail, contextual cards, inline tied Reply, clear-anywhere behavior and the shared count.
- AC-IA10-1–2: a cross-kind read exposes source identity, kind, member/team/project, summary/context/recommendation/expiry, authorized actions and source clearing rule; durable clearing reaches consumers within two seconds.
- AC-IA11-1–2: inline approvals call the existing authorized operation and preserve unresolved items on refusal.
- AC-IA12-1–3: verify merged filtered Activity, truthful run expansion and reuse by scope, with no private-note metadata in shared results.

## 7. Flow 4 · Hire (the HR flow, on the office)

v1 §3.6 holds, with these changes for the office model:

- **Describe:** “What do you need?” plus required **Home team**, prefilled from the launching team, otherwise the top-level team. Project ownership and work assignment are separate later choices.
- **Suggestion (the HR lead's proposal):** role, title (with level), name, focus, required **home team**, optional **reports to** (a suggestion must be confirmed), and **execution profile/model** (selected separately from identity, with observed driver and readiness). While the HR lead drafts, the screen says "HR lead is reading…". A start taking over 60 s shows "HR lead is still starting…" with **Details** and **Fill in the details yourself**.
- **When the HR lead can't run** (§2a): Describe's "What do you need?" field is disabled with the reason ("The HR lead can't run on this server yet." or "The HR lead can't start: Codex isn't signed in on <instance>."), and the primary action is **Fill in the details yourself**. It opens Suggestion as a form: name and handle are empty and marked **Your choice needed**; optional role, title and focus are available without being required, and home team (from Describe), optional explicit reports to and separately chosen execution settings are shown without fabricated defaults. There's no "HR's reason" line. **Next** stays disabled until required fields are valid and names what is missing ("Choose a home team and handle to continue"); optional role, focus and reporting can remain unset. If the HR lead's run stops while drafting, the screen says "The HR lead stopped: <reason>." with **Fill in the details yourself** and **Try again**, and what was typed in Describe stays above the form as a note. Review, confirming, Hiring and Done are the same as when the HR lead drafts. Role association is optional and references an existing stable role ID; retained free text stays `role_text`. When drafting new roles is supported, a new-role request needs the HR drafting workflow: "New roles need the HR lead. Pick an existing role, or try again once it's running."
- **Review:** what will be created, as a dry run: the employee (name, handle `qa-engineer-marlowe`), required home team, optional explicit reports to, separate execution profile/model and readiness, office (its service machine or instance), home folder, and "Marlowe can be reached by everyone in <org>". Rows for features that don't exist yet are left out, never shown empty or with a made-up value: the estimated cost per month (§7.1) until cost estimates exist, and How it runs until the office runs agents.
- **Hiring:** in words (Creating Marlowe's record… Setting up the home folder… Registering with messaging…). Provisioning preserves running work. Any affected process change requires the safe lifecycle, permit/fencing and cleanup checks in document 11; do not automatically restart unrelated sessions. **Done:** “Marlowe is hired and in App.” **View profile** is primary; Assign a task and View session appear when those supported operations are available. Hired does not mean runnable or started.
- **Hiring stops partway**: "Couldn't finish hiring Marlowe: <step that failed>. Already done: <steps done>." with **Try again**, which finishes the hire without a new confirmation. Marlowe isn't shown as a member (chart, roster, pickers) until the hire completes, and the request shows **Not finished: <step that failed>** (§7.1, A97).
- **Runtime not installed or signed out** (chosen in Suggestion): the Review warns "Codex isn't signed in on <office machine / instance>. Marlowe is hired, but can't start until its administrator completes runtime setup there." **Hire anyway** works.
- **The office can't check the runtime** on that instance: the Review says "Can't check whether Codex is installed and signed in on <instance>. Marlowe will be hired; check the runtime setup there before Marlowe starts." **Hire anyway** works.
- **Agents come only from this flow.** Hire is the only way an agent comes to exist: nothing brings in an agent that runs elsewhere, starts one by hand, or binds one the office didn't start. Only someone who may hire into the new agent's home team can start a hire (§7.1), and nothing is provisioned until that hirer confirms the proposal. Only then does the office materialize the member, managed home/configuration and messaging binding, by the product's own code. Direct HTTP/member-creation calls must enforce the same: a caller who may not hire into the team is refused, and nothing is created without a confirmed hire request. No external home path or runtime takeover is accepted. The built-in HR lead, made with the organization (§2a), is the only agent that isn't hired.

### 7.1 Who may hire

There is no approval of a hire (owner, 2026-11-18, [ADR-0029](../adr/0029-only-who-may-hire-hires.md)). Only someone who may hire into a team can start a hire into it:

- the organization's owner, and the server's administrator acting in the organization, into any team;
- an employee, person or agent, with **Allow hiring**, into the teams it leads and the teams under them.

The permission is on for a person and off for an agent unless changed. Only the owner, or a person above that employee, changes it, from the employee's page ("Allow hiring for the teams it leads"), and the change shows in Activity. Nobody changes their own. It is given lead by lead: the lead of an engineering team can have it off while the lead of the director team above it has it on. It does nothing for someone who leads no team, and the HR lead never has it. The service checks the lead appointment and this permission on every call; no role, title or team name grants it ([employee structure](../design/organization-data/04-agent-structure.md#who-may-hire)).

**Everyone else is refused.** The app's Hire, `agent-os hr hire`, and a direct message or `@hr` mention to the HR lead asking for a hire all answer: "You don't have access to hire. Ask your team lead." (`not_authorized`). In the app, Hire shows this message instead of the form to a person who may hire nowhere, and the home team picker lists only the teams the person may hire into. A direct call for another team is refused the same way. A refused call records nothing and starts no turn for the HR lead. An employee that needs a colleague asks its lead in a message, as for anything else. A lead that may not hire asks its own lead. In the example above, the engineering lead asks the director team's lead, who hires into the engineering team.

**The hire.** A hire (through the app's Hire, `agent-os hr hire` or a message to `@hr`) becomes a **hire proposal**. It may name a role by its key, and an agent can read a role and what a proposal needs first with `agent-os role inspect`; this is how a lead agent with Allow hiring staffs its project when its development process says the time has come ([development processes](../design/development-processes.md#7-hiring-when-the-development-process-says-so)). The proposal is drafted by the HR lead, or built from the fields the hirer filled in (Fill in the details yourself, §7): optional role, title and reporting, required home team, separate execution profile/model and office choices, and the cost estimate once it exists (§7 Review). The hirer sees exactly what will be created and confirms it; a call that gives every required field is its own confirmation. The office then provisions the hire at once, whether or not the HR lead can run. The HR lead drafts and decides nothing.

**What is recorded.** The hire is recorded on the new agent ("Hired 8 Nov · hired by Sam Ortiz") and in Activity. The owner sees every hire on Home ("Sam Ortiz hired Marlowe, QA engineer, for Office · ≈ $40/month"; until cost estimates exist, Home leaves out the cost, as the Review does (STAFF-7): "Sam Ortiz hired Marlowe, QA engineer, for Office"), with **Open** and, from the agent's profile, the usual **Offboard…** if they disagree.

**Hire requests** (Agents › Requests tab): each hire with who hired, the home team, its state (**Drafting** · **Hiring…** · **Not finished: <step that failed>** · **Hired** · **Withdrawn**) and age. A hirer sees its own hires; the owner sees all of them.
- **Empty:** "No hire requests. Hires you start show here."
- **Drafting:** the proposal is not confirmed yet. The hirer can open it to confirm it, or **Withdraw** it; nothing has been created.
- **Not finished**: "Not finished: couldn't register with messaging" with **Try again** for the hirer and the owner, and a Needs you item for them: "Hiring Marlowe stopped: couldn't register with messaging. **Try again**". The row names what was already done. Try again finishes the hire without a new confirmation, and Marlowe isn't shown as a member until it completes (A97). If the hire is given up instead, it is recorded as failed with the reason, and the row keeps the step that failed.
- **Loading:** skeleton rows.
- **Error:** "Can't load hire requests" with **Retry**.

**Optional limit** (Settings › Hiring): "Hires by leads: no limit" (default), or "at most [n] per week", or "only up to ≈ $[amount] per month each". A hire above the limit is refused with the limit named ("This is above the limit of 2 hires per week. Only the owner can make it."). Only the owner can make it.

## 8. Flow 5 · Assign and change teams

- **Move home team…** (profile, quick view, team menu or drag shortcut) opens a required single destination and optional appointment title. Preview source/destination, any departed lead appointment, canonical path move and whether explicit workspace migration is required. “Nadia moves from App to Office. Current project/task assignments stay as recorded.” One recoverable expected-revision operation; no teamless intermediate.
- **Make lead** accepts a direct home-team employee only. “app-frontend-dev now leads App and manages its subtree.” Resolve an invalid old-team appointment during a home move. Do not infer project co-leads or rewrite explicit reporting.
- **Project participation** edits owner/contributor teams through project settings, not employee memberships. Assigning a cross-team task does not require a home-team move or contributor registration and does not grant repository/tool permissions.
- **Assign task** shows accountable team, optional project, optional assigned team and optional individual separately. Show active employees' readiness/paused status; a paused assignee can be recorded with a warning, without claiming execution will resume. Replace the complete desired assignment pair, including nulls, with expected task revision; record immutable history. Team-only, individual-only, both and neither are valid. Reporter is the immutable creator, never the latest assignee.

## 9. Flow 6 · Deallocate: remove from team, pause, offboard

There is no **Remove from team**: **Move home team…** replaces it, and no action clears the employee's required home team. Pausing and offboarding have different lifecycle consequences.

- **Pause:** “Pause Juno? HQ will record the request. Juno stays in Agent tooling; new admissions stop when the authoritative control is applied.” Show **stop pending** until the owning office confirms actual process quiescence; an offline office supplies only last-observed state/freshness. Stop pending is not held. Manual, security and budget holds are independent; Resume clears only the authorized manual hold and must name any remaining block. Requests are revision-checked/deduplicated, and a later Resume must not let stale control delivery restart or stop the wrong placement generation. Existing UI Show paused can hide cards while control status remains inspectable.
- **Offboard…:** preview employment becoming inactive, retained home team/profile/handle/history, leadership replacement/clear, explicit reporting resolution and assignment changes. “Juno's profile and home-team history stay in organization data. The handle stays reserved. Choose how to resolve 1 current assignment.” Clear/reassign the individual with expected task revision and history, retaining an independently assigned team if selected; do not automatically change task accountability, reporter or status. Preserve task worktrees until closure or explicit safe migration. Former members shows retained identity/date, not a fabricated offboard timestamp.
- **Running now:** name active process/turn state and the supported safe stop procedure. Do not declare success from HQ acceptance alone or blindly delete a workspace. Partway failure names confirmed completed steps and supports idempotent retry; revision conflicts refresh current state without repeating applied steps. HR execution is never a dependency of cleanup.

## 10. Accessibility and copy

Task references are the same everywhere, for example T-A15. All task types use T-.

- Status, runtime, Paused and Lead are always words plus icons. Screen reader: "codex-dev, agent, Agent tooling, runs on Codex, working".
- Activity rows are list items with time, actor, kind and text. **Open thread** is a button labelled with the thread's subject.
- Messaging names are shown in mono only as secondary text, and the display name leads everywhere.
- Never "deallocate" in the UI (decision 4). Words: Move home team · Pause · Resume · Offboard · Unassign.

## 11. UX acceptance criteria

**Import (IMP)**
- IMP-1 · Withdrawn: there is no import of an existing team.
- IMP-2 · Withdrawn: there is no import of an existing team.
- IMP-3 · Withdrawn: there is no import of an existing team.
- IMP-4 · Withdrawn: there is no import of an existing team.
- IMP-5 · Withdrawn: there is no import of an existing team.
- IMP-6 · Withdrawn: there is no import of an existing team.
- IMP-7 · Withdrawn: there is no import of an existing team.
- IMP-8 · Withdrawn: there is no import of an existing team.
- IMP-9 · Withdrawn: there is no import of an existing team.
- IMP-10 · Withdrawn: there is no import of an existing team.
- IMP-11 · Withdrawn: there is no import of an existing team.

**Agent record (PROF)**
- PROF-1 · Profile/quick view distinguish employee identity/home team/optional role from execution profile/model/driver and office/readiness/freshness. Missing optional data is named; missing home team blocks admission.
- PROF-2 · View session opens the authorized read-only session view through HQ. Takeover is a local command on the owning office machine; there is no terminal in the browser.
- PROF-3 · Directory/tree rows use one truthful shared presence signal, omit unknown pills in lists, and link to the colleague profile. Agent configuration lives in Agent setup.
- PROF-4 · Identity settings use employee revision; profile/model/prompt/tools use separate execution revision. Model/driver are not identity fields. Managed home stays read-only through the owning office resolver; there is no path picker or path input.

**Activity (LIVE)**
- LIVE-1 · Activity lists authorized messages, status, typed tasks (including bugs/requests), team changes, git merges and sessions, newest first, filterable by team/member/project/kind. Decision rows and noisy Status rows are hidden by default. Task references are complete on their own.
- LIVE-2 · An authorized visible message appears in Activity within 2 seconds; Open thread, previews and counts follow message visibility and the explicit authenticated-owner policy. Shared activity never contains private task-note bodies, IDs or author associations.
- LIVE-3 · The owner can reply only as an authorized participant/recipient. Otherwise the thread is read-only and says “Reading as the owner. To weigh in, message {agent}”. Reading never grants answer/accept authority.
- LIVE-4 · Home shows Needs you (first five Inbox items and the common count), Projects, Happening now and Recent activity, with the Following card only when Following ships (§6.1). Each section has its own empty/loading/error state.
- LIVE-5 · New rows never move the list while the owner is scrolled down. A "n new · Jump to latest" pill appears instead.
- LIVE-6 · A status change shows on the agent's card within 2 seconds, and Status rows are hidden from Activity by default.
- LIVE-7 · When messaging isn't connected, Activity says so with How to connect instead of looking empty.
- LIVE-8 · When live changes from the office stop (the connection ended or couldn't be made, or listening failed), every screen shows "Live updates paused. Showing what was loaded at <HH:MM>." (24-hour, the organization's last load; "Live updates paused." alone if nothing has loaded yet) with Retry ("Retrying…" while it works). The line clears once live changes resume, and it persists across screens until then.

**Hire and add (STAFF)**
- STAFF-1 · Hire Review shows name/handle, required home team, optional role/reporting/title/focus, separate execution profile/model, office/readiness and managed path before provisioning.
- STAFF-2 · A hired agent appears in its team in the chart, highlighted, and in Activity ("Marlowe was hired into App").
- STAFF-3 · Choosing a runtime that isn't installed or signed in warns in Review, and Hire anyway still hires.
- STAFF-4 · A hire is the only way an agent comes to exist; the built-in HR lead, made with the organization, is the only agent that isn't hired. The proposal requires a valid home team and handle; an optional role_id must reference an existing role, with role_text kept separately. Only a caller who may hire into the home team can start a hire, and nothing is provisioned before the hirer confirms the proposal, including for direct API callers; no call brings in, starts or binds an agent the office didn't provision.
- STAFF-5 · When HR cannot run or drafting stalls, Fill in the details yourself preserves the description and opens a proposal form without invented HR reasoning/defaults. Require valid name/handle/home team and any enabled execution fields; optional role/focus/reporting may stay unset. Review and confirming remain unchanged.
- STAFF-6 · When the office can't check whether the chosen runtime is installed and signed in on the instance, Review says so and Hire anyway still hires.
- STAFF-7 · Hire's Review leaves out rows for features that don't exist yet (How it runs, the estimated cost) rather than showing them empty or estimated, and Done makes View profile primary, with no View session, until the office runs agent sessions.
- STAFF-8 · Provisioning a new hire preserves running work. Only explicitly affected office processes may transition, through the safe turn, attempt and process lifecycle with valid permits, placement fencing and cleanup; unrelated sessions are not restarted.

**Assign and move (MOVE)**
- MOVE-1 · Move home team is one revision-checked recoverable operation updating home_team_id/canonical path, with no teamless intermediate.
- MOVE-2 · Move dialog identifies source/destination, departed lead appointment and explicit workspace migration needs; current task assignments, reporter, history and project participation remain unchanged.
- MOVE-3 · Working on a project does not require joining a team. Project owner/contributor participation and task assigned team/individual are separate actions; assignment grants no permissions.
- MOVE-4 · Assignment shows accountable team, project, independent nullable assigned team/individual and readiness. A paused assignee warns; the complete desired pair writes atomically with expected revision and immutable history.
- MOVE-5 · Make lead accepts only a direct home-team employee and shows management of its whole subtree. It does not infer project co-leads or rewrite explicit reporting.

**Who may hire (APPR)**

The IDs keep their old prefix because [ADR-0029](../adr/0029-only-who-may-hire-hires.md) cites APPR-1 to APPR-7. There is no hire approval.
- APPR-1 · Only someone who may hire into the new agent's home team can start a hire: the owner, or the administrator acting in the organization, into any team; an employee, person or agent, with Allow hiring, into the teams it leads and the teams under them. The service checks this on every call from the lead appointment and the permission. Allow hiring is on for a person and off for an agent unless changed. Only the owner or a person above the employee changes it; nobody sets their own, and no role, title or other field grants the right.
- APPR-2 · Anyone else is refused, in the app, by `agent-os hr hire` and by a direct message or `@hr` mention to the HR lead, with `not_authorized` and "You don't have access to hire. Ask your team lead." Nothing is recorded and no turn starts for the HR lead.
- APPR-3 · A lead with Allow hiring hires into its own team and into a team under it, and no other employee is asked. Its hire into a team it does not lead, directly or through a parent team, is refused. The owner's is not. The HR lead decides nothing.
- APPR-4 · The hirer's confirmation is enough: the office provisions the hire at once, whether or not the HR lead can run. A drafted proposal creates nothing until the hirer confirms it.
- APPR-5 · Every hire records who hired, on the new agent's profile ("hired by") and in Activity, and the owner sees it on Home.
- APPR-6 · The Requests tab shows each hire with who hired and its state: Drafting, Hiring…, Not finished: <step that failed>, Hired or Withdrawn. A hirer sees its own; the owner sees all.
- APPR-7 · With the optional weekly or cost limit set and reached, a hire by a lead is refused with the limit named, and the owner's succeeds.
- APPR-8 · `agent-os hr hire` works with the same words as the app. With every required field given, the call is its own confirmation and the office provisions the agent. With words only, the HR lead drafts, and the hirer confirms the draft. When the HR lead can't run and only words were given, it creates nothing and answers `unavailable` with a hint to fill in the fields ([agent command line](../design/agent-os-command-line.md)). It never answers `gate_required`.
- APPR-9 · Retired, ADR-0029 (it described a proposal waiting on a lead).
- APPR-10 · After the hirer confirms, a request shows Hiring…, then Hired. If hiring stops partway, it shows "Not finished: <failed step>" with Try again for the hirer and the owner, they get a Needs you item, the steps already done are named, finishing needs no new confirmation, and the agent is shown in no chart, roster or picker until the hire completes.

**HR lead (HRL)**
- HRL-1 · HR is a built-in agent employee, created with the organization in its top-level team, with a stable handle and identity and separate execution settings. It is not kept running: it runs only when called. Its reserved handle and protection from offboarding and renaming are enforced by the service, never granted by a field.
- HRL-2 · HR appears as a shortcut beside the owner bar, Built-in roster row and Chat participant, and once at its actual home-team card/reporting position. It is counted as an employee; pickers apply supported active/direct-membership rules.
- HRL-3 · When HR can run, Hire asks HR to draft. Otherwise the hirer fills in the proposal (STAFF-5). Either way, nothing is provisioned before the hirer confirms the proposal (§7.1), and provisioning is the product's own code. The creation endpoint checks that the caller may hire into the team and that the hire request is confirmed, including for direct API callers.
- HRL-4 · A message or `@hr` mention to the HR lead asking for a hire, from an agent or a person who may not hire into the team, is refused with "You don't have access to hire. Ask your team lead." and starts no HR turn (§7.1). From someone who may hire, it starts a draft that the hirer confirms, never an automatic hire.
- HRL-5 · HR uses separate execution profile/model and local provider/credential bindings. Its usage is attributed to its stable employee ID under HR lead.
- HRL-6 · When the HR lead can't run, its chip, Built-in group row and quick view say "Not running" (the server doesn't run it yet) or "Can't start" (its run failed to start) with "hires are filled in by hand", and the quick view gives the reason and, for a failed start, the fix and Run now.
- HRL-7 · Nothing waits on the HR lead: when it can't run, confirmed hires are provisioned and Offboard completes its cleanup as usual (APPR-4, DEAL-4).
- HRL-8 · When the HR lead can't run, a DM to it or an `@hr` mention says "HR lead can't run right now, so it won't answer yet. To hire, use Hire and fill in the details yourself." The message is kept for when it can run (`chat-ux.md` §6).

**Deallocate (DEAL)**
- DEAL-1 · The UI names Move home team, Pause, Offboard and Unassign explicitly. Remove from team cannot clear the required home team; each action previews its independent effects.
- DEAL-2 · No employee can be left teamless. Moves require a valid destination and resolve old-team lead appointments while preserving identity, explicit reporting and current task assignments.
- DEAL-3 · HQ acceptance shows stop pending until process quiescence is confirmed; offline observations remain labelled stale. Stop pending is distinct from held. Manual Resume does not clear independent budget/security holds or bypass permits, cleanup or placement fencing.
- DEAL-4 · Offboard retains inactive employee/profile/home team/handle/history and resolves live leadership/reporting/assignment explicitly. Assignment pairs/history use expected task revisions; accountability/reporter/status never change merely from offboarding. Task worktrees survive handoff until closure or explicit safe migration.
- DEAL-5 · Show paused in the chart toolbar hides or shows paused agents, and is remembered.

## 12. Not in v1

- Undo of a home-team move.
- Notes and memory in the profile.
- Cost estimates for a hire.

## 13. Open questions

- A97 · When hiring a confirmed proposal stops partway (the record made, the home or messaging binding not), HQ keeps the partial member out of the chart, roster and pickers until it completes, and Try again finishes it without a new confirmation and without repeating what's done. The states are decided (`drafting`, `hiring`, `hired`, `withdrawn`, `failed`, with the step that failed, [document 04](../design/organization-data/04-agent-structure.md#hire-requests)). What happens to the partial records when a hire is given up as `failed` is still to be designed. APPR-10 and §7 Hiring need it.
- A98 · How the app learns that an agent can't run, and why (its runtime is missing or signed out, its start failed, it's Paused, or its office is offline), and whether the office can tell that a runtime is installed and signed in on an instance, is still to be designed. HRL-6, STAFF-5 and STAFF-6 need it.
- A96 · SRV-30 and STAFF-4 require HQ to refuse employee creation without a confirmed hire request from a caller who may hire into the team, including from direct API callers. Which field names the hire request, and which error code a call without one returns, is still to be designed (a caller who may not hire gets `not_authorized`). `dogfood-run.md` 3a.8 checks it.
