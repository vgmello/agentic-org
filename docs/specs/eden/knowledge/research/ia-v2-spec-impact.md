# IA v2 · Impact on my specs, the BUG-70 rows and the nav freeze

Eden · 2026-11-11 · for ada (`ac-staff-engineer-ada`) and Rowan (`ac-fullstack-engineer-rowan`), to re-sequence work.

**Purpose:** one working list of what IA v2 rewrites, supersedes, moves or leaves standing. It isn't the rewrite itself; the spec revisions follow from it.

**Rule: IA v2 (PRD rev 3 §11, D9–D19) wins where it conflicts.** §11 overrides §10 (PRD:5), and §10 overrides the earlier sections.

**Sources** (short names used below)
- PRD = `ac-product-manager-emery/knowledge/prds/information-architecture-v2.md` (rev 3)
- ST = `ac-product-manager-emery/knowledge/prds/ia-v2-stories.md` (IA-01…IA-16, E-01)
- RV = `knowledge/research/review-ia-v2.md` (my review and its Addendum)
- V1 = `knowledge/specs/desktop-app-v1.md` (rev 4)
- OO = `knowledge/specs/our-org-in-the-app.md` · OT = `offices-and-teams-ux.md` · CH = `chat-ux.md` · PS = `project-status-ux.md` · RM = `agent-run-modes-ux.md` · WI = `work-items-ux.md` · RO = `roles-ux.md` · MO = `models-ux.md` · PC = `processes-ux.md` · CI = `costs-and-insights-ux.md` · DT = `decisions-and-tech-debt-ux.md` · FR = `feature-requests-ux.md` · RE = `retrospectives-ux.md` · DF = `dogfood-run.md`
- G70 = `knowledge/research/bug-70-per-screen-gaps.md`
- NF = `ac-fullstack-engineer-rowan/knowledge/designs/bug70-nav-freeze.md` (rev 4) · SO = `…/bug70-slice-order.md` · TR = `…/ia-v2-technical-read.md`

Cites are `file:line`. **check** = I'm not sure, or there's no IA story yet; it needs a look before the rewrite.

---

## 1. v1 spec (`desktop-app-v1.md`)

**Context:** V1 rev 4 is the Tauri-era single-machine spec. Later specs (OO, OT, WI, CH) already replaced parts of it. I list only what IA v2 changes now.

### 1a. Criteria

| ID | Today (≤15 words) | Under IA v2 (≤20 words) | Disposition | IA story |
| --- | --- | --- | --- | --- |
| SHELL-1 (V1:730) | Nav: Home, Org chart, Agents, Projects, Tasks, Knowledge, Settings; no Hire item | Home, Inbox · Projects, Work, Chat, Activity · Teams · Organization settings; no group label | Rewrite | IA-01 (AC-IA01-1) |
| SHELL-2 (V1:731) | ⌘N or top-bar **Hire** opens the Hire panel | Top-bar **New ▾** (outline) › Hire an agent; also team page "Hire or invite ▾". ⌘N: check | Rewrite | IA-01, IA-05, IA-07 (no story owns New ▾: check) |
| SHELL-3 (V1:732) | ⌘K palette with Recent and Actions | IA has top-bar grouped search, no palette spec. Does ⌘K survive? | check | IA-13 |
| SHELL-4 (V1:733) | `ac-ada` finds agent in Agents group; Enter opens quick view | Results grouped Projects, Work, Members; Enter navigates to the member's profile | Rewrite | IA-13 (AC-IA13-1) |
| SHELL-5 (V1:734) | Bell, Home nav badge, dock badge show the same count | Inbox sidebar badge and Home Needs you read the same count; no bell. Dock: check | Rewrite | IA-09 (AC-IA09-4), IA-01 |
| SHELL-7 (V1:736) | Notification click opens Session panel, or Home with item highlighted | Fallback lands on the Inbox item, not Home | Rewrite | IA-09 |
| SHELL-13 (V1:742) | Switcher: org name, "This computer", workspace path, recent workspaces | Footer "You · Owner" + switcher, and office line "This Mac · HQ office · Online" | Superseded | IA-01 (AC-IA01-2) |
| SHELL-17 (V1:746) | Status stays Working, never Idle, within 2 s | Word "Idle" becomes **Available** | Unchanged-but-relabel | IA-08 |
| SHELL-21 (V1:750) | Other workspace's item: switcher count, bell unchanged | "bell" → Inbox badge | Unchanged-but-relabel | IA-09 |
| HOME-1 (V1:754) | Needs you: "ada wants to run Bash", wait time, **Open session** | Inbox **Approvals** card (permission request): who/what/why/expiry; Approve/Deny once the op exists, else Open | Rewrite | IA-09, IA-10, IA-11 |
| HOME-2 (V1:755) | Open session opens the Session panel on the prompt | Card's **Open** goes to the source screen (D9) | Rewrite | IA-09, IA-11 |
| HOME-3 (V1:756) | Permission/question clears when the agent continues; three badges drop together | D11: Approvals clear only when acted on; Questions when answered; badge drops within 2 s | Rewrite (conflict, §8 #2) | IA-10 (AC-IA10-2) |
| HOME-4 (V1:757) | Only crash has Dismiss; others can't be dismissed | D11: Problems error **and** crash can be Dismissed; nothing else | Rewrite | IA-10 |
| HOME-5 (V1:758) | On hold: blocked-task item, dismissable, Dismissed filter | Problems kind "blocked-on-you work", clears when resolved, not dismissable | Superseded | IA-10 (D11) |
| HOME-6 (V1:759) | "Nothing needs you right now." stays visible | "Nothing needs you" | Unchanged-but-relabel | IA-04 (AC-IA04-2) |
| HOME-7 (V1:760) | Running now: sessions, status, DoD, GoalChip, Open session, Stop | **Happening now**: members with "Working on …", no pill; v1.0 empty "Live status isn't available yet" | Rewrite | IA-04, IA-15 |
| HOME-8 (V1:761) | No agents: whole page shows only Hire your first agent | Each section keeps its own empty state ("No projects yet" + Create project…) | Rewrite | IA-04 (AC-IA04-2) |
| HOME-9 (V1:762) | Summary strip: agents by status, tasks by status, no percentages | Replaced by Projects cards: done/total progress, open and blocked counts | Superseded | IA-04 (AC-IA04-3, D15) |
| HOME-10 (V1:763) | Backlog diff shows in Recent activity within 2 s | Recent activity reads the Activity feed (org log + work feed) | Rewrite | IA-04, IA-12 |
| HOME-11 (V1:764) | `bin/agent` terminal session item; Open session shows terminal state | Card offers Open (D9); terminal-session handling isn't an IA concept | Superseded (check) | IA-09 |
| HOME-12 (V1:765) | Oldest first; permission/question tie-break; no reordering | Inbox groups Today / Earlier; order inside groups not specified | Moves to Inbox (check order) | IA-09 |
| HOME-13 (V1:766) | Usage limit: one banner, one notification, error items | Items become Inbox **Problems** (run error) | Unchanged-but-relabel | IA-10 |
| HOME-14 (V1:767) | One question = one item, one notification | Same rule, for the Inbox | Moves to Inbox | IA-10 |
| ORG-1 (V1:771) | Root "You · Owner", HR staff node, department frames | Vertical tree, owner card at root (D17); no departments (OT rev 2) | Superseded | IA-02 (AC-IA02-3) |
| ORG-2 (V1:772) | Agent hangs under its `reports_to` manager | Same, on `/teams/org-chart` | Unchanged-but-relabel | IA-02 |
| ORG-3 (V1:773) | Project lens "View: <project>", persisted | No lens in IA v2 | check (likely Superseded) | — |
| ORG-4 (V1:774) | Arrows; Enter opens profile; Space toggles quick view | Enter opens `/members/:id`; nodes link to the profile | Rewrite | IA-06 (AC-IA06-5) |
| ORG-5 (V1:775) | ⌘0 / Fit fits the tree | Same, new route | Unchanged-but-relabel | IA-02 |
| ORG-6 (V1:776) | Unknown manager: hangs off root with chip | Same, new route | Unchanged-but-relabel | IA-02 |
| ORG-7 (V1:777) | Status icon + label update within 2 s | Presence pill/dot (IA-08); v1.0 only Paused / Status unknown | Rewrite | IA-08, IA-15 |
| ORG-8 (V1:778) | No agents: You, HR, "Hire your first agent" | Owner card + empty state with Hire an agent | Unchanged-but-relabel | IA-02, IA-07 |
| QV-1 (V1:782) | Quick view: header, status, machine, Needs you, goal, journal, actions | Rows and nodes open the profile page; side sheet's role unclear | check (Superseded by profile?) | IA-06 |
| QV-2 (V1:783) | Primary: Go to session / Resume / Open session | Profile primary is **Message** (M1) | check | IA-06 |
| AGT-1 (V1:789) | Agents table: name, title, dept, project, status, goal, reports to | Split: Members rows (kind, presence, Working on) + Agent setup list (runtime, model, run mode, office, health) | Rewrite | IA-02 (AC-IA02-2), IA-07 (AC-IA07-1) |
| AGT-2 (V1:790) | Profile tabs: Overview, Tasks, Sessions, Journal, Memory, Knowledge, Settings | Overview, Work, Conversations, Activity, Notes and memory (agents only) | Rewrite | IA-06 (AC-IA06-2) |
| AGT-3 (V1:791) | Overview lists all in-progress rows | Overview: working on, goal, recent activity; "Not working on anything right now" | Rewrite | IA-06 (AC-IA06-4) |
| AGT-4 (V1:792) | DoD from backlog, GoalChip Active/Paused | Overview "goal"; source now office work, not the backlog file | Rewrite (check) | IA-06 |
| AGT-5 (V1:793) | File tabs show path + Open in editor | Journal and memory become read-only Notes and memory, "Not available yet" in v1.0 | Superseded | IA-14, D18 |
| AGT-6 (V1:794) | Malformed backlog: error on Tasks tab | Work tab reads office work; no backlog file | Superseded (check) | IA-06 |
| AGT-7 (V1:795) | Offboard dialog: effects, focus on Cancel | Opened from profile "More" menu and Agent setup; content per OO DEAL-4 | Moves to profile More / Agent setup | IA-06, IA-07 |
| AGT-10 (V1:798) | Settings tab: `agent.json` read-only, "Changes go through HR" | Setup fields live in Agent setup; profile shows a Setup summary | Moves to Org settings › Agent setup | IA-07 (AC-IA07-2), IA-06 (AC-IA06-3) |
| SESS-1 (V1:836) | Status becomes Working or Idle | Working or **Available** | Unchanged-but-relabel | IA-08 |
| SESS-2 (V1:837) | Sessions tab lets the owner resume an earlier session | No Sessions tab; runs are Activity entries | Rewrite (check) | IA-12 (AC-IA12-3) |
| SESS-4 (V1:839) | Header status Needs you while prompt waits | Presence Needs you comes from open Inbox items | Unchanged-but-relabel | IA-08 (AC-IA08-2) |
| SESS-5, 6, 11, 13, 20 (V1:840, 841, 846, 848, 855) | Each creates or keeps a Needs-you item | Same items, as Inbox **Problems** (goal paused, error, crash) | Unchanged-but-relabel | IA-10 (D11) |
| SESS-8 (V1:843) | Card, quick view, Running now show terminal session | "Running now" → Happening now; card → member row | Unchanged-but-relabel | IA-04, IA-02 |
| SESS-12 (V1:847) | "No notifications" chip on card, quick view, Running now | Same relabel as SESS-8 | Unchanged-but-relabel | IA-04, IA-02 |
| SESS-17 (V1:852) | Sessions tab row without Resume | No Sessions tab; run entries | Superseded (check) | IA-12 |
| SESS-21 (V1:856) | "2 sessions running"; Sessions tab lists both | Listed in Activity runs | Rewrite (check) | IA-12 |
| PROJ-1 (V1:861) | Project row: name, key, agents, task counts, no percentage | IA list: status, progress (done/total), open work, team | Rewrite | — (no story; PRD:64; check) |
| PROJ-2 (V1:862) | Project tabs: Brief, Team, Tasks | Overview · Work · Team · Activity · Conversations | Rewrite | — (no story; PRD:64, PRD:120-124) |
| PROJ-3 (V1:863) | View in org chart sets the lens | No lens (see ORG-3) | check | — |
| TASKS-2 (V1:870) | Tasks nav badge = Blocked rows | Only Inbox has a badge | Superseded | IA-01 |
| SET-1 (V1:885) | Sections: Workspace, Claude Code, Updates, Notifications, Appearance, About | General · Offices · Agent setup · Roles · Processes · Members and access · Updates | Rewrite (conflict, §8 #5) | IA-03 (AC-IA03-1) |
| SET-3 (V1:887) | Restart to update keeps sessions running | Updates shows "Not in this release" in v1.0 | check (conflict, §8 #5) | IA-03 (AC-IA03-3) |
| SET-4 (V1:888) | Notification details toggle, per-kind settings | Kinds become Approvals · Questions · Decisions · Problems; no Notifications section | Rewrite (check) | IA-10 (D11) |
| SET-5, 6, 7 (V1:889-891) | Notifications denied; Workspace trust; Claude Code environment | No section for them in Org settings | check | IA-03 |
| A11Y-4 (V1:912) | Live region announces a new Needs-you item | Same, for a new Inbox item | Unchanged-but-relabel | IA-09 |

### 1b. Section text without IDs

| Where | Today | Under IA v2 | Disposition | IA story |
| --- | --- | --- | --- | --- |
| §1 out of scope (V1:42) | "human members (OQ-1)" out of scope | Humans are team members from day one (D4); owner isn't (D17) | Rewrite | IA-02, IA-05 |
| §2.1 layout (V1:51-66, 73) | Nav list; top bar: search, bell, **Hire** | New nav; top bar: search, **New ▾** (outline) | Rewrite | IA-01 |
| §2.2 nav table (V1:82-92) | 7 items, ⌘1–⌘6, badges on Home and Tasks | IA nav; badge only on Inbox | Rewrite | IA-01 |
| §2.3 palette (V1:99-104) | Groups: Actions, Agents, Projects, Tasks, Files | Search groups Projects, Work, Members (Conversations later) | Rewrite (check: palette) | IA-13 (D16) |
| §2.4 (V1:109) | Bell and Home › Needs you are the same list | Inbox and Home's top 5 | Rewrite | IA-09 |
| §2.4 (V1:112-118) | Clearing by reason; Dismiss only crash and blocked task | D11 clearing rules per kind | Rewrite | IA-10 |
| **§2.4 (V1:119)** | **"v1 has no answering from the inbox"**; Open session only | **D9 overrides:** Reply inline now; Approve/Deny inline once ops exist; else Open | Superseded | IA-09, IA-11 |
| §2.4 kinds table (V1:124-132) | Permission, Question, Waiting, Goal paused, Error, Crash, Blocked | Approvals · Questions · Decisions · Problems (D11 mapping) | Rewrite | IA-10 |
| §2.5 switcher (V1:138) | Org name, This computer, workspace menu | Footer per AC-IA01-2 (OT §4 already changed it) | Superseded | IA-01 |
| §2.6 precedence (V1:174) | Unreachable > Needs you > Working > Idle > Offline | Needs you > With you > Working > Paused > Available > Asleep > Offline > Status unknown | Rewrite | IA-08 (RV:179) |
| §3.1 Home (V1:225-265) | Strip · Needs you · Running now · Recent activity | Needs you (top 5) · Projects · Happening now · Recent activity | Rewrite | IA-04 |
| §3.2 Org chart (V1:267-326) | Own nav item, departments, lens, HR node | Teams › Org chart, vertical tree | Superseded | IA-02 |
| §3.3 Quick view (V1:328-341) | Side panel with session actions | Profile page is the colleague view | check | IA-06 |
| §3.4 Agents list (V1:343-352) | Table, Hire, Show offboarded | Members + Agent setup; Former is membership, not presence | Rewrite | IA-02, IA-07 |
| §3.5 profile header (V1:358) | Primary Open session; breadcrumb Agents › Eden | Message (primary), Assign task (outline), More menu; Teams current | Rewrite | IA-06 (AC-IA06-1) |
| §3.5 tabs (V1:362-370) | Overview, Tasks, Sessions, Journal, Memory, Knowledge, Settings | See AGT-2 | Rewrite | IA-06 |
| §3.5 states (V1:386, 388) | "Go to Agents" | "Go to Members" | Unchanged-but-relabel | IA-06 |
| §3.10 Tasks (V1:536-547) | Tasks roll-up (WI already replaced it with Work) | Work adds Needs me, Mine, By project, By assignee views | Rewrite | — (no story; PRD:65) |
| §3.11 Knowledge (V1:549-560) | Own nav item | No place in IA v2 nav | check | — |
| §3.12 Settings (V1:562-573) | App and workspace settings | Organization settings shell | Rewrite | IA-03 |
| §6.1 shortcuts (V1:928-930) | ⌘1…⌘6 nav; ⌘⇧I bell | Must follow the new nav and Inbox | Rewrite (check) | IA-01 |
| §6.2 focus order (V1:948-949) | Top bar: search, bell, Hire | Search, New | Rewrite | IA-01 |
| §6.3 labels (V1:964-965) | "Needs you, 3 items"; "Tasks, 1 blocked" | "Inbox, 3"; no Tasks badge | Rewrite | IA-01, IA-09 |
| §7.2 (V1:1011) | Approve or answer from the inbox is v1.1 | Pulled into v1.0 by D9 | Superseded | IA-11 |

---

## 2. Other specs in my folder

| ID / place | Today (≤15 words) | Under IA v2 (≤20 words) | Disposition | IA story |
| --- | --- | --- | --- | --- |
| CH decision 1 (CH:19) | Chat is second nav item, after Home | Chat sits after Projects and Work | Rewrite | IA-01 |
| CH §1 layout (CH:40-58, 61) | ⭐🧵📣 emoji; DM status words Working, Waiting, Asleep, Offline, Paused; people Online | No emoji (E-01); presence component; add "Between agents" group (D8) | Rewrite | E-01, IA-08, IA-16 |
| CH §1 kind markers (CH:62) | ❓ 📣 🔁 kind markers | Heroicons plus words | Rewrite | E-01 |
| CH decision 5 (CH:29) | Owner can reply in any conversation | Agent-to-agent threads read-only, with "Reading as the owner. To weigh in, message {agent}" | Rewrite (conflict, §8 #3) | D13 (no story; check) |
| CHAT-1 (CH:109) | Sidebar: Needs you, Mentions, Threads, channels, DMs | Needs you links to Inbox; add Between agents group | Rewrite | IA-09 (AC-IA09-3) |
| CHAT-3 (CH:111) | DMs show each member's status | Status via the presence component | Unchanged-but-relabel | IA-08 (AC-IA08-3) |
| CHAT-5 (CH:113) | Answering an ask in Chat clears Needs you | Clears the Inbox Question item (Handled) | Unchanged-but-relabel | IA-09 |
| CHAT-12 (CH:120) | Owner reads any conversation, "reading as the owner", **can reply** | Can't reply in agent-to-agent threads (D13) | Rewrite | D13 |
| CHAT-18 (CH:126) | A person's DM status is a word | No presence for persons other than the owner in v1.0 (D12) | Rewrite | IA-08 |
| CH §2 (CH:74) | Ask to owner also in Home › Needs you | Inbox + Home top 5 | Unchanged-but-relabel | IA-09 |
| OO §5 card (OO:181) | Card: square avatar; Working · Waiting · Idle · Needs you · Paused · Offline | Presence set (RV:109-117); kind in words | Rewrite | IA-08 |
| OO §5 profile header (OO:183) | Primary Open in <runtime> / Open session | Message primary, Assign task, More | Rewrite | IA-06 |
| OO §5 profile tabs (OO:184) | Overview, Activity, Tasks, Messages, Sessions, Knowledge, Processes, Role | Overview, Work, Conversations, Activity, Notes and memory | Rewrite (conflict, §8 #4) | IA-06 |
| OO §6 Home (OO:207) | Needs you, Working now, Latest (top 8) | Needs you (top 5), Projects, Happening now, Recent activity | Rewrite | IA-04 |
| OO §7.1 Requests (OO:243) | Hire requests on Agents › Requests tab | Hire requests are Inbox **Approvals** | Moves to Inbox | IA-09, IA-10, IA-11 |
| PROF-1 (OO:288) | Profile and quick view show runtime, messaging name, office, status | Setup summary on profile; full fields in Agent setup | Rewrite | IA-06 (AC-IA06-3), IA-07 |
| PROF-2 (OO:289) | Open session adapts to runtime | Where Open session lives (profile More? Agent setup?) | check | IA-06, IA-07 |
| PROF-3 (OO:290) | Chart cards: dot and word, no runtime | Presence pill/dot with word; v1.0 unknown shows no dot | Rewrite | IA-08, IA-02 |
| PROF-4 (OO:291) | Edit title, focus, runtime, home in profile Settings | Edit in Agent setup detail | Moves to Org settings › Agent setup | IA-07 |
| LIVE-1 (OO:294) | Activity: filter team, member, project, kind; Decisions hidden by default | Filters All · Work · Conversations · Decisions · Runs | Rewrite (check Decisions default) | IA-12 |
| LIVE-3 (OO:296) | Reply only when owner is a recipient; else read-only | Confirmed by D13, plus the note copy | Unchanged-but-relabel | D13 |
| LIVE-4 (OO:297) | Home: Needs you, Working now, latest 8 Activity rows | Needs you, Projects, Happening now, Recent activity | Rewrite | IA-04 |
| LIVE-6 (OO:299) | Status change on card within 2 s | Presence; real only once the office reports runs | Unchanged-but-relabel | IA-08, IA-15 |
| IMP-1 (OO:275) | Import from Settings › Team | No "Team" settings section | check | IA-03 |
| HRL-2 (OO:328) | HR lead in Agents roster's Built-in group | Agent setup list or Members? | check | IA-02, IA-07 |
| APPR-1 (OO:317) | Roster and quick view show "Approves hires" | Agent setup / profile | Unchanged-but-relabel (check) | IA-07 |
| APPR-5 (OO:321) | Owner sees approved hire on Home | Home Recent activity | Unchanged-but-relabel | IA-04 |
| APPR-6 (OO:322) | Owner approves any proposal; >1 day shown to owner | Inbox Approvals | Moves to Inbox | IA-11 |
| DEAL-3 (OO:336) | Paused shows Paused on card | Paused is gray (D8) | Unchanged-but-relabel | IA-08 |
| DEAL-5 (OO:338) | "Show paused" in chart toolbar | Members filter or chart toolbar? | check | IA-02 |
| OO §7 Add an existing agent (OO:225) | Hire panel tab "Add an existing agent", button **Add agent** | "Hire" wording (D14); tab name check | Rewrite (check) | IA-07 (AC-IA07-3) |
| OT decision 10 (OT:67) | Org chart nav item with Teams \| People \| Reporting lines; Offices own nav item | Teams · Members · Org chart; Offices in Org settings | Superseded | IA-01, IA-02, IA-03 |
| OT §4 switcher (OT:229) | Footer tile · name · office · dropdown with Manage offices | Footer per AC-IA01-2 | Rewrite | IA-01 |
| OT §6.1 (OT:349) | Offices screen, "nav: Offices" | `/settings/offices`, screen unchanged | Moves to Org settings › Offices | IA-03 (AC-IA03-2) |
| OT Flow 4 (OT:462-464) | Nav Org chart, pill tabs, office filter, toolbar | Teams index, team pages, Members, Org chart tree | Rewrite | IA-02, IA-05 |
| OT §7.0 (OT:468-476) | Agent square robot tile vs person circle | Round avatars (G70 F-5); kind in words | Rewrite (check) | IA-02 |
| OT §7.1 / decision 15 (OT:72, 488) | Owner bar above every view | Owner card on Teams index and root of Org chart; never a member row | Rewrite | IA-02, IA-05 (D17) |
| TEAM-1 (OT:731) | Teams view: ownership tree of team cards | Teams index: divided rows; team page | Rewrite (check hierarchy) | IA-05 (AC-IA05-1) |
| TEAM-10 (OT:740) | All three views keyboard-usable, each with List view | Three views gone | Rewrite (check) | IA-02 |
| TEAM-11 (OT:741) | Three tabs Teams, People, Reporting lines | Teams · Members · Org chart switch | Superseded | IA-02 (AC-IA02-1) |
| TEAM-12 (OT:742) | Agents square tile, people round; SR says kind | Kind in words, visible in rows | Rewrite (check) | IA-02 (AC-IA02-2) |
| PEOPLE-1 (OT:748) | People view: tree with only people | Members filter "People" | Superseded | IA-02 |
| PEOPLE-3 (OT:750) | **Add person** from toolbar, team menu, picker | "Invite a person" (D14) | Rewrite (check wording, §8 #10) | IA-05 |
| REPORT-1 (OT:758) | Reporting lines view, owner at root | Becomes the Org chart vertical tree | Unchanged-but-relabel | IA-02 (AC-IA02-3) |
| OWNER-1 (OT:765) | Owner shown once above chart in every view | Teams index card + chart root + "Reports to You" + footer | Rewrite | IA-02, IA-05 (D17) |
| OFC-5 (OT:720) | Join request raises a Needs-you item | Which Inbox kind? (Approvals?) | check (§8 #1) | IA-10 |
| OFC-10 (OT:725) | Unplaced agents: Needs-you item until handled | Not a D11 kind | check (§8 #1) | IA-10 |
| PS decision 3 (PS:19) | Health computed from rules (+ Reporter run) | D15: health only when set on the project; At risk red + icon | Rewrite (conflict, §8 #6) | IA-04 (AC-IA04-3) |
| PS Flow 3 / STATUS-9 (PS:64-73, 91) | Home › Projects status table: health, features, cost, waiting | Home Projects outlined cards: done/total, open/blocked, avatars | Rewrite | IA-04 |
| STATUS-8 (PS:90) | Change of health also appears in Needs you | Not a D11 kind | check (§8 #1) | IA-10 |
| STATUS-1 (PS:83) | Status page = project Overview tab | Overview: goal, status, progress, project's Needs you, activity | Unchanged-but-relabel (check) | — (no story) |
| RUN-1 (RM:88) | **Started by a person**; mode set in profile › Settings › How it runs | **With you** presence (WithYou); How it runs in Agent setup | Rewrite | IA-08, IA-07 (RV:180) |
| RUN-2 (RM:89) | Asleep with wake time; Offline for always-on | Same words, shown only once the office reports (D12) | Unchanged-but-relabel | IA-08, IA-15 |
| RUN-5 (RM:92) | Each wake is one row on the Runs tab and in Activity | No Runs tab; Activity tab includes runs | Rewrite | IA-12 |
| RUN-9 (RM:96) | Missing runtime raises a Needs-you item | Inbox Problems (run error) | Unchanged-but-relabel | IA-10 |
| RUN-13 (RM:100) | How it runs shows recent runs and cost | In Agent setup | Moves to Org settings › Agent setup | IA-07 |
| RM Home (RM:43) | Home › Working now with trigger | Happening now | Unchanged-but-relabel | IA-04 |
| RO Flow 1 / ROLE-5 (RO:35, 66) | Agents › Roles tab; Profile › Role tab | Org settings › Roles ("Not in this release"); no Role tab on profile | Moves to Org settings › Roles (check Role tab) | IA-03 |
| ROLE-9 (RO:70) | Change role in agent Settings | Agent setup | Moves to Agent setup | IA-07 |
| MOD-1 (MO:70) | Settings › Models; Agents › Roles › Models; Profile › Settings › Models | No Models section in IA settings list | check | IA-03, IA-07 |
| PC U25 / §2 (PC:66, 168) | Processes is its own nav item | Org settings › Processes, hidden in v1.0 | Superseded | IA-03 (AC-IA03-3) |
| PC profile Processes tab (PC:66) | Processes tab on each agent profile | Not in IA-06 tabs | check (§8 #4) | IA-06 |
| PROC-25 (PC:149) | Escalation in Needs you: options, Recommended, Reply | Inbox **Decisions** | Moves to Inbox | IA-09, IA-10 |
| PROC-17 (PC:141) | Process-load warning in Needs you after 1 h | Not a D11 kind | check (§8 #1) | IA-10 |
| WI decision 2 (WI:61) | Nav: Home · Chat · Org chart · Projects · Work · Knowledge · … · Offices · Settings | IA-01 nav | Rewrite | IA-01 |
| PRJ-1 (WI:470) | Project tabs: Overview, Features, Tasks, Bugs, Debt, Docs, Insights, Settings | PRD: Overview · Work · Team · Activity · Conversations | Rewrite (conflict, §8 #7) | — (no story) |
| WORK-3 (WI:567) | Work nav badge counts triage, verify, blocked | Only Inbox has a badge | Superseded (check) | IA-01 |
| TRI-5, FIX-4 (WI:516, 533) | Critical bug / verify request in Needs you | Not a D11 kind | check (§8 #1) | IA-10 |
| CI Insights nav (CI:40) | Insights is an org-wide nav item | No place in IA nav | check (§8 #7) | — |
| BUD-1 (CI:137) | 100% budget: Needs-you item with "Dismiss for this month" | Not a D11 kind; D11 "nothing else is dismissable" | check (conflict, §8 #1) | IA-10 |
| DEC-5 (DT:335) | Proposed ADRs in Needs you | Decisions? (D11 Decisions = escalations) | check (§8 #1) | IA-10 |
| EXN-2, EXN-3 (DT:379-380) | Exceptions: Approve/Reject; Keep/Renew/Revoke in Needs you | Approvals? not listed in D11 | check (§8 #1) | IA-10 |
| RQS-7 (FR:118) | Scoped requests and questions in Home › Needs you | Inbox (Questions fit; scoped requests: check) | Moves to Inbox (check) | IA-09 |
| RE retro (RE:49) | "Retro ready" Needs-you item | Not a D11 kind | check (§8 #1) | IA-10 |
| MOD-7 (MO:76) | Missing model raises a Needs-you item | Problems? | check (§8 #1) | IA-10 |
| DF 2.1 (DF:52) | Home shows Working now and 8 Activity rows (LIVE-4) | New Home sections | Rewrite | IA-04 |
| DF 2.3 (DF:54) | Card status word changes with presence | v1.0 shows Status unknown until IA-15 | Rewrite (check timing) | IA-15 |
| DF 1.6 (DF:42) | Three quick views | Profile pages | check | IA-06 |

---

## 3. BUG-70 rows (`bug-70-per-screen-gaps.md`)

Rows not listed are **still wanted as is** (§6).

| ID | Today's gap (short) | Under IA v2 | Note |
| --- | --- | --- | --- |
| SH-1 (G70:17) | 10 nav items → 7, Agents first | Superseded by IA-01 | Nav rev 5 (D19) |
| SH-2 (G70:18) | Org chart as tab of Agents | Rewrite for Teams › Org chart | `/org-chart` redirects; Teams current (AC-IA01-3, -4) |
| SH-3 (G70:19) | Offices under Settings + footer link | Rewrite for Org settings › Offices | `/settings/offices` (IA-03) |
| SH-4 (G70:20) | Processes hidden | Rewrite for Org settings › Processes | Unavailable in production (AC-IA03-3) |
| SH-5 (G70:21) | Agents **is** Home + Activity column | Superseded by IA-04 | Home is the landing (D1) |
| SH-6 (G70:22) | Projects section A–Z, cap 8 | Rewrite for nav rev 5 | 5 "Recent" + View all (AC-IA01-1, RV:142); S3 data path stays |
| SH-7 (G70:23) | Footer: two links | Still wanted, small rewrite | Office line adds "· Online", links `/settings/offices` (AC-IA01-2) |
| SH-10 (G70:26) | Amber badge on Chat only | Rewrite for Inbox | Badge on Inbox, amber only when > 0 |
| SH-12 (G70:28) | Search "Search agents…" filters Agents | Superseded by IA-13 | Grouped results (D16) |
| SH-13 (G70:29) | Add agent primary in top bar | Superseded by New ▾ › Hire an agent | Outline (RV:100); no story owns New ▾ (check) |
| SH-16 (G70:32) | Rail: logo switch, hide Projects | Still wanted | Also hide the Teams list (check) |
| F-2 (G70:43) | StatusPill `tone` prop | Still wanted | Agent tone map: Paused gray (D8), presence tones from IA-08 |
| F-4 (G70:45) | StatusDot gray-400 + pulse | Rewrite for IA-08 | Working indigo-400; Paused pause mark; no dot for unknown (RV:109-117) |
| F-5 (G70:46) | One round avatar | Still wanted | Kind in words alongside (principle 4) |
| F-6 (G70:47) | EmptyState primitive | Still wanted, now critical | Every IA shell depends on it (AC-IA04-2, AC-IA03-3) |
| F-11 (G70:52) | shadcn Tabs | Still wanted | Teams switch, profile tabs, settings sub-nav |
| AG-1 (G70:73) | Agents \| Org chart bar, list, Activity column | Rewrite for Teams › Members | IA-02 |
| AG-2 (G70:74) | Row meta: teams · manager · office · runtime · mode | Rewrite (split) | Members row (kind, presence, Working on); runtime/mode to Agent setup (IA-07) |
| AG-3 (G70:75) | 390 px roster | Rewrite for Members / Agent setup | Setup table → rows below 640 (RV M3) |
| AG-4 (G70:76) | Filter All/Active/Paused/Former | Rewrite | All · People · Agents (AC-IA02-2) |
| AG-5 (G70:77) | Sort by name/status/team | Rewrite for Members (check) | Not in IA ACs |
| AG-6 (G70:78) | Pill tone, paused amber | Superseded by IA-08 | Paused gray |
| AG-7 (G70:79) | Messaging name in mono | Rewrite for Agent setup (check) | — |
| AG-8 (G70:80) | Activity column (deferred) | Dropped | Home Recent activity (IA-04) and Activity (IA-12) |
| AG-9 (G70:81) | Page-level Add agent button | Rewrite | "Hire an agent" in Agent setup; "Hire or invite ▾" on team page (D14) |
| AG-10 (G70:82) | Empty "Add your first agent" | Rewrite copy | "Hire" (D14) |
| AG-11 (G70:83) | Sample data pill | Still wanted | On every new page |
| AD-1 (G70:89) | Agent profile (Q3: no) | Superseded by IA-06 | `/members/:id`; my Q3 withdrawn (RV:15) |
| AD-2 (G70:90) | Edit + kebab actions | Rewrite for IA-06 / IA-07 | Message, Assign task, More (Pause/Resume, Edit, Offboard) |
| AD-3 (G70:91) | One pill, drop "Now —" | Rewrite for IA-06 | One pill + one line (RV M2) |
| OC-1 (G70:97) | Agents \| Org chart tabs; inner Teams/People/Reporting switch | Rewrite for Teams › Org chart | Inner switch goes (RV:140) |
| OC-2 (G70:98) | Remove owner card; owner as root | Rewrite (compatible) | Owner card at the root (AC-IA02-3, D17); check "card" vs plain row |
| OC-3 (G70:99) | Pill view switch | Superseded by IA-02 | Tree replaces the three views |
| OC-4 (G70:100) | Toolbar: New team, Add person, Add agent, View ▾ | Rewrite | Add person → Invite a person; Add agent → Hire. Does team editing stay on the chart? check |
| OC-6 (G70:102) | Team cards style | Rewrite (check) | If the tree is members-only, team cards move to the Teams index as rows |
| OC-7 (G70:103) | Selected-member strip | Rewrite | Node links to profile (AC-IA06-5) |
| OC-8 (G70:104) | Search members field | Still wanted (check) | Top-bar search also finds members |
| PR-2 (G70:113) | Drop disabled New project | Rewrite (check) | Home empty state offers Create project (AC-IA04-2), New ▾ has Project |
| PR-4 (G70:115) | Row: name, teams, lead | Rewrite (check) | IA list: status, progress, open work, team (PRD:64); no story |
| PD-2 (G70:124) | "← Projects · Overview · Tasks · Bugs" | Rewrite | Overview · Work · Team · Activity · Conversations; no story |
| PD-3 (G70:125) | Show only Overview, Tasks, Bugs | Rewrite | As PD-2 |
| WK-3 (G70:137) | Saved views as Show ▾ | Rewrite | Needs me · Mine · By project · By assignee; List \| Board toggle (RV:91); no story |
| NB-1 (G70:149) | Sessions/Activity/Settings placeholders, "Back to agents" | Rewrite | Sessions redirects; Activity and Settings get shells (IA-03, IA-12); "Back to Home" |
| NB-2 (G70:150) | Settings lists Offices | Superseded by IA-03 | Sub-nav |
| NB-3 (G70:151) | Activity page | Superseded by IA-12 | — |
| CH-1 (G70:157) | Emoji as UI | Still wanted | Now E-01 (A-CH1) |
| CH-4 (G70:160) | Conversation sidebar labels | Still wanted, plus | Add "Between agents" group (D8) |
| CH-5 (G70:161) | Counts; amber only Needs you | Still wanted | Chat's Needs you links to Inbox (AC-IA09-3) |
| OF-6 (G70:181) | "← Settings · Offices" back item | Rewrite for IA-03 | Settings sub-nav instead |
| PC-1 (G70:188) | Processes not linked | Superseded by IA-03 | Org settings › Processes, hidden |
| UN-1 (G70:194) | Unavailable page nav list | Still wanted | Its nav list follows rev 5 |
| UN-2 (G70:195) | 404 "Back to agents" | Rewrite copy | "Back to Home" |
| DL-5 (G70:205) | Add agent sheet, primary "Add agent" | Rewrite copy | "Hire" (D14) |

---

## 4. Rowan's nav freeze rev 4 and slice order

### 4a. Freeze items (`bug70-nav-freeze.md`)

| Item | Line | Under IA v2 | How |
| --- | --- | --- | --- |
| (a)1 NAV 7 items | NF:33 | Superseded | Nav rev 5 (D19, IA-01) |
| (a)2 `pageFor` route table, `navFor` | NF:34-37 | Carried over | Same table, new items (TR:16; ST IA-01 "reuses S1's pageFor") |
| (a)3 Routes: no moves, no redirects; `/settings` with one Offices link | NF:38-41 | Superseded | Redirects (AC-IA01-3); `/settings` sub-nav (IA-03). `/processes` unavailable carries |
| (a)4 `aria-current` page/true rule, focus helpers | NF:42-45 | Carried over | New mappings: profile → Teams, `/settings/*` → Org settings (AC-IA01-4) |
| (a)5 Footer: switcher + office line | NF:46-49 | Carried over, small change | "· Online", links `/settings/offices` (AC-IA01-2) |
| (a)6 Projects section (S3) | NF:50-58 | Data path carried; presentation re-scoped | 5 Recent + View all; order by recent activity needs a source (check); add Teams list (5) |
| (a)7 S1 tests | NF:59-65 | Rewritten | Counts, SECTIONS, redirects, new current-item matrix |
| (b)8 `AgentsTabs` (S2) | NF:69-74 | Carried over | Teams view switch: Teams · Members · Org chart, new routes, still not a tablist (AC-IA02-1) |
| (b)9 Agents Home (S4) | NF:75-78 | Superseded | Home is the landing; rows split into Members and Agent setup |
| (b)10 Order and ownership | NF:79-83 | Superseded | ada re-sequences (ST "Suggested order") |
| (b)11 Gates per slice | NF:84 | Carried over | Process, unchanged |

### 4b. Slices (`bug70-slice-order.md`)

| Slice | Line | Under IA v2 | How |
| --- | --- | --- | --- |
| A-QADD (`?q`/`?add=1` on `/agents`) | SO:11 | Carried, retarget (check) | `/agents` redirects to `/teams/members`; the hire intent moves to Agent setup |
| S2 | SO:12 | Carried (merged v1 `673e1cb7`, validated) | Relabelled as IA-02; fix my S3 from validation (focus after the bar, `validation-s2-agents-views.md`) |
| J-BUG76 | SO:13 | Done (`2efce5d5`) | — |
| S1 | SO:14 | Re-scoped to nav rev 5 | = IA-01 (D19) |
| A-OC1 | SO:15 | Partly carried | 390 px and BUG-90 still wanted; toolbar "Add agent" → Hire; People-tab Fit (BUG-90): check, the tab goes |
| S3 | SO:16 | Data path carried (Tier A) | Feeds the 5 Recent projects (TR:17); presentation in IA-01 |
| S3b ("No projects yet", cap 8) | SO:157 | Superseded | Cap 5 + View all (AC-IA01-1) |
| A-CH1 | SO:17 | Carried | = E-01 |
| J-PILL | SO:18 | Carried | StatusDot colours from the presence table (RV:109-117); feeds IA-08 |
| A-OF1 | SO:19 | Carried as is | Offices screen unchanged (AC-IA03-2) |
| J-HEAD | SO:20 | Carried | Already waits on freeze rev 5 |
| S4 | SO:21 | Superseded | Row work rehomed to IA-02 (Members) and IA-07 (Agent setup); "divided rows at every width" (SO:148) still holds |
| J-TABS | SO:22 | Carried | Used by Teams switch, profile tabs, settings sub-nav |
| A-WK1 | SO:23 | Carried as is | — |
| J-EMPTY | SO:24 | Carried, re-scoped | No Sessions placeholder; EmptyState for Home/Inbox/Settings shells; "Back to Home" |
| A-AG2 | SO:25 | Superseded | Header/filter/sort rehomed to IA-02 / IA-07 |
| J-TOKENS | SO:26 | Carried as is | — |
| A-WK2 | SO:27 | Carried; Work views change | Needs me, By assignee (no story: check) |
| J-AVATAR | SO:28 | Carried as is | — |
| A-PD1 | SO:29 | Re-scoped (check) | Project tabs per PRD §6; no story |
| J-FORMS | SO:30 | Carried as is | — |
| A-PR1 | SO:31 | Carried (check row content) | PRD:64 row fields |
| J-ICONS | SO:32 | Carried as is | Heroicons (E2) |
| A-PD2 | SO:33 | Carried as is | — |
| J-POLISH | SO:34 | Carried as is | — |
| J-TOP | SO:35 | Superseded | Search → IA-13; Add agent → New ▾ (no story: check) |
| A-CH2 | SO:36 | Carried, plus | Between agents group; presence words via IA-08 |
| A-AD1 | SO:37 | Superseded (was dropped, SO:175) | Replaced by IA-06 |
| A-OC3 | SO:38 | Superseded | Folds into IA-06 (AC-IA06-5) |
| A-OC2 | SO:39 | Re-scoped | For the Org chart tree; `AgentsTabs` → Teams switch |
| A-OF2 | SO:40 | Re-scoped | Back item → settings sub-nav (IA-03); icons and tone map still wanted |
| A-ON1, A-DL1 | SO:41-42 | Carried as is | — |
| A-DL2 | SO:43 | Carried, copy change | Sticky footer "Hire", not "Add agent" |
| J-BACK | SO:167 | Carried as is | — |
| A-CH3 | SO:172 | Carried as is | — |
| J-SIDEW | SO:179 | Carried as is | — |

**Conflict rulings in SO:** C1 (SO:91) carried · C2 (SO:94) superseded (cap 5) · C3 (SO:99) superseded (no `/agents?q`) · C4 (SO:102) superseded · C5 (SO:104) carried · C6 (SO:106) carried ("You · Owner", AC-IA01-2) · C7 (SO:108) carried · C8 (SO:113) carried · C9, C10 carried · C11 (SO:123) superseded (`/members/:id`, Rowan's rev 5) · C12 (SO:125) superseded.
**My answers in SO:** Q1 row spec (SO:147-152) partly carried (rows yes, fields change) · Q3 "no profile pages" (SO:155) reversed · SH-12 `/agents?q=` (SO:163) superseded · others carried.

---

## 5. Rowan's technical read: one mismatch

TR:19 redirects `/agents` → `/teams/people`. D19 (PRD:220) and AC-IA01-3 say `/teams/members`. **D19 wins.**

---

## 6. What still stands

- **Look slices, unchanged:** J-PILL (tones; dot colours follow IA-08), J-TOKENS, J-FORMS, J-ICONS, J-AVATAR, J-HEAD, J-TABS, J-POLISH, J-BACK, J-SIDEW, and J-EMPTY (re-scoped). PRD:183 confirms the pills, tokens, icons and forms are unaffected.
- **Screen slices, unchanged:** A-OF1 (Offices, incl. BUG-81/82/91/92), A-WK1, A-PD2, A-ON1, A-DL1, A-DL2 (copy only), A-CH1 (= E-01), A-CH2, A-CH3.
- **Validated work that carries:** S2 (`673e1cb7`, `validation-s2-agents-views.md`), now IA-02's switch; J-BUG76 (`2efce5d5`); FIXED rows PR-1, PR-3, PD-1, CH-11; S3's data path design (NF:53-57).
- **BUG-70 rows still wanted as is:** SH-8, SH-9, SH-11, SH-14, SH-15, SH-17–SH-20; F-1, F-3, F-7–F-10; ON-1–ON-8; OC-5, OC-9, OC-10; PR-5, PR-6; PD-4–PD-7; WK-1, WK-2, WK-4–WK-9; CH-2, CH-3, CH-6–CH-14; OF-1–OF-5, OF-7; DL-1–DL-4, DL-6, DL-7.
- **Specs that stand:** V1 Hire steps (§3.6, HIRE-*, apart from "Hire" entry points), Assign task (ASSIGN-*), Session panel (§3.8, apart from the status words), First run (FIRST-*); OO Import (apart from IMP-1's location), STAFF, MOVE, DEAL (apart from DEAL-5), APPR-2–4, 7, 8; OT menu bar, open/join and Offices (MENU, OPEN, JOIN, OFC apart from OFC-5/10); CH-2, 4, 6–11, 13–17; the rest of WI, RO, MO, PC, CI, DT, FR, RE beyond the rows above.

---

## 7. Count

Counted by row, by each row's disposition. "Rewrite (check)" counts as Rewrite; "Re-scoped" counts as Rewrite; "Moves" and "Relabel" count as carried over.

| Table | Rows | Rewrite | Superseded / dropped | Moves | Relabel only | Carried / still wanted | check only |
| --- | --- | --- | --- | --- | --- | --- | --- |
| §1a v1 criteria | 58 | 24 | 11 | 4 | 14 | — | 5 |
| §1b v1 section text | 24 | 17 | 4 | 0 | 1 | — | 2 |
| §2 other specs | 78 | 33 | 5 | 9 | 13 | — | 18 |
| §3 BUG-70 rows | 53 | 31 | 11 | 0 | 0 | 11 | 0 |
| §4a freeze items | 11 | 2 | 4 | 0 | 0 | 5 | 0 |
| §4b slices | 36 | 5 | 6 | 0 | 0 | 25 | 0 |
| **Total** | **260** | **112** | **41** | **13** | **28** | **41** | **25** |

**112 rewritten, 41 superseded, 82 carried over** (13 moves + 28 relabels + 41 carried or still wanted), and 25 rows that need an answer first ("check"). Several §1a and §2 rows group more than one ID (e.g. SESS-5, 6, 11, 13, 20).

---

## 8. Conflicts that need a decision

1. **Needs-you sources outside D11's kinds.** My specs send more to Needs you than D11 maps: budget 100% (BUD-1, with "Dismiss for this month", against D11's "nothing else is dismissable"), proposed ADRs (DEC-5), exceptions (EXN-2/3), Critical bugs and fix verification (TRI-5, FIX-4), scoped requests (RQS-7), retro ready (RE:49), health change (STATUS-8), office join requests and unplaced agents (OFC-5, OFC-10), process-load warning (PROC-17), missing model (MOD-7). Each needs a tab (Approvals / Questions / Decisions / Problems) and a clearing rule, or an explicit "not in v1.0". **Emery.**
2. **Clearing rule for permissions.** V1 §2.4 clears when the agent continues (answered anywhere, V1:113); D11 clears Approvals "only when acted on". Answering in a terminal or another client must still clear the item (AC-IA10-2's "acting anywhere" suggests yes). Also HOME-4 vs D11 on dismissing errors. **Emery + Rowan.**
3. **Owner replies in agent threads.** CH decision 5 and CHAT-12 let the owner reply anywhere; LIVE-3 and D13 make agent-to-agent threads read-only. D13 wins; I rewrite CHAT-12. No IA story carries the note copy. **ada: which slice (A-CH2?).**
4. **Profile tabs.** OO:184 adds Tasks, Messages, Sessions, Knowledge, Processes and Role tabs; RO:35 and PC:66 rely on Role and Processes tabs. IA-06 has Overview, Work, Conversations, Activity, Notes and memory. **Proposal:** Role and Processes go to the agent's Agent setup detail. **Emery.**
5. **Organization settings contents.** IA-03's list has no home for V1's Workspace, Claude Code, Notifications, Appearance and About, nor OO's Team › Import and Hiring, MO's Models, CI's Costs, PC's Autonomy, DT's Agent guidance, CH's Bridges. Personal preferences (Appearance, Notifications) aren't organization settings. AC-IA03-3 shows Updates as "Not in this release", while SET-3 specs restart-to-update. **Emery.**
6. **Project health source.** PS decision 3 computes health from rules (and STATUS-9 is a table on Home); D15 shows health "only when set on the project" and AC-IA04-3 uses cards. Is health set by hand, computed by PS's rules, or both? **Emery.**
7. **Pages in the PRD with no story:** project page tabs (PRD:64, 120-124; PRJ-1, PD-2, PD-3), Work views Needs me and By assignee (PRD:65; WK-3), the top-bar **New ▾** menu (PRD:74; SH-13, SHELL-2), and where Insights (CI:40) and Knowledge (V1 §3.11) live. **Emery to add stories or defer.**
8. **Org chart editing.** OT's chart edits teams (New team, Move team, Change manager, office filter, List view) across three views (TEAM-1, TEAM-10). IA-02 keeps one members tree, and IA-05 makes the Teams index flat rows, which loses the team hierarchy. Where do team hierarchy and editing live? **Me, with Rowan.**
9. **Avatar shape.** OT §7.0 and TEAM-12 tell agents and people apart by square vs round; G70 F-5 and IA principle 4 use round avatars with the kind in words. I'll rewrite TEAM-12 to words-first. **Mine; noting for Avery.**
10. **"Invite a person."** D14 says Invite; person sign-in is out of v1.0 (PRD:198). Inviting implies an account. "Add a person" may be more truthful in v1.0. **Emery.**
11. **Presence truth vs tests.** D12 shows only Paused and Status unknown in v1.0. DF 2.3, LIVE-6, RUN-2 and ORG-7 expect live words. They can't pass until IA-15. **ada: sequence IA-15 or mark those checks deferred.**
12. **"Recent" projects order.** The sidebar's 5 Recent need a last-activity time per project; S3's data path orders A–Z (NF:52). **Rowan.**
