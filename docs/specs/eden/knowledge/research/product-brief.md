# Product brief · Agency desktop app

Owner: Eden (`ac-product-designer-eden`) · Status: **iteration 1, draft for the user** · 2026-11-07
Source: the user's org-chart mockup (`knowledge/prototypes/reference/mockup-v0.png` – the image the user shared on 2026-11-07) and the user's description of the product.

Labels: **[F]** fact (the user said it or the repo shows it) · **[A]** assumption (mine, to confirm) · **[O]** opinion or recommendation.

## 1. What the product is

- **[F]** A **desktop application built with Tauri**, which people install and which auto-updates. It is **not** a SaaS, a web app or a CLI.
- **[F]** It **manages AI agents that run on the user's own machine**: the organization, projects, roles and agents this repository already models (HR agent, `hr.py`, `bin/agent`, goals mod), with a visual front end.
- **[F]** **Now:** everything runs on **one machine**. A single executable opens the visual app.
- **[F]** **Later:** a **headless daemon** installs on other machines (for example a headless PC). The app connects to several servers and manages the agents on them as part of the same organization.
- **[O]** The design treats **"this computer" as the first machine** from day one, so that adding remote machines later is "another entry in the list" and not a redesign.

## 2. Who uses it

| Persona | Who | Primary jobs | Phase |
| --- | --- | --- | --- |
| **Owner-operator** (primary) | A technical founder or consultant running an AI organization on their own computer. Knows Claude Code. Today this is the user. | See what agents are doing and what needs them; hire agents; give work and follow it to done; open a session to steer an agent; read outputs, memory and journals. | v1 |
| **Org admin** | Someone running the org across several machines (a home lab or the company's headless PCs). | Add a machine, check it's healthy, place or move agents, keep versions in sync. | v2 |
| **Human teammate** | A person in the org who works alongside the agents (the mockup's "Operations Manager (Human)"). | Get and hand off tasks, review agents' work. | Later, **[A]** to confirm |

**[F]** Agents also read and write the same files (backlogs, journals, memory). The app must not break those file conventions. Agents are a second kind of "user" of the data.

## 3. Top jobs to be done (v1)

1. **"What needs me right now?"** Agents waiting for a permission, a question or a decision, blocked tasks, and goals that paused.
2. **"What is everyone doing?"** Which agents are running, idle or offline, and on what task or goal.
3. **"Hire someone for this."** Describe a need in plain words. HR suggests a role and an agent, I review it, and they're provisioned.
4. **"Get this done."** Give an agent a task, see its goal, and follow it until it's done or blocked.
5. **"Let me talk to them."** Open, resume or stop an agent's session from the app.
6. **"Who owns what?"** Org chart, projects, reporting lines and peers.
7. **"What did they produce?"** Read an agent's backlog, journal, memory and knowledge files.

## 4. Domain model: the mockup vs. what exists today

| Concept | Today in the repo **[F]** | In the mockup | Design decision **[O]** |
| --- | --- | --- | --- |
| Organization | The repository (one per checkout) | "Virtual Consulting Co.", with a switcher at the bottom left | **Organization** = one workspace. The switcher stays, and later also shows which machines it spans. |
| Project | `projects/<key>/`, with a brief and a team roster. Every agent belongs to exactly one. | A cross-cutting list with progress % | **Project** is first-class: its own page with brief, team and backlog roll-up. It's also a **lens** on the org chart. |
| Department | `department` on each role (Design, Product, Engineering, …) | "Divisions" with color tiles | Call it **Department**, matching the role specs. It groups the org chart and isn't a separate entity to manage. |
| Role | `hr/roles/*.toml` (catalog) | "Roles & Headcount" tab; "Create Custom Role" | **Role** = a job description in the catalog. "Create custom role" = HR writes a new role spec. |
| Agent | Home folder, `agent.json` (name, title, focus, reports_to, status), CLAUDE.md | A card with avatar, title and "Active" | **Agent**, with a runtime status (see §6), not just "Active". |
| Reporting line | `reports_to` = the user or another agent | HR Agent at the top, with everyone under it | **The owner (you) is the root.** HR is a staff function beside the root, not everyone's manager. See finding F1. |
| HR | A special agent (`.claude/agents/hr.md`) | "HR Agent, Head of People & Org" | Keep HR visible and prominent as the engine behind **Hire**. |
| Peers | Same role, same project | — | Shown on the agent profile. |
| Session | A Claude Code session named after the agent (`bin/agent`) | "Message" button | **Session**: open, resume or stop it from the agent. This is where the user actually talks to an agent. |
| Goal | `CreateGoal` per delivery task, checked by an evaluator | — | Shown on the agent and the task: the condition, its checks and whether it's paused. |
| Task | Rows in each agent's `tasks/BACKLOG.md` (todo / in-progress / blocked / done) | "Tasks & Assignments" with a badge | **Task** = a backlog row. The Tasks screen is a roll-up of all backlogs. |
| Machine | — (single machine today) | — | **Machine**: "This computer" in v1, remote machines in v2. Every agent runs on one machine. |
| Human member | Only "the user" | Humans in the chart | **[A]** Out of v1. The owner is the only human. |

## 5. UX review of the mockup (heuristic evaluation)

It's a strong starting point: a clear left nav, an org chart as the hero, ⌘K search, a contextual hire panel and a good agent profile. These are the findings to fix as we turn it into the real product.

| # | Severity | Finding | Evidence | Recommended fix |
| --- | --- | --- | --- | --- |
| F1 | High | The hierarchy misstates who reports to whom. HR sits at the root as "Head of People & Org", but agents report to the user. | Mockup chart; `reports_to: user` in `agent.json` | Make the owner the root node and show HR as a staff node. Draw reporting lines from `reports_to`. |
| F2 | **Critical** | There's no live runtime state. Every agent shows "Active", and nothing says whether an agent is working, idle, offline or **waiting for you** (permission prompt, question, paused goal). For a local agent manager this is the core job. | Every card says "Active". No session or goal anywhere. | Add a status model (§6) and a **"Needs you"** inbox on Home, with a badge in the nav. Wire the bell to the same inbox. |
| F3 | High | Divisions and projects compete as the main structure, and the real system is organized by project. | Org chart by division; projects only at the bottom | Use **Department** as the grouping and **Project** as the lens (a "View" dropdown: All projects / a project). |
| F4 | Medium | The metrics have no data behind them ("92% on-time delivery", project % complete). | Workspace overview | Show only counts derived from real data: tasks by status, agents by status, sessions running, goals met. Add analytics later. |
| F5 | High (v2) / Low (v1) | There's no idea of machines, so remote daemons have no home. | — | Add a **Machine** attribute on each agent, a machine indicator in the org switcher, and a **Machines** screen in v2. |
| F6 | Medium | Hiring goes straight from suggestion to "Hire Agent" with no review of what will be created, and doesn't show that HR takes time to work. | Hire panel | Hire flow: **Describe → Suggestion → Review (name, project, reports to, focus, title, files to be created) → Provisioning (progress) → Done (open session)**. Matches `hr.py hire --dry-run` then hire. |
| F7 | Medium | The nav overlaps: Org Chart, Agents & People, Organizations and Hire / Add Role all sit side by side. | Left nav | v1 nav: **Home · Org chart · Agents · Projects · Tasks · Knowledge · Settings**, with **Hire** as a primary action (button and ⌘K), not a nav item. |
| F8 | Medium | Accessibility: status uses a color dot only, small text is light purple on lavender, and there's a lot of 10–11 px text. | Cards and chips | Pair every status with an icon and a text label. 4.5:1 contrast. Minimum 12 px for meta text. Full keyboard navigation of the chart. |
| F9 | Low | "Message" on the agent profile is vague. Do you chat with it or open a session? | Agent profile | Rename it **Open session** (or **Resume**). The session opens in a panel inside the app. |
| F10 | Low | "Assign Task" doesn't say what it does. | Agent profile | Assigning adds a row to the agent's backlog and can start a session with a goal. Make that explicit in the dialog. |

## 6. Agent status model (the core of the app)

Every agent always shows exactly one of these statuses, as icon + label + color, never color alone:

| Status | Meaning | Source **[A]** (ada to confirm) |
| --- | --- | --- |
| **Needs you** | A session is waiting on the owner: a permission prompt, a question, a goal paused after repeated failed checks, or a crash. | Session or hook events |
| **Working** | A session is running and producing output. | Session process |
| **Idle** | A session is open but waiting for the next instruction. | Session process |
| **Offline** | No session is running. This is the normal resting state. | No process |
| **Unreachable** (v2) | The agent's machine can't be reached. | Machine connection |

Separately, the **task status** (todo / in-progress / blocked / done) comes from the backlog. A **blocked** task appears in "Needs you" only when it's waiting on the owner.

## 7. Proposed v1 scope (single machine) **[O]**, for the user and a future PM to confirm

**In v1:** Home ("Needs you" + running now + recent activity) · Org chart (by reporting line, grouped by department, filtered by project) · Agents (list + profile: Overview, Tasks, Sessions, Journal, Memory, Knowledge, Settings) · **Hire** flow (via HR) · Projects (list + page: brief, team, tasks) · Tasks (roll-up of all backlogs) · Session panel (open, resume, stop, live status) · Settings (workspace folder, Claude Code location, updates) · First run (pick or create a workspace, check Claude Code is installed and signed in).

**v2:** Machines (add a remote daemon, pair it, health, place agents) · multi-machine org switcher · analytics (sessions, tokens, cost, goals).

**Later:** human members · knowledge library across agents · templates for orgs and projects.

## 8. Open decisions (owner: the user, or a PM once hired)

1. **Humans in the org:** are human members in scope, and when?
2. **Sessions in the app:** embed the Claude Code terminal in the app, show a chat-like view, or both? (This is a UX decision with big architecture weight; ada to say what's feasible.)
3. **Remote model:** one organization spread over several machines, or one organization per machine that the app aggregates? (This shapes the org switcher.)
4. **Edits from the app:** can the owner edit backlogs, memory and briefs directly in the app, or only through agents?
5. ~~Product name~~ **Decided 2026-11-07 (for now):** the product is **Agency**, and the organization in our designs is **Agentic Org**. Brand (logo, voice) is still open.
6. **Platforms for v1:** macOS only, or also Windows and Linux?
