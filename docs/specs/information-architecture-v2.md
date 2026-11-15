# Information architecture v2: organize the app like an organization

Emery (`ac-product-manager-emery`) · **Rev 4.1 (2026-11-12): decided.**
- §10 covers my decisions; §11 covers the decisions after Eden's review (`ac-product-designer-eden/knowledge/research/review-ia-v2.md`) and Rowan's technical read (`ac-fullstack-engineer-rowan/knowledge/designs/ia-v2-technical-read.md`).
- §12 (rev 4 and 4.1) covers the decisions on Eden's design system v2 to v2.2 and her spec-impact list.
- Later sections override earlier ones where they differ.
- **2026-11-12 (costs alignment):** the Organization group gains a Cost page, owner only (§4, D42). D26's Budget reached row is no longer dismissible and lists what clears it, and a task Blocked "Budget reached" gets only the Budget reached item, not Blocked on you (D26, `costs-and-budgets-v1.md` §12.2, C20; `tasks-v1.md` §6.13); D26's Budget reached row names a bug's Closed.
- **2026-11-12 late (staff review):** D26's Budget reached row adds the fifth clear cause, a person's unit change to a value above spent (`costs-and-budgets-v1.md` req 23).
- **2026-11-13 (routines, D44):** the team page and the project page gain a Routines tab, with a routine page and an occurrence page under them; D26 gains two non-dismissible Problem rows, "Routine {name} missed twice" and "Routine {name} needs a lead", and D9 their inline actions; New ▾ (D43) and D29 are unchanged (`ceremonies-and-retros-v1.md` Rev 17, D10, §8, §12). Decisions now run D1-D44.
- **2026-11-13 (platform, D45):** the owner's call via ada:
  - v1 has no desktop shell; it runs in the user's own browser against `org-gateway` on localhost.
  - Supported browsers: the last 2 stable Chrome or Chromium majors; Firefox's latest stable plus the current ESR; and the last 2 Safari majors on macOS.
  - The Tauri window is out of scope.
  - A Mac status-bar app for office servers is future work.

  Decisions now run D1-D45.
- **2026-11-13 (v1.0 scope):** ada's sizing decisions (Rowan's `v1-prd-sizing.md`), no new decision. The project Insights tab (D40) and its Flow tile ship in v1.1; D40's progress definition (its D15 amendment) applies from v1.0. Under D42, task and agent budgets and task, release-header and agent-profile cost are v1.0 (the release cap is v1.1); Insights' Cost section (CU7) and the Organization › Cost page (CU6) are v1.1 ("Project cost is CLI-only in v1.0"). Under D41, the Time zone preference (TZ-1) is v1.0. D40, D41 and D42 carry "(v1.0)"/"(v1.1)" markers in place.
- The stories are in [`ia-v2-stories.md`](ia-v2-stories.md). · 2026-11-11
Request: the owner, 2026-11-11 (in session). Prototype: [`../prototypes/ia-v2/index.html`](emery/knowledge/prototypes/ia-v2/index.html).
Labels: **[F]** fact · **[A]** assumption · **[O]** my proposal or opinion.

## 1. The problem

**[F] What the owner said** (2026-11-11):
- Today the app is organized around agents. It should be organized the way a company works: an organization, its offices, the projects it works on, and teams of people and agents.
- Projects, their tasks and their status are what everyone looks at most.
- Agents stay, but as a more advanced, configuration-like area, described in less technical terms.
- Offices are server configuration and belong under organization or settings.
- Lead with what's used most and what matters most, and with how humans step into the loop of autonomous agents.
- The look and feel is fine. Icons are welcome; **emoji are not allowed anywhere in the system**.

**[F] Today** (origin/v1 `2efce5d5`; frozen nav rev 4):
- **Navigation:** the sidebar has 10 items: Agents, Org chart, Chat, Projects, Work, Sessions, Activity, Offices, Processes, Settings. The frozen nav rev 4 cuts this to 7, with **Agents as the landing page**.
- **What needs me:** "Needs you" exists only inside Chat.
- **Placeholders:** Sessions, Activity and Settings show "Coming soon".
- **Agent identity:** the agent profile page was dropped, so there's nowhere to see an agent as a colleague: what they're working on, with whom, and what they did.

**[O] The result.** The first screen answers "what agents exist and how are they configured?" The questions the owner asks most come later:
- What needs me?
- How are my projects doing?
- Who's doing what right now?

## 2. Principles

1. **Organize like a company, not like a runtime.** Use the organization's own words: organization, projects, work, teams, people. Words for the machinery (sessions, runtimes, offices, processes) live in Organization settings.
2. **Urgent and frequent first.** The order is: what needs me, then what's happening, then projects and work, then people and structure, then configuration. Navigation follows that order.
3. **An agent has two faces.**
   - **Colleague:** name, role, team, what they're working on, conversations and activity. This lives with the team.
   - **Setup:** runtime, model, how it runs, office, home folder. This lives in Organization settings.
   - Each face links to the other.
4. **People and agents are members of the same teams, using the same patterns.** The kind ("Agent", "Person") is said in words. It's never only a shape or a colour.
5. **The human loop is one place.** Everything that waits on the owner (asks, approvals, decisions, blocked work, hire requests) appears in **Inbox**, with enough context to act there.
6. **One presence vocabulary everywhere**: roster, chat, team, profile (see §7).
7. **No emoji. Icons from one set, always with a text label or an accessible name** (see §8).

## 3. The model, as the owner sees it

```
Organization
├── Offices ............ where it runs (settings)
├── Projects ........... what we are working on
│   └── Work ........... tasks, bugs, requests, each owned by a member
├── Teams .............. how we are organized
│   └── Members ........ people and agents (manager, role, presence)
└── Conversations and activity ... how members work together (chat, hand-offs, runs, decisions)
```

## 4. Navigation

**Sidebar**, top to bottom:

| Group | Item | Answers | Contains |
| --- | --- | --- | --- |
| — | **Home** (landing) | "What needs me, and how are we doing?" | (Once AW-3 ships, a **While you were away** card leads, above Needs you's content, after an away of at least the threshold; `away-digest-and-timezones-v1.md`, D41.) **Needs you** (top 5, with a link to Inbox) · **Following** (once TK-1 ships, D38) · **Projects** at a glance (status, progress, at-risk) · **Happening now** (who's working on what) · **Recent activity** |
| — | **Inbox** (count badge) | "What's waiting on me?" | Asks, approvals, decisions, escalations, blocked-on-you work, hire requests. Act inline: approve, deny, reply, open. Filter by project, team or kind. |
| — | **Following** (neutral badge; D38, from TK-1) | "Where are the things I asked for?" | Tasks you asked for, created or follow, grouped by status, with the outcome when done. See [`tasks-v1.md`](tasks-v1.md). |
| Work | **Projects** (the 5 most recent listed under it) | "How is each project doing?" | Project list with status, progress, open work and team. **Project page:** Overview · Work · Team · Activity · Conversations (plus Backlog and Releases after Work, once PL-5/PL-6 ship, D39; plus Insights, once IN-1 ships (v1.1), D40; plus Routines, proposed after Team, once routines ship, D44) |
| Work | **Work** | "What's being done across the org?" | Tasks · Bugs · Requests. Views: Needs me, Mine, By project, By assignee, plus Created by me and Asked by me, and a Created by ▾ filter for any member (TK-2, D38). List or board. |
| Work | **Chat** | "Who's talking to whom?" | Channels, direct messages, threads. The owner can read agent-to-agent threads. |
| Work | **Activity** | "What happened?" | Org-wide feed: task changes, hand-offs, decisions, agent runs. A run opens its log, which replaces the Sessions screen. (Plus While you were away and Daily summary once AW-6/AW-7 ship, `away-digest-and-timezones-v1.md`, D41.) |
| Organization | **Teams** (each team listed under it) | "How are we organized?" | Team pages (tabs Work · Projects · Activity · Conversations, plus Routines, proposed after Projects, once routines ship, D44) · **People** (directory of humans and agents) · **Org chart** view |
| Organization | **Cost** (owner only; once CB-13 ships, D42) | "What does the org spend?" | Totals for a period by project and by agent, R&D only, with the org's Not on a task line; each figure opens its records. Hidden from everyone but the owner. See [`costs-and-budgets-v1.md`](costs-and-budgets-v1.md) requirement 31, CB-13 (*Emery's addition; the owner can cut it*). |
| Bottom | **Organization settings** | "How is it set up?" | General · **Offices** · **Agent setup** · Roles · Processes · Members and access · Costs (owner only, once costs req 29 ships, D42) · Updates |
| Footer | You · Owner · org switcher · office status line ("{Office name} office · {status}", e.g. "HQ office · Online"; host and machine only in Settings › Offices) | | |

**Top bar:**
- **Search** across everything: projects, work, members and conversations.
- **New** menu: Project, Work item, Request, Message, then a separator, Hire an agent, Add a person (D33, D43; AC-IA19-1).

**[O] Why this order:**
- **Home and Inbox are the human-in-the-loop entry points**, used several times a day.
- **Projects, Work, Chat and Activity** are where the owner follows and steers the work.
- **Teams** is visited less often and answers who does what.
- **Settings** is visited rarely.

## 5. Where today's screens go

| Today | v2 |
| --- | --- |
| Agents (landing, roster) | **Teams → People** for the colleague face, and **Organization settings → Agent setup** for the configuration face |
| Org chart | **Teams → Org chart** view |
| Chat (including its Needs you section) | **Chat**. Needs you moves to **Inbox** and the Home summary. |
| Projects, Project page | **Projects**, with Team, Activity and Conversations tabs added to the page |
| Work | **Work**, adding the Needs me and By assignee views |
| Sessions (placeholder) | **Activity**: each run is an activity entry with its log. The member profile shows that member's runs. |
| Activity (placeholder) | **Activity** |
| Offices | **Organization settings → Offices**, plus the status line in the sidebar footer |
| Processes (hidden in v1.0) | **Organization settings → Processes** (still hidden in v1.0) |
| Settings (placeholder) | **Organization settings** |
| Add an agent | **New → Hire an agent**, and **Add member** on a team page. Both use the existing Describe → Suggestion → Review → Hiring flow. |

## 6. Key pages

**Home**
- **Needs you** (amber, the only amber on the page).
- **Projects:** one card each, with status, progress, open and blocked work, and who's on it.
- **Happening now:** members who are working, each with the task they're working on.
- **Recent activity.**
- Every section has an honest empty state: "Nothing needs you", "No one is working right now".

**Inbox**
Each item shows:
- **Who** is asking.
- **What** they need, in one line.
- **Why:** the context, the project and task, and the agent's recommendation when there is one.
- **Actions:** Approve, Deny, Reply or Open.
- **When it expires**, if it does.

Grouping and filters:
- Group by Today and Earlier.
- Filter by kind, project and team.
- Done items move to "Handled".

**Project page**
- **Overview:** goal, status, progress, Needs you for this project, and recent activity.
- **Work:** list or board.
- **Backlog** and **Releases** (once PL-5/PL-6 ship, D39): the unplanned work, and the releases with their milestones. A release has its own page, `/projects/:projectId/releases/:releaseRef`.
- **Team:** the teams assigned to it (add and remove, D37), then the members working on it and what each is doing.
- **Activity** and **Conversations:** the project channel.
- **Routines** (once routines ship, D44; proposed after Team, pending Eden's E1): the project's routines, with a routine page and an occurrence page under the tab (`ceremonies-and-retros-v1.md` §8).

**Team page**
- **Header:** name, lead and purpose.
- **Members:** people and agents in one list. Each row shows avatar, name, role, kind in words, presence, and "Working on ...".
- **Tabs:** Work, Projects, Activity, Conversations. Plus **Routines** once routines ship (D44; proposed after Projects, pending Eden's E1): the team's routines, with a routine page and an occurrence page under the tab (`ceremonies-and-retros-v1.md` §8).
- **Add member:** Hire an agent, or Invite a person.

**Member profile** (a page; the same layout for people and agents)
- **Header:**
  - avatar, name and role
  - kind ("Agent" or "Person")
  - teams and manager
  - presence, with its current-task line
- **Actions:** Message · Assign task · Pause or Resume (agents only) · Edit.
- **Tabs:**
  - **Overview:** working on, goal, recent activity.
  - **Work.**
  - **Conversations.**
  - **Activity:** includes runs.
  - **Notes and memory:** agents only; read-only journal and memory, see Q6.
- **Setup** (agents only): runtime, model, how it runs, office and home folder, as a summary with "Open in Agent setup".

**Agent setup** (Organization settings)
- **List:** one row per agent, with runtime, model, how it runs, office and health.
- **Detail:** the setup fields, Pause, Offboard, and "View profile".

**Offices** (Organization settings): today's Offices screen, unchanged.

## 7. One presence vocabulary [O, for Eden]

Today Chat uses six presence states and the roster uses five different words. I propose one set, shown everywhere as icon + label + colour:

| Presence | Meaning |
| --- | --- |
| **Needs you** (amber) | waiting on the owner |
| **Working** | a run is live |
| **Available** | online, idle |
| **Asleep** | wakes when called or scheduled |
| **Paused** | |
| **Offline / Status unknown** | |

"Former" is a membership state, not a presence state.

## 8. Requirement: no emoji

- **E1.** No emoji characters in the UI: labels, buttons, statuses, empty states, notices, sample and mock data, or seeded content. Pseudo-icon symbols (check marks, stars, warning signs used as icons) are replaced by icons from the app's icon set.
- **E2.** Every icon comes from the one icon set and has a visible label or an accessible name. The set is **Heroicons**, as the design system already says (design-system.md:36). Today's 14 emoji are all in Chat, and Lucide is still used inside screens.
- **E3.** An automated check fails the UI suite if an emoji code point appears in `apps/org-ui/src`.
- **E4** (open, Q7): content that members write, such as agent chat messages.
- **Inventory:** every current occurrence, with its replacement, is in [`../research/emoji-inventory.md`](emery/knowledge/research/emoji-inventory.md).

## 9. Fit with v1.0 and what changes [O]

- **Data:**
  - Home, Inbox and Activity need the Needs-you and activity data, which arrives in M3 (F2-09, B1-13).
  - Until then they show honest empty states. The IA doesn't wait for the data.
- **Superseded:**
  - The **nav freeze rev 4** (Agents as landing; Org chart as a tab of Agents; `/settings` holding only Offices).
  - The BUG-70 slices built on it: S1 and S3, which haven't started, so pause them. S2 (the Agents | Org chart links) already merged at v1 `673e1cb7` (Eden validated it). It carries over as the Teams → People | Org chart switch: the same component with new labels and routes. The look slices (pills, tokens, icons, forms) are unaffected and still wanted.
- **Reversed:** this proposal reverses the v1.0 call "no profile pages" (bug-70 AD-1, Eden Q3). The member profile is where an agent becomes a colleague.
- **Next steps:**
  1. Agree this with the owner.
  2. Co-design screens and the presence vocabulary with Eden.
  3. Rowan works out the routes and data impacts.
  4. ada re-sequences the BUG-70 nav slices.

## 10. Decisions (Emery, 2026-11-11; the owner delegated them: "you should be able to make those decisions")

| # | Decision | Why |
| --- | --- | --- |
| D1 | **Home is the landing page.** Needs you comes first, then Projects. | The owner's first question is "what needs me", and Home answers it next to project status. A Projects-only landing would push the human loop below the fold. |
| D2 | **Inbox is its own sidebar item, with a count.** Home shows its top 5. | It's the most frequent human-in-the-loop action. It needs a full queue with filters and a Handled history, which a Home section can't hold. |
| D3 | **Naming:** "Teams", with tabs Teams · **People** · Org chart. Inside People the filter reads All · Humans · Agents. Keep **"Work"** (it holds tasks, bugs and requests, not only tasks). Keep **"Organization settings"**. | Plain company words. "People" is how a company names its staff, and agents count as staff. |
| D4 | **Humans are members of teams in the IA from day one.** If v1.0 data holds only the owner, teams show agents plus "You". | The plan models employees as humans and agents. Designing for both avoids a later restructure. Person sign-in stays out of v1.0 (A-7), and that doesn't change this. |
| D5 | **Agent setup lives in Organization settings**, not in the sidebar. | The owner asked for agents' technical side to be "more advanced configuration". The colleague face stays one click away, in Teams. |
| D6 | **Notes and memory: a read-only tab on an agent's profile in v1.0** (latest journal entries and memory). | Trust in delegated work depends on seeing what the colleague knows and did. This is the cheapest high-value signal. Rowan to confirm the data is reachable. If it isn't, the tab ships with an honest "not available yet" state, and the IA doesn't change. |
| D7 | **No emoji from agents.** Agent instructions forbid them, and the app renders agent-authored text with emoji removed. Text a human types is shown as typed. | The owner: no emoticons in the system. Agents write almost all the text, so both a rule and a guard are needed. Rewriting a person's own words would be surprising. |
| D8 | **Prototype choices adopted** (for Eden to fold into the design system):<br>- Paused is gray; amber means only "Needs you".<br>- "At risk" is red.<br>- Home and the Teams index use outlined cards.<br>- Team page: Members above the tabs.<br>- The org chart is a vertical tree.<br>- Chat has a "Between agents" group.<br>- The sidebar pins up to 5 active projects; the prototype has 3. | Each keeps amber scarce, or keeps the structure readable at a glance. |

## 11. Decisions after Eden's review and Rowan's read (rev 3)

I accept **all** of Eden's page fixes (H1-H5, I1-I4, T1-T4, M1-M5, sidebar, top bar, targets, type) and her presence rules. Her D8 positions replace D8.

| # | Decision | Why |
| --- | --- | --- |
| D9 (Q1) | **Inline actions call the same operation as the source screen. Where none exists, the card offers Open.**<br>- Approvals (hire approval, run approvals, permission requests): Approve or Deny inline, once their operations exist (hire approval M2; run approvals B1-12, M3).<br>- Questions: Reply inline (messaging exists).<br>- Decisions: choose an option, or Open.<br>- Problems: Open, plus Resume for a paused goal.<br>- Routine Problems (D44, `ceremonies-and-retros-v1.md` §8): "Routine {name} missed twice": Open · Run now · Pause (Edit in place of Run now when it needs a lead; Run now is hidden while an occurrence is open). "Routine {name} needs a lead": Open · Edit · Pause.<br>- On Home cards, buttons are outline only. | The Inbox is where the owner steps into the loop, and making them open another screen to say yes doubles the cost of every approval. "No answering from the inbox" was a Tauri-era rule, made for terminal permission prompts. No new backend logic: the card calls the existing operation. This overrides that v1 spec line. |
| D10 (Q2) | **"Members"** is the mixed directory: Teams · **Members** · Org chart, with the filter All · People · Agents. "People" means humans everywhere. Supersedes D3's "People". | One word per concept. It's the owner's own word ("team members") and this model's word (§3). |
| D11 (Q3) | **Inbox kinds** (tabs All · Approvals · Questions · Decisions · Problems · Handled):<br>- **Approvals:** hire requests, run-mode approvals, permission requests (v1 "Permission"). They clear only when acted on.<br>- **Questions:** agent asks (v1 "Question"). They clear when answered.<br>- **Decisions:** escalations with options and a recommendation. They clear when one is chosen.<br>- **Problems:** blocked-on-you work, goal paused, run error, crash (v1 "Goal paused", "Error", "Crash"). They clear when resolved; error and crash can also be **Dismissed**.<br>Nothing else is dismissable. | Every v1 waiting kind maps to exactly one tab, and each kind has one clearing rule an engineer can test. |
| D12 (Q4) | **Presence is truthful in v1.0.**<br>- Use Eden's set and precedence, including **With you**.<br>- Shown in v1.0: **Paused** (member status), **Needs you** (derived from the member's open Inbox items), "Working on {task}" (B1-11 C2a) and **Status unknown** otherwise.<br>- Working, Available, Asleep and With you appear only once the office reports runs.<br>- **I'm asking ada to schedule runtime presence reporting in v1.0** (P1): without it, agents never look alive.<br>- Humans other than the owner have no presence in v1.0. | §1.6 truth over decoration. Presence is the core of the colleague model, so its data is a scope ask, not a design workaround. |
| D13 (Q5) | **Agent-to-agent threads stay read-only** (LIVE-3), with the note "Reading as the owner. To weigh in, message {agent}". | When the owner steps into a thread between two agents, it muddies whose decision it is. Messaging an agent directly keeps that clear. |
| D14 (Q6) | **"Hire"** everywhere: Hire an agent, Hire / Offboard. "Invite a person" for humans. | It matches the company model and the existing hire flow. |
| D15 (Q7) | **Project progress = done / total work items** (definition per `project-insights-v1.md` IN-2, D40). Due date and health appear only when set on the project; otherwise the card shows the open and blocked counts. | Real data only (§1.6). A health label with no source would be decoration. |
| D16 (Q8) | **Search** in the top bar opens grouped results: Projects, Work, Members (and Conversations once indexed). Enter on a result navigates; "See all results" opens `/search?q=`. | The owner thinks in projects and work, not agents. Today's search finds only agents. |
| D17 | **The owner is not a team member.**<br>- Replaces D4's "teams show agents plus You". The org rule stays (`structure.rs:144`), so there's no one-way door.<br>- The owner appears once: the owner card on the Teams index and Org chart, "Reports to You" on members, and the sidebar footer.<br>- Humans other than the owner can be team members (D4 otherwise stands). | Following Rowan's read. A "You" row would claim a membership the data doesn't have. |
| D18 | **Notes and memory (D6), sequenced.**<br>- In v1.0 the tab ships with an honest "Not available yet".<br>- The read API is a Tier A slice with a security review (read-only, human principal only, confined to `journal/` and `memory/MEMORY.md`, no symlinks, capped, versioned).<br>- ada places it after the core IA slices. | Rowan: there's no API, and the files may hold sensitive text. The IA doesn't wait for it. |
| D19 | **Technical conventions from Rowan, adopted as requirements:**<br>- URLs key on IDs, never names.<br>- Old URLs redirect indefinitely: `/agents`→`/teams/members`, `/org-chart`→`/teams/org-chart` (keeping `?member=`), `/offices`→`/settings/offices`, `/processes`→`/settings/processes` (still "Not in this release" in production), `/sessions`→`/activity`, `/`→`/home`.<br>- Presence is a UI mapping of the existing wire enum.<br>- Emoji are stripped at render only (D7), never in storage.<br>- Rowan writes **nav freeze rev 5**, and Jules's S1 is re-scoped to it. | Reversible routes; no protocol change; no data loss. |

## 12. Decisions on design-system v2 (rev 4, 2026-11-11)

These answer Eden's open items in design-system v2 (her commit `e51b184`) and the conflicts in her `ia-v2-spec-impact.md` §8.

### Open items in design-system v2

| # | Decision |
| --- | --- |
| D20 | **Agent setup has a Health column, not presence.**<br>- **Ready.**<br>- **Needs setup: {reason}.** The reasons are missing model, unplaced, and runtime missing. Missing model follows the design system's MOD-7 with all its exclusions (no model set, the model list can't be read, runtime missing, office unreachable); this supersedes the earlier "no model" reason (design system v2.3).<br>- **Paused.**<br>Each shows an icon and a word, from data v1.0 already holds. If a reason has no data source, it isn't shown. When an agent is both paused and needs setup, it shows Needs setup. |
| D21 | **The owner has no member profile and no `/members/you`.**<br>- "You · Owner" in the footer opens an account menu: Your preferences, Switch organization, About, Sign out. Items whose flow doesn't exist yet are left out, as with the New ▾ menu. Sign out appears only once the gateway has owner sign-in.<br>- This follows D17: the owner isn't a member. |
| D22 | **Project cards show "Lead {name}" when the project has a lead.** Otherwise they show nothing in its place. |
| D23 | **No amber "Scoped" pill.** A request's state uses a neutral or indigo pill. A scoped request that waits on the owner reaches the Inbox as an Approval (see D27), and shows Needs you there. |
| D24 | **Activity gets an "Org changes" filter:** All · Work · Conversations · Decisions · Runs · Org changes. |
| D25 | **People can be assigned work.** Every profile has Message (primary) and Assign task (outline). The More menu holds Pause/Resume, Edit and Offboard for agents, and Edit for people. AC-IA06-1 is amended to match. |

### Conflicts in ia-v2-spec-impact §8

| # | Decision |
| --- | --- |
| D26 (§8.1, §8.2) | **Every Needs-you source maps to one Inbox kind.** The table below gives the mapping. Two rules apply to all kinds:<br>- **Clearing:** an item clears when it's resolved **anywhere**: answered in a terminal, in another client, or because the agent carried on. The point of "cleared only by acting" in D11 is that these items can't be dismissed, not that they must be answered in the Inbox. AC-IA10-2 already assumes this.<br>- **Dismissing:** only run error and crash items can be dismissed. Budget reached items can't: they hold work stopped, and dismissing would leave it stopped with nobody deciding (`costs-and-budgets-v1.md` C20, D42). Neither can the routine Problems, missed twice and needs a lead: each clears on its own (D44). Home and the Inbox follow the same rule (HOME-4).<br>- **One item per budget stop:** a task Blocked "Budget reached" (or on a budget hold) gets only its Budget reached item, never a "Blocked on you" item for the budget stop (`tasks-v1.md` §6.13, AC-TK4-3). A task that was already Blocked on the owner before its budget was reached keeps that block and its "Blocked on you" item and also gets the Budget reached item (`tasks-v1.md` AC-TK4-19, costs req 19): two separate asks. |
| D27 (§8.3) | **D13 stands: agent-to-agent threads are read-only.** The note copy is "Reading as the owner. To weigh in, message {agent}." ada places it in a Chat slice (A-CH2). |
| D28 (§8.4) | **Profile tabs**, following Eden's proposal:<br>- Tasks becomes Work. Messages becomes Conversations. Sessions becomes Activity, filtered to runs.<br>- Role and Processes move to the agent's detail page in Agent setup.<br>- Knowledge comes later (Wave B), in Notes and memory. |
| D29 (§8.5) | **Organization settings hold org-wide settings only.**<br>- **General:** name, workspace, and the Claude Code location.<br>- **Offices.**<br>- **Agent setup:** includes Models and Agent guidance; Autonomy comes later.<br>- **Roles.**<br>- **Processes:** not in this release.<br>- **Members and access:** includes hiring approvals; Import comes later.<br>- **Costs:** the price list (owner only), once costs-and-budgets-v1 req 29 ships (D42).<br>- **Updates:** "Not in this release". SET-3 (restart to update) isn't in v1.0, because it needs signed builds and a distribution channel (ada, 2026-11-11).<br>Bridges come later.<br>Personal preferences (Appearance, Notifications) and About go in the footer account menu (D21), not in Organization settings. |
| D30 (§8.6) | **Project health in v1.0 is set by hand.** D15 stands. When project status (PS, Wave B) ships, PS rules compute health and the owner can override it. The STATUS-9 table comes later. |
| D31 (§8.7) | **New stories for pages that had none:** IA-17 the project page tabs, IA-18 the Work views, IA-19 the New ▾ menu (see the stories file). Insights comes later, with Costs and insights in Wave B. Knowledge comes later (D28). |
| D32 (§8.8) | **Product requirement:** the team hierarchy and team editing (New team, Move team, Change manager) must survive IA v2. The Teams index shows sub-teams as indented rows. The org chart shows reporting lines. Eden and Rowan own the design. |
| D33 (§8.10) | **"Add a person" in v1.0, not "Invite a person".** Person sign-in is out of v1.0, and an invite implies an account. It becomes "Invite" when sign-in ships. Supersedes part of D14. |
| D34 (§8.11) | IA-15 is in v1.0, after C2a (ada). Until it lands, checks that expect live presence words (DF 2.3, LIVE-6, RUN-2, ORG-7) are marked "deferred until IA-15". ada keeps that list. |
| D35 (§8.12) | **The "Recent" projects in the sidebar need each project's last-activity time** (Rowan). Until that exists, the sidebar shows the first 5 projects A–Z, headed "Projects", with "View all". |
| D36 (backlog triage, 2026-11-12) | **The org chart is one view.** IA-02's vertical tree replaces all three of today's views (Teams, People and Reporting lines), along with their View menu, Fit and chart search. Teams live in the Teams index (D32) and people in Members. Three drawings of one org is the technical surface the owner asked us to retire. |
| D37 (backlog triage, 2026-11-12) | **Projects keep team assignment.** The project's Team tab lists the assigned teams first (add and remove, as today), then the members. Projects are staffed by teams, and IA v2 doesn't drop shipped capability. |
| D38 (Emery, from the owner's request, 2026-11-12) | **Following joins the sidebar directly under Inbox, and Home gains a Following card between Needs you and Projects.** Following lists the tasks the owner asked for, created or follows. Its badge is neutral (updated since you last looked), never amber. The Inbox stays "needs you now". Details, statuses, review and private notes are in [`tasks-v1.md`](tasks-v1.md). It amends §4, the sidebar order (AC-IA01-1), the Home order (D1, §6, AC-IA04-1), Work's views (AC-IA18-1) and nav freeze rev 5 (Rowan to revise). ada sequences it. When TK-5 ships, Review requested comes from the task's In review state, superseding D26's "a real review state comes after v1.0". D26's "blocked work always has an open Inbox item" holds for tasks: the work "Blocked on you" row and a task hand-off row ship with task statuses and reassignment (tasks-v1 §6.13). |
| D39 · Project Backlog and Releases tabs (`projects-planning-v1.md`, 2026-11-12) | **The project page gains Backlog and Releases tabs next to Work, plus a release page (`/projects/:projectId/releases/:releaseRef`).** Backlog lists the project's unplanned tasks and bugs; Releases lists releases, each with its milestone count and progress; the milestones themselves show on the release page. Details, keys and stories are in [`projects-planning-v1.md`](projects-planning-v1.md) (PL-5, PL-6). It amends IA-17's project page tab list (`ia-v2-stories.md:156`, `Overview, Work, Team, Activity, Conversations`) and §4 (line 66, the project page tabs). Applies once PL-5/PL-6 ship; until then the tabs don't show. |
| D40 · Project Insights tab (`project-insights-v1.md`, 2026-11-12) | **The project page gains an Insights tab once IN-1 ships (v1.1).** It supersedes, for Insights only, AC-IA17-3's "Insights ... aren't shown in v1.0" and D31's "Insights comes later, with Costs and insights in Wave B". Its Cost section (CU7) arrives with the tab in v1.1; costs capture and budgets themselves are v1.0 (`costs-and-budgets-v1.md`). Insights is the last tab, after Conversations. Details, figures and stories are in [`project-insights-v1.md`](project-insights-v1.md) (Insights v1: IN-1..IN-8, IN-10, IN-13). It amends IA-17's project page tab list (`ia-v2-stories.md:156`) and §4 (line 66, the project page tabs). Until IN-1 ships, the tab doesn't show. **It amends D15's progress definition (v1.0; the tab itself is v1.1):** the project header's progress (AC-IA17-1) uses IN-2's definition (top-level tasks and bugs, Cancelled excluded, backlog included), so the header and the Insights project % are always equal (AC-IN2-4). **It amends AC-IA17-2's Overview (v1.1):** a minimal Flow tile (items done in the last 7 days, their median cycle time, the current release %, and the current release's open milestones with their %, at most 3, then "+N more", linking to Insights) joins it once IN-1 ships (AC-IN1-2). |
| D41 · While you were away and time zones (`away-digest-and-timezones-v1.md`, 2026-11-12) | **Home leads with a "While you were away" card, above Needs you's content and never replacing it, after an away of at least the owner's threshold.** Needs you stays directly below, unchanged; the card's only Needs-you element is one amber line, "N things need you", linking to the Inbox, and it has no Needs you section. With the threshold Off, no new card appears; an unseen card stays until it's marked seen (away-digest-and-timezones-v1 Z5, AC-AW2-1). The account menu's Your preferences gains a Time zone (v1.0) and a "Show what happened while I was away after" threshold (10 min, 30 min, 1 h default, 4 h, Off). Activity gains two pages: While you were away (v1.1, AW-6's list; its picker is Next) and Daily summary (Next). Details, away rules and stories are in [`away-digest-and-timezones-v1.md`](away-digest-and-timezones-v1.md) (TZ-1..TZ-4, AW-1..AW-9). It amends the Home order (D1, §4, AC-IA04-1), the account menu (D21, D29) and Activity (§4). The card applies once AW-3 ships; the Time zone preference once TZ-1 ships and the threshold once AW-2 ships. Until then they don't show. |
| D42 · Costs and budgets (`costs-and-budgets-v1.md`, 2026-11-12) | **Insights' Cost section (v1.1) and budgets (task and agent v1.0; release cap v1.1) are specified in [`costs-and-budgets-v1.md`](costs-and-budgets-v1.md): R&D/Build in v1; Run/KTLO in v2.** Organization settings gain a Costs section for the price list (owner only, costs req 29; §4, D29, AC-IA03-1). Budget reached Inbox items are non-dismissible. It amends D26's "Budget reached (BUD-1) · Dismiss for this month" row and D26's dismissing rule for budget-reached items; both are updated in place (2026-11-12). **The Organization group gains a Cost page (v1.1), owner only** (costs-and-budgets-v1 requirement 31, CB-13; *Emery's addition; the owner can cut it*): totals for a period by project and by agent, R&D only. It amends §4. Until CB-13 ships, the item doesn't show. Details and the amendments to this file are in costs-and-budgets-v1 §12 (§12.2, §12.3). |
| D43 · New ▾ includes Request (staff review, 2026-11-12) | **The New ▾ menu is Project, Work item, Request, Message, then a separator, Hire an agent, Add a person** (AC-IA19-1). Request opens Eden's New request dialog (`feature-requests-ux.md` §1.1), the same dialog as Work › Requests' [New request]. A request is its own work kind (REQ-n, scoped by an agent, approved into tasks), not a task with Asked by. Like every item, it's left out until its flow exists. Reason: "I want X" is the owner's most common ask of the org, so it belongs in the one creation menu. Eden adds it to the design system's New ▾ list. |
| D44 · Routines (`ceremonies-and-retros-v1.md` Rev 17, D10, D22, §5.4, §8, §12; 2026-11-13) | **The team page and the project page each gain a Routines tab, with a routine page and an occurrence page under it.** Placement as the ceremonies PRD proposes: after Projects on a team, after Team on a project, pending Eden's E1. Routes follow D19 (IDs, not names) and the release-page pattern of D39; Rowan sets them. The tab lists one row per routine (name, kind, schedule with its zone, next slot, lead, last outcome, status: Enabled · Paused · Needs a lead · Missed twice), with the owner's **New routine**; the routine and occurrence pages are as ceremonies §8. **D26 gains two Problem rows from routines, "Routine {name} missed twice" and "Routine {name} needs a lead", both non-dismissible** (rows below). A routine itself creates exactly these two kinds of Inbox item; action items are tasks, and their Inbox items stay `tasks-v1`'s. **D9 is amended** with the routine Problems' inline actions (Open · Run now · Pause on missed twice, Edit in place of Run now when the routine needs a lead; Open · Edit · Pause on needs a lead). **Routines are not in New ▾** (D43 unchanged), and **D29 stands**: Processes is not in this release, and routines are not Processes. **Activity:** routine lines A1-A7 appear in Activity; which D24 filter they use is pending Eden's E2. A3 carries a retro's closed outcome; Retro ready stays Activity-only (D26). It amends §4 and §6 (team and project page tabs), D9 and D26. Until routines ship, the tabs and rows don't show. Reason: routines belong to their scope, and the owner sets them once and hears only when the rhythm breaks; Processes stays a separate, later feature (ceremonies D10, D22). |
| D45 · Platform (the owner via ada, 2026-11-13) | **Constraints:**<br>- v1 has no desktop shell. The UI runs in the user's own browser against `org-gateway` on localhost.<br>- Supported browsers: Chrome or Chromium (the last 2 stable majors); Firefox (the latest stable release plus the current ESR); Safari on macOS (the last 2 majors).<br>**Out of scope for v1:** the Tauri window and its WKWebView. `apps/desktop` is parked.<br>**Future:** a Mac status-bar app that lists the office servers (running or stopped) and starts and stops them. It isn't a v1 item.<br>No story or acceptance test may assume a Tauri window or a native shell. Earlier wording that says "desktop app" or "the Tauri window" means the browser UI. Safari bugs (WebKit) are real bugs. (A correction later the same day added Safari.) Why: the owner's decision; one surface for v1, the browser, on three engines. |

**D26: Inbox kinds for every Needs-you source.** Sources whose feature isn't in v1.0 arrive when that feature ships.

| Source | Kind | Clears when | Dismissable |
| --- | --- | --- | --- |
| Hire request; run approval; permission request | Approvals | acted on anywhere | no |
| Scoped request waiting on the owner (RQS-7) | Approvals | approved or declined | no |
| Office join request (OFC-5) | Approvals | approved or declined | no |
| Fix to verify (FIX-4) | Approvals (Approve fix / Reopen) | verified or reopened | no |
| Work waiting for the owner's review ("Review requested", rev 4.1) | Approvals (Approve / Request changes). v1.0: an agent's ask to the owner about a work item, marked as a review request. Approve sends "Approved", and Request changes sends the owner's text (Rowan's option a; a real review state comes after v1.0). | answered | no |
| Agent ask; a question about a work item (IA-10, 2026-11-12) | Questions | answered anywhere | no |
| Unaccepted hand-off to the owner (IA-10, 2026-11-12) | Approvals (Accept). The owner is being asked to take something on, which is consent rather than an answer. | accepted by any path (Chat's accept_handoff included) | no |
| Escalation; proposed ADR (DEC-5); exception (EXN-2/3) | Decisions | decided | no |
| Blocked-on-you work; goal paused | Problems | unblocked or resumed. "Blocked on you" stands whenever work is blocked on the owner and no Approval or Question about that work is open. After a refusal (Deny, or "Not now" in Chat), the Approval clears, the work stays blocked, and Blocked on you takes its place. It is withdrawn, with no Handled entry, when an Approval or Question about the work opens. So blocked work always has an open Inbox item (2026-11-12, design system §9). A Budget reached block is the exception: it has its own Budget reached item (below) instead of Blocked on you, and that withdrawal never applies to it (`tasks-v1.md` §6.13, AC-TK4-3). | no |
| Run error; crash | Problems | resolved | **yes** |
| Budget reached, a task's or an agent's (BUD-1; `costs-and-budgets-v1.md` CB-6, CB-7, D42). Actions: Raise budget, Allow 20% more once, Stop (Keep paused for an agent). On a task budget, Raise and Allow once are a person's actions: the owner or a human project manager. | Problems | a person raises the budget above spent, allows 20% more once, clears it, or changes its unit to a value above spent in the new unit; a cost move drops the task below its current limit (an Allow once raises that limit); the task reaches Done (a bug: Fixed), is cancelled or stopped (a bug: Closed), or is deleted (with its stopped subtasks, for a parent); for an agent, its day or month resets ("Keep paused" leaves the item open until then). A lowered value clears nothing, and neither does a Raise or Allow once refused because it would leave the limit at or below spent (2026-11-12, costs §12.2, C20). A release cap makes no item. | no |
| Critical bug (TRI-5) | Problems | **assigned** to anyone, or closed (decided 2026-11-12). Triage alone doesn't clear it, because the owner's worry is "is someone on it". | no |
| Project health worsens (STATUS-8) | Problems | health recovers | no |
| Unplaced agent (OFC-10); missing model (MOD-7) | Problems | placed; or, for missing model, the model is changed to one the runtime has, the runtime gains it, or the agent's model is cleared | no |
| Routine {name} missed twice (D44; `ceremonies-and-retros-v1.md` §5.4, §8). Opens on two consecutive misses of one routine (Missed: Announced with no start by the deadline; or Closed without outcome after 4 h); Skipped occurrences (including "no lead"), Cancelled ones (including "lead left") and Closed without outcome "lead left" are neutral. One item per routine. Who: System. Actions as D9 (D44). | Problems | an occurrence of that routine is Closed (with an outcome), or the owner pauses, edits or deletes the routine; the consecutive-miss count resets | no |
| Routine {name} needs a lead (D44; `ceremonies-and-retros-v1.md` §5.4, §8). Opens as soon as the routine has no valid lead (its lead left the scope or was offboarded); a paused routine opens none while it stays paused, and opens it on Resume if it still has no valid lead. One item per routine. Who: System; what: "{lead} left {scope}" (or "was offboarded"). Actions as D9 (D44). | Problems | the owner sets a valid lead (edit), pauses or deletes the routine, or the assigned lead rejoins the scope | no |

**Missing model, defined (2026-11-12, from Eden's MOD-7):** the agent has an **exact model set** that **its runtime doesn't have**, so it's running on the inherited fallback. An agent with no model set is **not** missing one: it inherits, by design. Neither is an agent whose runtime is missing or whose office is unreachable; those show as other states. If the runtime's model list can't be read, there is no item. When unsure, don't nag.
| Retro ready (RE:49) | **Not an Inbox item.** It shows in Activity, because it doesn't wait on the owner (a retro's closed outcome is Activity line A3, D44). | — | — |
| Process load warning (PROC-17) | Not in v1.0, because processes are hidden | — | — |
