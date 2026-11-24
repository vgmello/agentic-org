---
id: doc-19
title: v1 UI Screens Build Plan
type: guide
created_date: '2026-11-21T00:16:00Z'
updated_date: '2026-11-21T00:16:00Z'
tags:
  - plans
  - ui
  - screens
---
# v1 UI screens: build plan

> [!NOTE] Review note (2026-11-21)
> Reviewed against `v1` and `docs/workstreams/ui.md`. Key status updates since this file was last edited: (1) `v1-ui` merged into `v1` (`v1-integration` is retired); (2) Step 3 Offices finished on `v1` (except 9 `SET-200` `fixme` tests at 960–1279 px / 200% text); (3) Step 4 Projects is **In progress** on `v1` (all screens landed except replacing the Work tab in Step 5), although the table below still reads `To do`; (4) The top review rule mentions Codex review cycles, which was superseded by the owner's 2026-11-19 Claude Opus review rule. Remaining steps are tracked in Backlog.md (`TASK-65`–`TASK-71`).

Branch `v1-ui`, started from `v1-integration`. Owner direction, 2026-11-18: build every
screen and interaction the specs call for that is not already built, with mock data. This
page is the list of screens and the order they are built in. Each row's Status is updated
as it lands.

Owner direction, later on 2026-11-18, replacing the earlier "review once at the end":
**the UI must be production ready except for the mock APIs**, and that includes its
structure and architecture (thin routes, domain modules owning their features, mocks only
under `e2e/fakes`, declared cross-area couplings). **Every feature gets a Codex review
cycle when it is done** (`.agents/skills/review-cycle`, `kit_codex.py start` → origin
validation → `final-review`), plus a fresh independent review, before it is merged into
`v1-ui`. Lint (`bun run lint`) is part of "green" alongside check, test, format and build.

## Where the work stands (2026-11-18, second session)

- Steps 0 and 1 are complete.
- The "UI work in progress" merges are repaired: type check, unit tests, format check and
  build pass; lint passes except nine unused names in the Chat route that belong to the
  unfinished Chat features below.
- An audit of the in-progress rows (against their specs) found most of them were never
  built: the APIs, types and mocks exist, the screens do not. Per row:
  - **Teams index** has gaps only (dead "Hire requests" link, team links reach a
    placeholder). **Members** and **Org chart** are still the old screens on the old
    `$lib/org` snapshot, with the wrong filters, zoom/fit tabs and an "Add an existing
    agent" sheet the spec forbids. **Team page, member profile, supervision/Run now,
    session log, sign-in access dialogs and Hire** are not built.
  - **Offices** works but its route is 772 lines and its code lives outside
    `modules/offices`. **Processes** is the old read-only view with its mock in `src`.
    **Agent setup, Roles, Members and access, Costs, Updates** are placeholders.
  - **Chat** misses pins, hand back, withdraw, check status, the live HR note, error
    with Retry; its route is 1,482 lines and its mock ships in `src`.
- Cross-cutting architecture to fix as each area is rebuilt: one shared `Resource` /
  `Loaded` state primitive (today duplicated in `members` and `settings`, and reached into
  by `teams` and offices); team and people dialogs in `src/lib/components` move into their
  modules; mocks leave `src` (`messaging/sample.ts`, `processes/{sample,world}.ts`,
  `copilot/api.ts`); routes get thin.
- Steps 4 to 9 have not started.

### Build order

Each item is one feature: built in its own worktree from `v1-ui`, Codex review cycle,
fresh review, merged with `--no-ff`. At most two are built at once, on disjoint modules.

1. Foundation: shared `Resource` / `Loaded` / confirm dialog / focus helpers.
2. Member profile with supervision, Run now, session log and sign-in access dialogs ·
   Team page.
3. Members directory · Hire (requests list, refused view, Allow hiring, wizard).
4. Org chart to the spec (one reporting tree, owner bar, Hand over ownership).
5. Settings: Agent setup · Roles.
6. Settings: Members and access, Costs, Updates · Processes (Standards, editor,
   development processes; mock out of `src`).
7. Offices route into `modules/offices` · Chat (missing features, module extraction, mock
   out of `src`).
8. Steps 4 to 8 in table order, two at a time where modules are disjoint.
9. Step 9.
- Each builder followed [the builder brief](doc-18%20-%20v1-UI-Builder-Brief-Historical.md): file ownership, the
  mock rules, the checks and the report format. The assignment for each step is its table
  below plus the specs named in the inventory of `docs/specs/`.
- Known loose ends from step 1: the navigation's Inbox count is loaded once and does not
  drop after an item is resolved; Home's empty Projects state has no Create project action
  until step 4 builds one; `?mock=` on the first URL opened selects mock states
  (`error`, `empty`, `slow` and others listed in `e2e/fakes/mock-mode.ts`).
- The New menu omits Project and Work item until step 4 provides those flows.

## Rules for this branch

- **Code layout stays as it is.** Routes are thin entry points in `src/routes`. Each domain
  lives in `src/lib/modules/<module>` with its types, state and `features/<feature>/`
  folders ([project structure](../design/doc-68%20-%20Design-v1-Project-Structure.md) section 4). Shared visual
  primitives live in `src/lib/components/ui`.
- **Mock data is JSON, and stays out of production builds.** Each module has an `api.ts`
  that declares the calls a screen needs; its production export refuses every call, as
  today. The mock for it is `e2e/fakes/modules/<module>.ts`, which answers from JSON files
  in `e2e/fakes/data/<module>/`. The dev server swaps it in by name. Replacing a mock with
  the real web API later means writing the production `api.ts`; no screen changes.
- **To see the screens:** in `apps/agentic-org-ui`, run `bun run dev:mock`, open
  `/welcome` and choose **Open**.
- **Design.** Builders always work as the principal designer
  (`.agent-kit/project-skills/principal-designer/`, the `principal-designer` agent). Product specs
  outrank it, above all `docs/specs/app-shell-ux.md` section 6a. Its UI/UX Pro Max companion
  skill is disabled (owner, 2026-11-18). Every UI feature also gets a fresh principal-designer
  UX review (the persona's review job and consistency audit) before its Codex cycle.
  When the screens are built, a principal-designer walkthrough goes through every built page in
  the running app (owner, 2026-11-18): quality, consistency across pages and the persona's audit,
  at the plan's widths and at 200% text. Its findings are fixed before step 9 is called done.
- **Checks kept green on every commit:** type check, unit tests, lint and build. Existing
  browser tests that pin the old routes and the old look are brought in line in the last
  step, not screen by screen.
- **Not built here:** Create organization and Sign in / Register / session handling (built
  or being built elsewhere), and everything a spec marks "not in v1" (project status
  reports, retrospectives and routines, run modes and schedules, tech-debt register,
  goals, private delegates, Following, planning backlog).

## Step 0 · Foundation

| Screen or part | Route | Status |
| --- | --- | --- |
| Mock composition found by module name, `dev:mock` script | | Done |
| Navigation in the spec's order: Home, Inbox, Projects (first five), Work, Chat, Activity, Teams (first five), Organization settings; rail and drawer | | Done |
| Footer account menu (Your preferences, Switch organization, About, Sign out) and office line | | Done |
| Top bar: Search field and New menu, `N` and `/` shortcuts | | Done; New omits Project and Work item until a flow exists outside a project page |
| Redirects that keep the query string: `/agents`, `/org-chart`, `/offices`, `/processes`, `/sessions` | | Done |
| Organization settings frame with its section navigation | `/settings/*` | Done |
| Existing pages moved: Members, Org chart, Offices, Processes | `/teams/members`, `/teams/org-chart`, `/settings/offices`, `/settings/processes` | Done (content unchanged) |
| Shared page parts: page header, tabs, empty / loading / error states, description list, field rows | | Done: see `apps/agentic-org-ui/src/lib/components/README-screens.md` |
| Honest placeholders ("isn't built yet", "Not in this release"); no "Coming soon" | | Done |

## Step 1 · Home, Inbox, Activity, Search, Preferences

| Screen | Route | Status |
| --- | --- | --- |
| Home: Needs you, Projects, Happening now, Recent activity, month line | `/home` | Done |
| Inbox: All, Approvals, Questions, Decisions, Problems, Handled; list and detail; reply, approve, deny | `/inbox` | Done |
| Activity: feed, filters, search, run rows, thread panel, "n new" | `/activity` | Done |
| Search results, grouped | `/search?q=` | Done |
| Your preferences: Keyboard, Time zone | `/preferences` | Done |
| About | account menu | Done in step 0 |

## Step 2 · Teams and people

| Screen | Route | Status |
| --- | --- | --- |
| Teams index: tree of teams, lead, counts, projects | `/teams` | Done (f7a4c963, cc8025ba); an open Delete dialog does not follow live changes yet |
| Members directory: people and agents, filter, Can sign in | `/teams/members` | Done (6a96a3a6) |
| Org chart: one reporting tree, owner bar, Hand over ownership | `/teams/org-chart` | Done (ecb129a9, 6f723cec) |
| Team page: header, members; tabs Work, Projects, Activity, Conversations, Processes | `/teams/[teamId]` | Done (f7a4c963, cc8025ba, 6f723cec) |
| Member profile: header and actions; tabs Overview, Work, Conversations, Activity, Runs, Processes, Role, Cost, Notes and memory | `/members/[memberId]` | In progress: built except the Cost tab, an "isn't built yet" placeholder until step 7 (0dbb0e02, 69c97894) |
| Supervision and execution view, attempt history, Run now | member profile | Done (0dbb0e02, 69c97894) |
| Session log viewer (messages, tool calls, files, usage) | `/members/[memberId]/runs/[runId]` | Done (0dbb0e02) |
| Sign-in access dialogs for a person: Invite, New link, Remove access | member profile | Done (0dbb0e02, 86cc9435) |
| Hire: refused view, requests list, Allow hiring switch | Teams | Done (34b65b5d) |

## Step 3 · Organization settings

| Screen | Route | Status |
| --- | --- | --- |
| General: name, workspace, runtime, recovery code row, administrator | `/settings` | Done |
| Offices: Add office, invitation, Approve a join, Remove, Rename, unplaced agents, outbox | `/settings/offices` | In progress (stopped part-way) |
| Agent setup: list and one agent's setup, health | `/settings/agents`, `/settings/agents/[memberId]` | Done (38e648de, ebb5d3b8) |
| Roles: list, role page, editor, New role | `/settings/roles`, `/settings/roles/[roleId]` | Done (6a56ae1b, f63f57ca) |
| Processes: Standards tab, editor, development processes | `/settings/processes` | Done (e54a1f6d, 1ac0e8ef) |
| Members and access | `/settings/access` | Done (bf324950, 86cc9435, bf4fe1e2, a54b1f3a) |
| Costs: prices, organization cost | `/settings/costs` | Done (4940b647, ab090eea) |
| Updates | `/settings/updates` | Done (b2e23e12, ffe818b2) |

## Step 4 · Projects

| Screen | Route | Status |
| --- | --- | --- |
| Projects list: teams, counts, investment; New project | `/projects` | To do |
| Project frame: header, counts, tabs in the URL | `/projects/[id]/*` | To do |
| Overview: brief, counts, features tile, latest activity | `/projects/[id]` | To do |
| Work: board and list, type filter, grouping | `/projects/[id]/work` | To do |
| Team, Activity, Conversations | `/projects/[id]/team` and so on | To do |
| Settings: name, teams, repositories, development process | `/projects/[id]/settings` | To do |

## Step 5 · Work items

| Screen | Route | Status |
| --- | --- | --- |
| Task page: fields, checklist, activity, comments, output, private notes, Why, cost and time | `/work/[ref]` | To do |
| Bug page: triage, fix tasks, Mark as fixed, Verify, Close as, Reopen | `/work/[ref]` | To do |
| Request page: conversation, outcome, draft tasks, Approve and create, Decline, Merge | `/work/[ref]` | To do |
| Work: filters, saved filters, Show menu, board and list, Needs me | `/work` | To do |
| New task, Report bug, New request brought to the current fields | dialogs | To do |
| Budget: set or raise, budget reached actions, chips | dialogs | To do |

## Step 6 · Docs, features and releases

| Screen | Route | Status |
| --- | --- | --- |
| Docs tab: list by kind and by section, filters, search | `/projects/[id]/docs` | To do |
| Document page: reader, editor, details, history, compare, issue a version | `/projects/[id]/docs/[docId]` | To do |
| New document: Brief, Decision, Design, Report | menu and editor | To do |
| Features tab and feature page; New feature, Retire, Break into tasks | `/projects/[id]/features`, `/projects/[id]/features/[featureId]` | To do |
| Releases tab and release page | `/projects/[id]/releases` | To do |
| Knowledge search ("Why is…") | `/search` | To do |

## Step 7 · Costs and insights

| Screen | Route | Status |
| --- | --- | --- |
| Project Insights: tiles, breakdowns, trend, table, export | `/projects/[id]/insights` | To do |
| Organization Insights: projects table | `/insights` | To do |
| Member Cost tab: spend, budgets | member profile | To do |

## Step 8 · Access and chat gaps

| Screen | Route | Status |
| --- | --- | --- |
| Set a new password, Approve a terminal sign-in, invited registration | `/set-password`, `/approve`, `/register` | To do |
| Change password, recovery code banner, organization switcher | shell | To do |
| Chat: question, approval and hand-off cards, browse channels, pins, mute, system notices, Inbox link, states | `/chat` | To do for the release: v1 ships origin/v1's Chat (restored in 7537f917). The work in progress (stopped part-way) is kept on branch `v1-ui-chat-wip`; step 8 continues from there |

## Step 9 · Closing

Bring the existing browser tests in line with the moved routes, capture a screenshot of
every screen at wide and narrow widths, and update `docs/FEATURES.md` rows with what the
UI now shows against mock data.

Started on `v1-ui-release-fixes` (2026-11-19): the shell, footer, navigation, landmarks,
top-bar and production specs, the shared entry helpers and the specs that drove the retired
org-chart canvas now follow the current screens; the `docs/FEATURES.md` UI rows carry the
step 0–3 evidence. Chat ships as on origin/v1, so its specs are v1's. Still open: the
screenshot pass.

## Decisions made while building

- Member profile is `/members/[memberId]` (our-org-in-the-app section 5) and marks Teams
  current. The run's session log is a page under it.
- Tasks, bugs and requests are full pages at `/work/[ref]`, as work-items-ux section 2 asks;
  the existing slide-overs stay as quick views.
- Organization Insights has no place in the navigation list of app-shell section 0. It is a
  page at `/insights`, reached from Home and from a project's Insights tab.
- New screens use the current data contract (accountable team, assignment pair, `T-`
  references). The older board and list code that still uses Areas is not rewritten here;
  it is listed as follow-up where a screen depends on it.
- The Teams view switch is `src/routes/(shell)/teams/(views)/+layout.svelte`; the route group keeps
  `/teams/[teamId]` outside it. The shell's top bar holds the page `h1` (the area name); page headings are h2.
- Navigation counts and lists and top-bar search come from `src/lib/modules/shell/api.ts`; its mock derives
  projects, teams and members from the org mock so IDs match the pages, and reads the Inbox count and work
  items from `e2e/fakes/data/shell/`.
- Step 9 must also restore four `allowedCoupling` entries in `e2e/gate-manifest.json` (focus-primitives →
  Processes, look-primitives → Members/Org chart, org-chart-agents → Offices, production-unavailable →
  Org chart) once those specs visit the new routes; the guard refuses entries without a current edge.
