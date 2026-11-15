# IA v2 · Stories for ada

Emery · 2026-11-11 · Source: [information-architecture-v2.md](information-architecture-v2.md) rev 4.1 (decisions D1-D45; D36-D43 added 2026-11-12, D44 and D45 added 2026-11-13; D45: browser-only v1 on Chrome or Chromium, Firefox and Safari, with no Tauri window). Design detail: Eden's `review-ia-v2.md`, design-system v2 (`e51b184`) and `ia-v2-spec-impact.md`. Technical read: Rowan's `ia-v2-technical-read.md`. Prototype: `../prototypes/ia-v2/index.html` (draft 4.1).

**Rev 4 changes:**
- AC-IA03-1, IA03-3, IA04-2, IA04-3, IA06-1, IA09-2 and IA12-1 are amended. Their IDs are unchanged.
- IA-17, IA-18 and IA-19 are new.

**2026-11-12 changes (costs and tasks alignment):**
- AC-IA04-2 and AC-IA09-2: Budget reached items aren't dismissible; their actions are those of `costs-and-budgets-v1.md` (C20, §12.2, D42).
- AC-IA18-1: a task Hand-off also opens when someone else assigns an unassigned task to the owner (`tasks-v1.md` §4.4, §6.13).

**2026-11-12 late changes (staff review):**
- AC-IA09-2: Budget reached items also clear when a person changes the budget's unit to a value above spent (`costs-and-budgets-v1.md` req 23).
- AC-IA19-1: the New ▾ order names the separator before Hire an agent (D43).

**2026-11-13 changes (routines, D44):**
- AC-IA04-2, AC-IA09-2, AC-IA05-2, AC-IA12-1 and AC-IA17-1 gain a note: routines ship with the ceremonies PRD's stories (`ceremonies-and-retros-v1.md` CER/RET), and these ACs gain the routine Problem rows, the Routines tab or the routine Activity lines then. New ▾ (AC-IA19-1) is unchanged.

**Every story:**
- is one concern, aimed at under ~400 non-test lines (RULES.md #1);
- carries Eden's UX acceptance criteria for its pages, and the AC IDs below are stable;
- passes the UI preflight: four widths, 200% text, keyboard focus, no emoji.

**Sizes:** S ≤150 non-test lines, M ≤300, L ≤400 or split.
**Tiers** are my proposal; ada confirms them.
**Order:**
- **P0:** the structure the owner asked for. Shells with honest empty states.
- **P1:** real data and the human loop.
- **P2:** depth.

## P0: the new structure (no new data needed)

### IA-01 · Nav rev 5 shell and redirects (re-scoped S1)
As the owner, I see the app organized like my company: Home, Inbox, then Projects · Work · Chat · Activity, then Teams, then Cost (owner only, once CB-13 ships), then Organization settings.
- **AC-IA01-1:** the sidebar shows, in order:
  - Home and Inbox, with the count badge (amber, only when count > 0);
  - Following (neutral badge; once TK-1 ships, `tasks-v1.md`, D38);
  - Projects, with up to 5 "Recent" projects and "View all";
  - Work, Chat, Activity;
  - Teams, with up to 5 teams and "View all";
  - Cost, owner only, once CB-13 ships (`costs-and-budgets-v1.md` req 31, D42);
  - Organization settings, at the bottom.

  There is no "Work" group label (Eden).
  - Sequencing (nav freeze rev 5, `6c98d5e`): the Inbox item is hidden until IA-09 lands, and the Teams sub-list arrives with IA-05.
- **AC-IA01-2:** the footer reads "You · Owner", which opens the account menu (Your preferences, Switch organization, About, Sign out when it exists; D21), and the office line "{Office name} office · {status}" (from `OfficeInfo.name`, e.g. "HQ office · Online"), which links to `/settings/offices`. The line never names the host or machine; those appear only in Settings › Offices (design system v2.3, Rowan 12 Nov).
- **AC-IA01-3:** these redirects keep their query string: `/` → `/home`, `/agents` → `/teams/members`, `/org-chart` → `/teams/org-chart`, `/offices` → `/settings/offices`, `/processes` → `/settings/processes` (still "Not in this release" in production), `/sessions` → `/activity`.
- **AC-IA01-4:** `aria-current` follows the route (a member profile marks Teams). The top-bar menu button shows only below 640 px.
- **Route/data:** routes only, built on Rowan's nav freeze rev 5. It reuses S1's `pageFor` table, focus helpers and footer.
- **Tier:** B · **Size:** M · **Depends on:** Rowan's nav rev 5 freeze.

### IA-02 · Teams view switch: Teams · Members · Org chart (relabel the merged S2)
- **AC-IA02-1:** `/teams`, `/teams/members` and `/teams/org-chart` share one view switch (the S2 component, relabelled).
- **AC-IA02-2:** Members lists every member (people and agents) as divided rows, with the filter All · People · Agents. Each row shows avatar, name, role, kind in words, presence pill (see IA-08) and "Working on …" when known.
- **AC-IA02-3:** the Org chart is a vertical tree with the owner card at its root. It replaces today's Teams, People and Reporting lines views (D36), along with their View menu, Fit and chart search. Teams live in the Teams index (D32) and people in Members.
- **AC-IA02-4:** the owner is never a member row (D17).
- **Route/data:** existing org view data · **Tier:** B · **Size:** M · **Depends on:** IA-01.

### IA-03 · Organization settings shell, with Offices moved
- **AC-IA03-1:** `/settings` has sub-navigation: General · Offices · Agent setup · Roles · Processes · Members and access · Costs (owner only, once `costs-and-budgets-v1.md` req 29 ships, D42) · Updates. It's an underline bar, or a left column of links from 960 px.
  - It holds org-wide settings only (D29). General shows the org name, the workspace and the Claude Code location.
  - Personal preferences (Appearance, Notifications) and About are **not** here. They're in the account menu that opens from "You · Owner" in the sidebar footer (D21). There's no `/members/you`. Your preferences also gains a Time zone (once TZ-1 ships) and the away threshold (once AW-2 ships) (`away-digest-and-timezones-v1.md`, D41).
- **AC-IA03-2:** `/settings/offices` renders today's Offices screen unchanged.
- **AC-IA03-3:** in-shell copy, pinned in nav rev 5 and design system §21:
  - Roles, Members and access, and Updates show "Not in this release". SET-3 (restart to update) isn't in v1.0 (ada, 2026-11-11).
  - General and Agent setup, until their stories land, show "{Title} isn't built yet" with "It's coming in this release.".
  - Processes keeps its link in the settings sub-nav. In production, like every gated route, it shows the root Unavailable page "Processes isn't available yet".
- **Tier:** B · **Size:** S · **Depends on:** IA-01.

### IA-04 · Home shell
As the owner, I land on Home and see what needs me, how my projects are doing and who's working.
- **AC-IA04-1:** the sections are, in order: Needs you (top 5 Inbox items, with "Open Inbox (N)"), Projects (outlined cards), Happening now, and Recent activity. Once TK-1 ships (`tasks-v1.md`, D38), a Following card sits between Needs you and Projects, and it's hidden when there's nothing to follow (2026-11-12). Once AW-3 ships, a While you were away card leads, above Needs you's content and never replacing it, after an away of at least the threshold (once AW-3 ships, `away-digest-and-timezones-v1.md`, D41).
- **AC-IA04-2:** with no data, each section shows its designed empty state: "Nothing needs you", "No projects yet" with Create project, "Live status isn't available yet", "No activity yet". It also shows a loading skeleton and a one-line error with Retry.
  - Needs-you cards follow the Inbox rules (D26):
    - An item leaves Home when it clears anywhere.
    - Only run error and crash items show Dismiss. Budget reached items, a task's or an agent's, aren't dismissible (`costs-and-budgets-v1.md` C20, D42). Their actions are Raise budget, Allow 20% more once and Stop (Keep paused for an agent); on a task budget, Raise and Allow once are a person's actions (the owner or a human project manager). They clear as AC-IA09-2 says.
    - Routine Problems, "Routine {name} missed twice" and "Routine {name} needs a lead", show no Dismiss either (D44). They arrive with the ceremonies PRD's stories (CER-8, CER-12), and this AC gains them then.
- **AC-IA04-3:** Project cards show the name, done/total progress (D15), the open and blocked counts, member avatars, and "Lead {name}" (D22), due date and health only when set. Health is set by hand in v1.0 (D30).
- **AC-IA04-4:** Needs-you cards have outline buttons only. There's one primary button per view.
- **Route/data:** `/home`. Projects and work counts use existing data; the other sections show empty states until IA-09, IA-12 and IA-15.
- **Tier:** B · **Size:** M · **Depends on:** IA-01.

### IA-05 · Teams index and team page
- **AC-IA05-1:** `/teams` shows the owner card, then one divided row per team: name, lead, member count (people · agents) and active projects.
- **AC-IA05-2:** `/teams/:teamId` (by ID, D19) shows:
  - a header with the name, lead, count line, purpose and "Hire or add ▾" (D33);
  - Members, capped at 6 rows with "All N members";
  - tabs Work (default), Projects, Activity, Conversations. A Routines tab (proposed after Projects, pending Eden's E1) joins once routines ship with the ceremonies PRD's stories (CER-9, D44).
- **AC-IA05-3:** work rows show the work's status once (Eden T1): id and title; meta: project · assignee · priority; one trailing pill.
- **Route/data:** org view (teams, members) and work reads · **Tier:** B · **Size:** L (split the tabs out if over 400) · **Depends on:** IA-02.

### IA-06 · Member profile page
As the owner, I open any member, human or agent, and see them as a colleague.
- **AC-IA06-1:** `/members/:memberId` shows:
  - a header with a 40 px avatar, a 16 px name with the role, the kind in words, teams, "Reports to …" and one presence pill with one line;
  - the actions Message (primary) and Assign task (outline), for people and agents alike (D25);
  - a "More" menu: Pause/Resume, Edit and Offboard for agents, and Edit for people.
- **AC-IA06-2:** the tabs are Overview, Work, Conversations, Activity, and Notes and memory (agents only, showing "Not available yet" until IA-14).
- **AC-IA06-3:** an agent's profile shows a Setup summary (runtime, model, how it runs, office, home folder) with "Open in Agent setup". Paths wrap and never scroll sideways at 390 px.
- **AC-IA06-4:** when the member has no current task, Overview reads "Not working on anything right now".
- **AC-IA06-5:** roster names and Org chart nodes link to the profile.
- **Route/data:** org view, `current_task_id` (C2a) and work reads · **Tier:** B · **Size:** L · **Depends on:** IA-02.

### IA-07 · Agent setup (the configuration face)
- **AC-IA07-1:** `/settings/agents` lists agents with runtime, model, how it runs, office and health. Below 640 px they show as divided rows. The route is `/settings/agents` from the first slice, S1a (nav rev 5).
- **AC-IA07-2:** `/settings/agents/:memberId` holds today's configuration fields and actions: How it runs, Edit, Pause, Offboard. It has a "View profile" link.
- **AC-IA07-3:** "Hire an agent" opens the existing flow, with the copy changed from "Add" to "Hire" (D14).
- **Route/data:** today's roster data and actions move here · **Tier:** B · **Size:** M · **Depends on:** IA-03.

### IA-08 · One presence vocabulary (UI mapping)
- **AC-IA08-1:** one component maps the wire `MemberPresence` and the member status to the labels Needs you, With you, Working, Paused, Available, Asleep, Offline and Status unknown.
  - Precedence and colours follow Eden's table.
  - Every pill has an icon and a word. Every avatar dot has a word, or an accessible name in dense views.
- **AC-IA08-2:** truth rules:
  - Paused comes from the member status.
  - Needs you comes from open Inbox items once IA-09 exists.
  - Everything the office doesn't report shows Status unknown, with no dot. In lists and rows, an unknown status shows **no pill at all**, only the "Working on …" line when known. The member profile header shows "Status unknown" once.
  - "Paused" never claims who paused it.
- **AC-IA08-3:** the roster, chat avatars, team pages and profiles all use this component. Chat's separate presence words are removed.
- **Tier:** B · **Size:** M · **Depends on:** none (can run in parallel with IA-01).

## P1: the human loop and real data

### IA-09 · Inbox page and decision card (first source: chat asks)
- **AC-IA09-1:** `/inbox` has tabs All · Approvals · Questions · Decisions · Problems · Handled (D11), and one "Show ▾" menu for project and team.
- **AC-IA09-2:** each item is a decision card: who · what · why (context and recommendation) · actions · expiry.
  - From 960 px, list and detail sit side by side.
  - Below 960 px, an item opens as its own view with "Inbox" back.
  - Each source maps to one kind, following the D26 table in the PRD.
  - An item clears when it's resolved anywhere, not only in the Inbox.
  - Only run error and crash items can be dismissed. Task and agent Budget reached items are not dismissible (`costs-and-budgets-v1.md` C20, §12.2, D42).
    - Their actions are Raise budget, Allow 20% more once and Stop (Cancel on a task, Won't fix on a bug); an agent's are Raise budget, Allow 20% more once and Keep paused. On a task budget, Raise and Allow once are a person's actions: the owner or a human project manager.
    - They clear when the budget is raised, cleared or extended once, a person changes its unit to a value above spent in the new unit, the task is stopped or cancelled (a bug: Closed, any resolution), the task reaches Done (a bug: Fixed), the item is deleted (with its stopped subtasks, for a parent), a cost move drops the task below its current limit (an Allow once raises that limit), or the agent's period resets. A Raise or Allow once that leaves the limit at or below spent is refused and clears nothing; a lower value is accepted and clears nothing. An agent's "Keep paused" leaves the item open until that reset, when it clears with the neutral reset notice. A release cap makes no Inbox item.
    - A task Blocked "Budget reached" gets only its Budget reached item, not a "Blocked on you" item (`tasks-v1.md` §6.13, AC-TK4-3).
  - Routine Problems, "Routine {name} missed twice" and "Routine {name} needs a lead", are Problems and not dismissible; their actions and clearing rules are D9 and D26 (D44). They ship with the ceremonies PRD's stories (CER-8, CER-12), and this AC gains them then.
  - A scoped request waiting on the owner is an Approval card. It never shows an amber "Scoped" pill (D23).
- **AC-IA09-3:** the first source is the existing chat asks. Reply works inline (D9), and the item moves to Handled when answered. Chat's own Needs you section links here.
- **AC-IA09-4:** the sidebar badge and Home's Needs you read the same count.
- **Route/data:** messaging asks (exists) · **Tier:** B · **Size:** L · **Depends on:** IA-01, IA-04.

### IA-10 · Cross-kind Inbox API (backend)
- **AC-IA10-1:** one read API returns open and handled owner-waiting items across all D11 kinds (asks, approvals, permission requests, decisions, blocked-on-owner work, goal paused, run errors). Each item carries kind, source member, project, team, summary, context, recommendation, expiry, available actions and clearing rule.
- **AC-IA10-2:** clearing is reported live, so the badge drops within 2 s of the owner acting anywhere.
- **Route/data:** a new contract (Rowan designs it; M3, F2-09 and B1-13) · **Tier:** A · **Size:** L+ (Rowan to slice) · **Depends on:** the B1-12 approvals and work reads (A2/B).

### IA-11 · Inline Approve and Deny
- **AC-IA11-1:** Approval cards show Approve and Deny when the source operation exists (hire approval M2, run approvals B1-12), calling exactly that operation. Otherwise the card shows Open (D9).
- **AC-IA11-2:** a deny that requires a reason asks for it inline. A failure leaves the card open with the server's message.
- **Tier:** A (authorization path) · **Size:** M · **Depends on:** IA-10.

### IA-12 · Activity page
- **AC-IA12-1:** `/activity` merges the org changes (org log) and the work feed (C2b), newest first, with filters All · Work · Conversations · Decisions · Runs · Org changes (D24). Runs appear once they're recorded (IA-15). (Plus While you were away and Daily summary once AW-6/AW-7 ship, `away-digest-and-timezones-v1.md`, D41.) Routine lines A1-A7 join once routines ship with the ceremonies PRD's stories; their filter is pending Eden's E2 (D44).
- **AC-IA12-2:** a run entry expands in place to show its log summary (trigger, duration, outcome).
- **AC-IA12-3:** the member profile and team page Activity tabs reuse this feed, filtered.
- **Route/data:** a new activity read contract (Rowan) · **Tier:** A for the API, B for the UI · **Size:** M + M · **Depends on:** B1-11 C2b.

### IA-13 · Search
- **AC-IA13-1:** typing in the top-bar search shows grouped results (Projects, Work, Members), navigable with the arrow keys and Enter. Below 640 px it's a 44 px icon button that opens a full-width field.
- **AC-IA13-2:** "See all results" opens `/search?q=` with the same groups.
- **Tier:** B (client-side over loaded data first) · **Size:** M · **Depends on:** IA-01.

### IA-15 · Runtime presence reporting (backend, scope ask)
- **AC-IA15-1:** the office reports Busy, Idle, Asleep, NeedsYou and WithYou per agent from session and run events, so the UI shows Working, Available, Asleep, Needs you and With you truthfully.
- **AC-IA15-2:** runs are recorded (trigger, start, end, outcome) for Activity.
- **Route/data:** the existing wire enum, no protocol change. The milestone is ada's call; I ask for v1.0, because without it no agent ever looks alive.
- **Tier:** A · **Size:** Rowan to size · **Depends on:** the runtime (B3).

### IA-17 · Project page tabs (D31)
As the owner, I open a project and see how it's doing, its work, who's on it and what's happening.
- **AC-IA17-1:** `/projects/:projectId` shows:
  - a header with the name, "Lead {name}" when set (D22), done/total progress (D15, defined per `project-insights-v1.md` IN-2, D40), and health and due date only when set;
  - tabs Overview (default), Work, Team, Activity, Conversations, with "← Projects" back. Backlog and Releases tabs follow Work once PL-5 and PL-6 ship (`projects-planning-v1.md`, D39). A Routines tab (proposed after Team, pending Eden's E1) joins once routines ship with the ceremonies PRD's stories (CER-9, D44).
  - This replaces Brief · Team · Tasks (PROJ-2) and Overview · Tasks · Bugs (PD-2, PD-3).
- **AC-IA17-2:** each tab shows existing data:
  - **Overview:** the brief, the open and blocked counts, and the 5 most recent activity entries (plus a Flow tile once IN-1 ships, `project-insights-v1.md`, D40).
  - **Work:** the project's work items in the IA-05 row format (status shown once).
  - **Team:** the teams assigned to the project, with add and remove as today (D37), then the members as Members rows.
  - **Activity:** the IA-12 feed filtered to the project, or "No activity yet" until IA-12 lands.
  - **Conversations:** threads linked to the project, or "No conversations yet".
- **AC-IA17-3:** Features, Bugs, Debt, Docs, Insights and Settings tabs aren't shown in v1.0. Insights returns once IN-1 ships (`project-insights-v1.md`, D40). Bugs are work items, and Work filters them by type.
- **AC-IA17-4:** health is set by hand from the header's More menu (D30).
- **Route/data:** project and work reads (exist) · **Tier:** B · **Size:** M (tabs that need IA-12 ship as empty states) · **Depends on:** IA-01, IA-05 (row format).

### IA-18 · Work views (D31)
- **AC-IA18-1:** `/work` has a "Show ▾" menu of saved views: Needs me, Mine, By project, By assignee. TK-2 (`tasks-v1.md`, D38) adds Created by me and Asked by me, a Followed filter (AC-TK2-3), and a Created by ▾ filter. The chosen view is in the URL (`?view=`).
  - With no `?view=`, the default is Needs me when the owner has open items, and Mine otherwise.
  - Needs me lists every task that waits on the owner, in exactly the five cases of `tasks-v1.md` §4.1 (2026-11-12): blocked on the owner, the owner is its reviewer, a Hand-off to the owner is open, a question to the owner is open, or its budget is reached (a Budget reached block or hold). Task hand-offs open whenever someone other than the owner assigns a task to the owner: on create (a subtask or a promoted step included), on assigning it from unassigned, on reassignment or on reopen, not only on reassignment (`tasks-v1.md` §4.4, §4.8). They come from task state (`tasks-v1.md` §6.13, AC-TK6-3, AC-TK6-5, AC-TK6-6) and are always listed while open: the owner is the assignee and hasn't made one of the moves that clear it (`tasks-v1.md` §4.4). Chat hand-offs arrive with IA-10b, through the hand-off adapter; only one that names its work item is listed here, and one without a work item shows in the Inbox and Chat only (Rowan).
  - Work blocked on the owner always has an open Inbox item: its Approval or Question, or else a "Blocked on you" Problem, or, for a block caused by its budget, its Budget reached item (D26). A refusal leaves it blocked and in Needs me, and Blocked on you takes the cleared Approval's place (D26). A review wait is also a "Review requested" Inbox Approval, an open question to the owner is an Inbox Question, and an unaccepted hand-off is also a "Hand-off" Inbox Approval (D26), so the row's Needs you, the badge and the member's Needs you agree.
  - By assignee groups by member, people and agents together.
- **AC-IA18-2:** a List | Board toggle. Board columns are the work statuses, and List rows use the IA-05 format. The columns include a collapsed Cancelled column (once TK-4 ships) and In review, which is hidden until TK-5 ships (tasks-v1.md, AC-TK4-1).
- **AC-IA18-3:** each view has an empty state, for example "Nothing needs you in Work".
- **Route/data:** work reads (exist); Needs me uses blocked-on-owner from work status until IA-10 · **Tier:** B · **Size:** M · **Depends on:** IA-01.

### IA-19 · Top-bar New ▾ menu (D31)
- **AC-IA19-1:** the top bar has one outline **New ▾** button. It replaces the top-bar Hire and Add agent buttons (SHELL-2, SH-13).
  - The menu holds, in order, Project, Work item, Request, Message, then a separator, Hire an agent and Add a person (D33, D43). Request opens Eden's New request dialog (`feature-requests-ux.md` §1.1), the same one as Work › Requests' [New request]; a request is its own work kind (REQ-n), not a task.
  - Items whose flow doesn't exist yet are left out, not disabled.
- **AC-IA19-2:** pressing `N` (or Shift+N, per design system v2.2) opens the menu when focus isn't in a text field. There's no ⌘N or Ctrl+N anywhere in v1.0 (design system v2.2); the browser keeps them for a new window (D45: v1 runs in the browser, with no Tauri window). Arrow keys and Enter choose an item, and Esc closes the menu.
- **AC-IA19-3:** below 640 px it's a 44 px icon button with the accessible name "New".
- **AC-IA19-4 (WCAG 2.1.4):** Your preferences has a Keyboard section with a "Single-key shortcuts" switch, on by default.
  - When it's off, N and `/` do nothing outside text fields, and the New button and search input drop `aria-keyshortcuts` and the hint.
  - The setting persists per device, with one device-wide key (nav rev 5), without syncing in v1.0, like Chat's "Enter sends". If it can't be read, it falls back to on. A synced preference comes later.
  - N ships together with this switch (IA-19). The switch also covers `/`, which ships with IA-13 (Search), and governs it from day one. Until the Keyboard section lands, Your preferences shows "Your preferences isn't built yet" with "It's coming in this release.".
- **Tier:** B · **Size:** M (S before AC-IA19-4) · **Depends on:** IA-01.

**Deferred, no story:**
- **Insights** is the project's last tab, after v1.0 (D40, AC-IA17-3). Its metrics are in `project-insights-v1.md`; costs are in `costs-and-budgets-v1.md` (D42).
- **Knowledge** (V1 §3.11) comes later, through Notes and memory (D28).

## P2: depth

### IA-14 · Notes and memory (read-only)
- **AC-IA14-1:** for a human principal only, an agent's profile shows its latest journal entries and `memory/MEMORY.md`, read-only. It's confined to those paths, with no symlinks, size and entry caps, and text only (D18).
- **AC-IA14-2:** without the API, the tab says "Not available yet: notes and memory arrive in a later release".
- **Tier:** A, with a security review · **Size:** about 250 lines of API plus a UI slice (Rowan) · **Depends on:** IA-06.

### IA-16 · No emoji in agent-authored text (D7)
- **AC-IA16-1:** at render only, agent-authored message and text content has `\p{Extended_Pictographic}` sequences removed, including ZWJ, variation selectors, skin tones and keycaps, while plain digits and `#` are kept. Human-typed text is unchanged, and storage is unchanged.
- **AC-IA16-2:** a unit test fails if any emoji code point appears in `apps/org-ui/src` (E3).
- **Tier:** B · **Size:** S · **Depends on:** the Chat emoji slice (already queued for Avery).

## Already queued by ada

**E-01 · Remove emoji from Chat (Avery, Tier B).** It covers the 14 occurrences in `../research/emoji-inventory.md`, replacing each with a Heroicon plus its word. The pseudo-icons (×, ↳, ›, @) are replaced with icons in the same slice.

## Milestone order and size priorities (for ada's sequencing)

Points use the plan's weights: S=1, M=2, L=4.

| IA milestone | What the owner gets | Stories | Points | Must land before |
| --- | --- | --- | --- | --- |
| **IA-M1: the company structure** (first) | The new sidebar, Home as the landing page, Teams · Members · Org chart, Organization settings with Offices and Agent setup, one honest presence vocabulary. Old URLs still work. | IA-01 (M), IA-08 (M), IA-02 (M), IA-03 (S), IA-04 (M), IA-07 (M), IA-19 (S) | 12 | any more BUG-70 nav work |
| **IA-M2: agents as colleagues** | Team pages and member profiles (people and agents), with "Working on …" from C2a | IA-05 (L), IA-06 (L), IA-17 (M) | 10 | B1-11 C2a for "Working on"; the shells can go first |
| **IA-M3: the human loop** | Inbox with decision cards; Reply in place now; Approve/Deny once the operations exist; grouped search | IA-09 (L), IA-13 (M), IA-18 (M), IA-10 (Tier A, Rowan sizes), IA-11 (M) | 10 + IA-10 | IA-10 at the plan's M3 (F2-09/B1-13); hire approval (M2) and B1-12 for IA-11 |
| **IA-M4: alive and accountable** | Real presence (Working, Available, Asleep, With you), the Activity feed with runs, notes and memory, no emoji in agent text | IA-15 (Tier A), IA-12 (M+M), IA-14 (Tier A), IA-16 (S) | 5 + Tier A | X-02 worker activation and C2a before IA-14 (ada); C2b before IA-12 |

**Size priority within a milestone:** the first story listed goes first.

**If capacity forces a cut**, cut from the bottom: IA-16 first, then IA-14, then IA-13, then IA-18 (rev 4: IA-17, IA-18 and IA-19 add 5 points). **Never cut IA-M1, IA-09 or IA-15.** Those three are the structure, the human loop and the colleague model the owner asked for.

## Suggested order

- **First, in parallel:** IA-01 and IA-08.
- **Next, in parallel:** IA-02, IA-03 and IA-04.
- **Then:** IA-05, IA-06 and IA-07.
- **Then:** IA-09 and IA-13.
- **Backend, alongside everything above:** IA-10, IA-12's API and IA-15 run on Rowan's and ada's sequencing.
- **After IA-10:** IA-11.
- **Last:** IA-14 and IA-16.

**Rough total:** about 10 Tier B UI slices (about 2,500-3,000 lines) and 4-5 Tier A backend slices.
