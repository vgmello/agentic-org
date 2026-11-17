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

## The goal (the user, 2026-11-08)

> All of us (ada, Eden, the reviewer, codex-dev and the other devs) are registered as an organization in the application. From it you can see everything that's happening, add agents, hire more, deallocate agents, assign agents and change teams, with a great user experience.

**Success test (the dogfood run).** In the real HQ browser application, with no mock data:
1. The owner creates a fresh organization and provisions members through supported organization/hiring operations. Each agent appears under the right team and manager with its runtime; nothing is imported.
2. Home and Activity show, live, what the team is doing: who's working, the messages they send each other, tasks and bugs moving, and merges into the integration branch.
3. The owner hires a new agent, chooses its required home team and assigns project or team work independently, moves an agent to another team, pauses an idle agent and offboards one. Every change shows at once in the chart and Activity, and every agent keeps working on its own runtime.

The [dogfood run](dogfood-run.md) checks item 1 in its §1, §3 and §3a, item 2 in §3b, and item 3 in §3a.

## 1. What the product must respect

- An organization is **mixed-runtime**. The app shows and respects each agent's runtime and never assumes Claude Code.
- Some agents are started by a person outside the office. Such an agent is added through **Add agent record** and approved provisioning; the office never imports an external home or assumes launch authority for it.
- Employment, observed presence and readiness, and authority are distinct facts. A snapshot of one never proves another.

## 2. The shape of an organization

```text
Owner account (you)                         # authenticated principal, outside the team graph
Agentic Consulting
├─ Engineering                            lead: ada; ada is a direct employee
│  ├─ App                                  lead: app-frontend-dev; Riley
│  ├─ Office                               lead: server-dev; River
│  ├─ Agent tooling                        lead: Remy; codex-dev, agy-senior-dev
│  └─ Quality                              lead: reviewer
└─ Design                                 lead: Eden; Eden is a direct employee
Project P-8 Agentic Org: owner Engineering; contributor Design
Explicit reporting: ada and Eden have no employee manager recorded; other engineering employees → ada
HR lead: its home team is chosen during bootstrap
```

Sample names. Every employee appears at exactly one home team. Engineering's lead manages its subtree, including child leads; explicit reporting is a separate graph. An employee record for the owner needs a valid chosen home team too. The organization (**Agentic Consulting** here) is distinct from the product, **Agentic Org**.

## 2a. The HR lead, a built-in agent

The HR lead drafts hire proposals, without approving hires, managing work queues or doing provisioning cleanup. The owning office performs approved provisioning and offboarding cleanup; neither waits for an HR model response. HR is never teamless.

- **Bootstrap:** before admitting HR's employee, organization creation must supply or explicitly create a valid home team and persist its `home_team_id` and stable-ID profile path. The owner chooses the team through a supported bootstrap flow; its name, hierarchy and lead are not selected here. HR is an agent employee with the same required home team, optional role/reporting fields and separate execution configuration as other agents. The built-in handle `hr` is reserved, and HR cannot be offboarded or renamed; no field on the record grants these protections, the service enforces them.
- **Execution:** a runnable HR needs `execution.md`, an ordered model profile, a chosen canonical model, eligible provider connections, office-local credentials, a valid placement generation and fencing, and readiness. HR runs when asked, like any agent (explicit turns, `agent-run-modes-ux.md`).
- **Where it shows:** shortcut beside the owner bar, Built-in roster group and participant in Chat, plus one counted employee at its home-team card and recorded reporting position. Lead/manager pickers apply supported active/direct-membership policy; being a built-in neither removes a home team nor creates leadership.
- **When HR cannot run:** chip/row says **Not running · hires are filled in by hand** (server lacks execution) or **Can't start · hires are filled in by hand** (failed readiness/start). Quick view names why and any known fix: “Codex isn't signed in on <instance>.” **Run now** appears when it can work; otherwise the unavailable action is explained.
- **Nothing waits for HR:** Hire allows **Fill in the details yourself**; Add agent record always uses the submitted proposal. Approved provisioning/offboarding can finish independently. HR DMs and mentions keep the message with “HR lead can't run right now, so it won't answer yet. To hire, use Hire or Add agent record.”
- **Approval:** every non-bootstrap hire still needs an approved proposal under §7.1; drafting alone does not authorize creation. HR never approves.

## 3. Decisions

1. **Start with a fresh organization.** Setup and the authorized browser provision members through normal domain operations. Nothing is imported.
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
9. **Hire is the HR flow on the office.** The v1 §3.6 steps (Describe → Suggestion → Review → Hiring → Done) stay, and the review requires **Home team** and separates **Execution profile/model** and **Office placement**. The HR lead proposes (or, when it can't run, the requester fills in the proposal; Add agent record's fields are always its proposal, §7), and the approver (the owner, or a manager with Can approve hires, §7.1) sees exactly what will be created before anything is.

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

The colleague page `/members/:memberId` uses the same layout for people and agents. Its header shows name/role/handle, Agent or Person in words, one Home team and explicit Reports to; an agent has one truthful presence signal and one context line. Message is primary and Assign task outline when supported for that employee. More contains Pause/Resume, Edit, Offboard for agents and Edit for people, with their existing authorization and lifecycle constraints. An owner security account alone cannot be assigned employee work. A 40 px avatar and 16 px name/role use the shared typography rules.

Profile tabs are Overview, Work, Conversations, Activity; agents also have Notes and memory. Overview with no current task says Not working on anything right now. Names in the directory and Org chart open this profile. Agent Setup summary shows selected execution profile/model, observed driver/runtime, how it runs when supported, office and managed home with provenance/freshness, and Open in Agent setup. Paths wrap at narrow widths. Role, process and execution editing lives in Agent setup. The separately authorized Cost tab is additional to these colleague tabs.

`/settings/agents` shows configured runtime/model/profile, supported run mode, office and Health. Below 640 px render divided rows. `/settings/agents/:memberId` contains supported configuration, How it runs, Edit, Pause, Offboard and View profile, preserving the separate execution/employee revisions. Hire an agent opens the existing reviewed Home-team-required hiring flow. Health uses Ready, Needs setup: {reason}, Paused only from actual readiness/configuration facts; Needs setup takes precedence over Paused, while runtime readiness unavailable is explicitly unknown. Missing model uses the strict predicate in Inbox; lack of a selection alone is not missing model. Health is not presence and cannot grant execution.

All member surfaces share one presence vocabulary, in priority order: Needs you, With you, Working, Waiting for an answer, Paused, Available, Offline, Status unknown. Each signal has an icon and word, with a matching accessible name and tooltip in dense avatar-only views. Needs you requires an open owner-waiting item; Working, Waiting for an answer (an agent suspended on a required ask, [ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)), Available and With you require live runtime evidence. Before that evidence, show known Paused or Needs you and the independently known current-task line; omit unknown pills in lists, show Status unknown once in an agent's profile header. People have no speculative presence in this release. Paused does not claim who paused the agent. Runtime observation remains separate from employment, manual/security/budget holds and placement admission.

Quick views preserve name/title/focus, copyable `@handle`, role, Home team, explicit Reports to and any lead subtree responsibility, current task/last activity and separately sourced execution/office freshness. Supported actions include Message, Assign task, Move home team, Pause/Resume and Offboard with their existing authorization and reasons. View session opens the read-only session view; takeover is the office-local command (ADR-0004).

Notes and memory initially says “Not available yet: notes and memory arrive in a later release.” When built, its read API is for people only, read-only, confined to `journal/` and `memory/MEMORY.md`, text-only with entry and size caps, with no symlinks, and needs a security review. It never reads author-only task notes.

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
 05:41  [A] ada → [E] Eden · message · "Phase 1 push: your part + status please"   [Open thread]
 05:39  [C] codex-dev → [E] Eden · reply · "No W7 draft or design branch yet…"
 05:33  [A] ada → 7 members · message · "PHASE 1 PUSH (the user, today)…"
 05:30  [S] server-dev · merged server-dev/create-org into v1 · 3 commits      [View]
 05:12  [R] River · status Working → Waiting · "waiting for the reviewer"
 04:58  [E] Eden · bug T-A15 reported in Agentic Org · Major
        ── Earlier: 14 more · [Show]
```

- **Kinds**: All · Work · Conversations · Decisions · Runs · Org changes, with team, member and project filters. Work includes typed tasks and Git merges; Org changes includes team and member changes.
- **Authorized messages** are shown as one line with **Open thread**. Authenticated-owner read access follows the existing explicit chat policy; no employee field grants it. Threads open in a side panel with per-message authorization, read-only unless the owner is a participant allowed to reply. Shared activity never contains author-only private task notes, IDs or author associations. Broadcasts are one row ("ada → 7 members").
- **Grouping:** consecutive rows of the same kind from the same member collapse ("River changed status 4 times · Show"). Default filter: everything except Status changes, which are noisy (they live as dots on the cards), and Decisions, which are for auditing (shown with the Decisions filter, and never on Home).
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
| Hire, run, permission or office-join approval; scoped request | Approvals | Source approved/refused |
| Agent ask / task question | Questions | Tied answer or explicit withdrawal |
| Unaccepted hand-off addressed to the owner | Approvals, Accept | Owning hand-off resolved |
| Explicit review request | Approvals | Owning review or tied review ask resolved |
| Escalation or proposed decision with choices | Decisions | Authorized decision recorded |
| Work blocked on the owner / goal paused | Problems | Unblocked / resumed |
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

- **Describe:** “What do you need?” plus required **Home team**, prefilled from the launching team. Create/select a valid team before submission; HR does not infer an owner/HR team. Project ownership and work assignment are separate later choices.
- **Suggestion (the HR lead's proposal):** role, title (with level), name, focus, required **home team**, optional **reports to** (a suggestion must be confirmed), and **execution profile/model** (selected separately from identity, with observed driver and readiness). While the HR lead drafts, the screen says "HR lead is reading…". A start taking over 60 s shows "HR lead is still starting…" with **Details** and **Fill in the details yourself**.
- **When the HR lead can't run** (§2a): Describe's "What do you need?" field is disabled with the reason ("The HR lead can't run on this server yet." or "The HR lead can't start: Codex isn't signed in on <instance>."), and the primary action is **Fill in the details yourself**. It opens Suggestion as a form: name and handle are empty and marked **Your choice needed**; optional role, title and focus are available without being required, and home team (from Describe), optional explicit reports to and separately chosen execution settings are shown without fabricated defaults. There's no "HR's reason" line. **Next** stays disabled until required fields are valid and names what is missing ("Choose a home team and handle to continue"); optional role, focus and reporting can remain unset. If the HR lead's run stops while drafting, the screen says "The HR lead stopped: <reason>." with **Fill in the details yourself** and **Try again**, and what was typed in Describe stays above the form as a note. Review, approval, Hiring and Done are the same as when the HR lead drafts. Role association is optional and references an existing stable role ID; retained free text stays `role_text`. When drafting new roles is supported, a new-role request needs the HR drafting workflow: "New roles need the HR lead. Pick an existing role, or try again once it's running."
- **Review:** what will be created, as a dry run: the employee (name, handle `ac-qa-engineer-cameron`), required home team, optional explicit reports to, separate execution profile/model and readiness, office (its service machine or instance), home folder, and "Cameron can be reached by everyone in <org>". Rows for features that don't exist yet are left out, never shown empty or with a made-up value: the estimated cost per month (§7.1) until cost estimates exist, and How it runs until the office runs agents.
- **Hiring:** in words (Creating Cameron's record… Setting up the home folder… Registering with messaging…). Provisioning preserves running work. Any affected process change requires the safe lifecycle, permit/fencing and cleanup checks in document 11; do not automatically restart unrelated sessions. **Done:** “Cameron is hired and in App.” **View profile** is primary; Assign a task and View session appear when those supported operations are available. Hired does not mean runnable or started.
- **Hiring stops partway**: "Couldn't finish hiring Cameron: <step that failed>. Already done: <steps done>." with **Try again**, which finishes the hire without a new approval. Cameron isn't shown as a member (chart, roster, pickers) until the hire completes, and the request shows **Approved, not hired** (§7.1, A97).
- **Runtime not installed or signed out** (chosen in Suggestion): the Review warns "Codex isn't signed in on <office machine / instance>. Cameron is hired, but can't start until its administrator completes runtime setup there." **Hire anyway** works.
- **The office can't check the runtime** on that instance: the Review says "Can't check whether Codex is installed and signed in on <instance>. Cameron will be hired; check the runtime setup there before Cameron starts." **Hire anyway** works.
- **Add agent record** (the standalone-v1 label for **Add an existing agent**) submits
  the same proposal and approval workflow as Hire. Its fields are the
  proposal: the HR lead never drafts or changes it, even when it can run, so it
  works the same whether or not the HR lead can run. It captures name,
  handle, required home team, optional role/title/reports-to and separate execution/office choices as proposal data.
  Nothing is provisioned until an authorized manager with Can approve hires or the
  owner approves under §7.1. Only then does the office materialize the member, managed
  home/configuration and messaging binding. Direct HTTP/member-creation calls must
  enforce that approved proposal; authorization alone is not a bypass. Initial
  built-in HR/owner bootstrap during organization creation is the explicit exception to hire approval only; any employee created there still needs a valid chosen home team.
  No external home path or runtime takeover is accepted.
  Add agent record uses existing roles only:
  its optional role field lists stable-ID role records by name/key. Under it: "No role fits? Use Hire to request a new
  one.", where Hire links to Hire. Any supplied role_id must reference an existing role; leaving role_id unset is valid. It cannot submit an invented role_id that
  is not in the list, so it never needs a new role. A new role is created in Roles (`roles-ux.md`).

### 7.1 Who approves a hire

Approval authority never comes from a role, a team name or a field on a record; the service checks it. Team-lead subtree responsibility is distinct from this routing along explicit reporting lines.

**The flag.** **Can approve hires** is a flag on an **agent** (not on its role), set by the owner in the agent's Settings, at hire or later. It's typical for managers and senior managers. The roster and quick view show it as a badge, "Approves hires". Only the owner can set or clear it. An agent can't give it to itself or to the agents it hires.

**The route.** A hire request (from an agent or a person, through `@hr`, `agent-os hire request` or the app's Hire) becomes a **hire proposal**, drafted by the HR lead, or built from the fields the requester filled in (Add agent record, or Hire when the HR lead can't run, §7): optional role, title and reporting, required home team, separate execution profile/model and office choices, and the cost estimate once it exists (§7 Review). The proposal then goes to its **approver**:

1. The requester's **manager**, if that manager has Can approve hires.
2. Otherwise up the **reporting line** to the nearest manager with the flag.
3. Otherwise **the owner**.

The owner can always approve or reject any proposal. Someone who requests a hire never approves their own request: it goes one level up. The one exception is **the owner**, who approves their own hires, since nobody is above the owner. The HR lead never approves.

**An agent approver** gets the proposal in its inbox, with the same facts as the Hire review, and acts on it at its next run. It answers `agent-os hire approve <id> --reason "…"` or `hire reject <id> --reason "…"`. Approved means the office provisions the hire at once, with no human needed, whether or not the HR lead can run. The approval is recorded on the new agent ("Hired 8 Nov · requested by codex-dev · approved by ada") and in Activity. The owner sees it on Home ("ada approved hiring Cameron, QA engineer, for Office · ≈ $40/month"; until cost estimates exist, Home leaves out the cost, as the Review does (STAFF-7): "ada approved hiring Cameron, QA engineer, for Office"), with **Open** and, from the agent's profile, the usual **Offboard…** if they disagree.

**An approver who can't act**: when a proposal's agent approver can't run (the server doesn't run agents yet, or its runtime is missing or signed out), is Paused, or is at an offline office, the proposal shows to the owner at once, not after a day, with the reason: "Waiting for ada · ada can't run right now, so you can approve it." (or "ada is paused", "ada's office is offline"). It stays waiting for ada too, and the first answer decides; a later answer is refused with "Already approved by <name>." (or "Already rejected by <name>.") When the approver is offboarded, the proposal moves to the next approver up the line (steps 2–3) and says "Was waiting for Remy, who left. Now waiting for ada."

**Hire requests** (Agents › Requests tab): each proposal with its requester, approver, state (**Waiting for ada** · **Waiting for you** · **Approved by ada** · **Hiring…** · **Approved, not hired** · **Rejected** · **Hired**) and age. A request waiting on an agent approver for more than a day is also shown to the owner, who can approve it directly.
- **Empty:** "No hire requests. Hires and agent records sent for approval show here."
- **Approved, not hired**: "Approved, not hired: couldn't register with messaging" with **Try again** for the owner, and a Needs you item: "Hiring Cameron stopped: couldn't register with messaging. **Try again**". The row names what was already done. Try again finishes the hire without a new approval, and Cameron isn't shown as a member until it completes (A97).
- **Loading:** skeleton rows.
- **Error:** "Can't load hire requests" with **Retry**.

**Optional guardrail** (Settings › Hiring): "Hires approved by agents: no limit" (default), or "at most [n] per week", or "only up to ≈ $[amount] per month each". Above the limit the request goes to the owner, and the proposal says why ("Above ada's weekly limit, so it comes to you").

## 8. Flow 5 · Assign and change teams

- **Move home team…** (profile, quick view, team menu or drag shortcut) opens a required single destination and optional appointment title. Preview source/destination, any departed lead appointment, canonical path move and whether explicit workspace migration is required. “Riley moves from App to Office. Current project/task assignments stay as recorded.” One recoverable expected-revision operation; no teamless intermediate.
- **Make lead** accepts a direct home-team employee only. “app-frontend-dev now leads App and manages its subtree.” Resolve an invalid old-team appointment during a home move. Do not infer project co-leads or rewrite explicit reporting.
- **Project participation** edits owner/contributor teams through project settings, not employee memberships. Assigning a cross-team task does not require a home-team move or contributor registration and does not grant repository/tool permissions.
- **Assign task** shows accountable team, optional project, optional assigned team and optional individual separately. Show active employees' readiness/paused status; a paused assignee can be recorded with a warning, without claiming execution will resume. Replace the complete desired assignment pair, including nulls, with expected task revision; record immutable history. Team-only, individual-only, both and neither are valid. Reporter is the immutable creator, never the latest assignee.

## 9. Flow 6 · Deallocate: remove from team, pause, offboard

There is no **Remove from team**: **Move home team…** replaces it, and no action clears the employee's required home team. Pausing and offboarding have different lifecycle consequences.

- **Pause:** “Pause Remy? HQ will record the request. Remy stays in Agent tooling; new admissions stop when the authoritative control is applied.” Show **stop pending** until the owning office confirms actual process quiescence; an offline office supplies only last-observed state/freshness. Stop pending is not held. Manual, security and budget holds are independent; Resume clears only the authorized manual hold and must name any remaining block. Requests are revision-checked/deduplicated, and a later Resume must not let stale control delivery restart or stop the wrong placement generation. Existing UI Show paused can hide cards while control status remains inspectable.
- **Offboard…:** preview employment becoming inactive, retained home team/profile/handle/history, leadership replacement/clear, explicit reporting resolution and assignment changes. “Remy's profile and home-team history stay in organization data. The handle stays reserved. Choose how to resolve 1 current assignment.” Clear/reassign the individual with expected task revision and history, retaining an independently assigned team if selected; do not automatically change task accountability, reporter or status. Preserve task worktrees until closure or explicit safe migration. Former members shows retained identity/date, not a fabricated offboard timestamp.
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
- STAFF-1 · Hire Review shows name/handle, required home team, optional role/reporting/title/focus, separate execution profile/model, office/readiness and managed path before provisioning. Bootstrap employees also require a valid chosen team.
- STAFF-2 · A hired agent appears in its team in the chart, highlighted, and in Activity ("Cameron was hired into App").
- STAFF-3 · Choosing a runtime that isn't installed or signed in warns in Review, and Hire anyway still hires.
- STAFF-4 · Add agent record always builds its proposal from submitted fields, independent of HR readiness. Require a valid home team and handle; an optional role_id must reference an existing role, with role_text kept separately. An approved proposal is required before non-bootstrap provisioning, including for direct API callers; a missing approval check is never a bypass.
- STAFF-5 · When HR cannot run or drafting stalls, Fill in the details yourself preserves the description and opens a proposal form without invented HR reasoning/defaults. Require valid name/handle/home team and any enabled execution fields; optional role/focus/reporting may stay unset. Review and approval remain unchanged.
- STAFF-6 · When the office can't check whether the chosen runtime is installed and signed in on the instance, Review says so and Hire anyway still hires.
- STAFF-7 · Hire's Review leaves out rows for features that don't exist yet (How it runs, the estimated cost) rather than showing them empty or estimated, and Done makes View profile primary, with no View session, until the office runs agent sessions.
- STAFF-8 · Provisioning a new hire preserves running work. Only explicitly affected office processes may transition, through the safe turn, attempt and process lifecycle with valid permits, placement fencing and cleanup; unrelated sessions are not restarted.

**Assign and move (MOVE)**
- MOVE-1 · Move home team is one revision-checked recoverable operation updating home_team_id/canonical path, with no teamless intermediate.
- MOVE-2 · Move dialog identifies source/destination, departed lead appointment and explicit workspace migration needs; current task assignments, reporter, history and project participation remain unchanged.
- MOVE-3 · Working on a project does not require joining a team. Project owner/contributor participation and task assigned team/individual are separate actions; assignment grants no permissions.
- MOVE-4 · Assignment shows accountable team, project, independent nullable assigned team/individual and readiness. A paused assignee warns; the complete desired pair writes atomically with expected revision and immutable history.
- MOVE-5 · Make lead accepts only a direct home-team employee and shows management of its whole subtree. It does not infer project co-leads or rewrite explicit reporting.

**Hire approval (APPR)**
- APPR-1 · Only the owner can set or clear an agent's Can approve hires flag, at hire or in the agent's Settings, and the roster and quick view show "Approves hires".
- APPR-2 · A hire proposal goes to the requester's manager if that manager has the flag, else up the reporting line to the nearest manager with it, else to the owner. The proposal shows who it's waiting for.
- APPR-3 · The requester never approves their own request (it goes one level up), except the owner, who approves their own hires since nobody is above the owner. The HR lead never approves.
- APPR-4 · An approval by a manager with the flag is enough: the office provisions the hire at once, with no human step, whether or not the HR lead can run.
- APPR-5 · Every approved hire records who requested and who approved it, on the new agent's profile and in Activity, and the owner sees it on Home.
- APPR-6 · The owner can approve or reject any proposal at any time, and a proposal waiting on an agent for over a day is shown to the owner.
- APPR-7 · Above the optional weekly or cost limit, a proposal goes to the owner and says why.
- APPR-8 · `agent-os hire request|approve|reject` work with the same words as the app, and approve and reject need a reason. When the HR lead can't run, `hire request` creates nothing and says "The HR lead can't run right now, so it can't draft hire requests. Ask the owner to use Hire or Add agent record."
- APPR-9 · A proposal whose agent approver can't run, is Paused or is at an offline office shows to the owner at once with the reason ("Waiting for ada · ada can't run right now, so you can approve it."), and the first answer decides. A proposal whose approver is offboarded moves to the next approver up the line and says so.
- APPR-10 · After approval a request shows Hiring…, then Hired. If hiring stops partway, it shows "Approved, not hired: <failed step>" with Try again, the owner gets a Needs you item, the steps already done are named, finishing needs no new approval, and the agent is shown in no chart, roster or picker until the hire completes.

**HR lead (HRL)**
- HRL-1 · HR is an agent employee with one required valid home team selected through bootstrap, a stable handle and identity and separate execution settings. Its reserved handle and protection from offboarding and renaming are enforced by the service, never granted by a field.
- HRL-2 · HR appears as a shortcut beside the owner bar, Built-in roster row and Chat participant, and once at its actual home-team card/reporting position. It is counted as an employee; pickers apply supported active/direct-membership rules.
- HRL-3 · When HR can run, Hire asks HR to draft. Otherwise the requester fills in the proposal (STAFF-5). Add agent record (formerly Add an existing agent) always builds its proposal from its fields (STAFF-4). Either way, nothing is provisioned before the proposal is approved by the owner or a manager with Can approve hires (§7.1). The creation endpoint checks the approval record, including for direct API callers.
- HRL-4 · An agent's request to the HR lead becomes a hire proposal routed to its approver (§7.1), never an automatic hire.
- HRL-5 · HR uses separate execution profile/model and local provider/credential bindings. Its usage is attributed to its stable employee ID under HR lead.
- HRL-6 · When the HR lead can't run, its chip, Built-in group row and quick view say "Not running" (the server doesn't run it yet) or "Can't start" (its run failed to start) with "hires are filled in by hand", and the quick view gives the reason and, for a failed start, the fix and Run now.
- HRL-7 · Nothing waits on the HR lead: when it can't run, approved hires are provisioned and Offboard completes its cleanup as usual (APPR-4, DEAL-4).
- HRL-8 · When the HR lead can't run, a DM to it or an `@hr` mention says "HR lead can't run right now, so it won't answer yet. To hire, use Hire or Add agent record." The message is kept for when it can run (`chat-ux.md` §6).

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

- A97 · When hiring an approved proposal stops partway (the record made, the home or messaging binding not), HQ keeps the partial member out of the chart, roster and pickers until it completes, and Try again finishes it without a new approval and without repeating what's done. Which states a proposal exposes (hiring, stopped at a named step) is still to be designed. APPR-10 and §7 Hiring need them.
- A98 · How the app learns that an agent can't run, and why (its runtime is missing or signed out, its start failed, it's Paused, or its office is offline), and whether the office can tell that a runtime is installed and signed in on an instance, is still to be designed. HRL-6, STAFF-5, STAFF-6 and APPR-9 need it.
- A96 · SRV-30 and STAFF-4 require HQ to refuse employee creation without an authorized approval, including from direct API callers. Which field carries the approval, and which error code a call without one returns, is still to be designed. `dogfood-run.md` 3a.8 checks it.
