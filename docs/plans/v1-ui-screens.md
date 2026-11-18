# v1 UI screens: build plan

Branch `v1-ui`, started from `v1-integration`. Owner direction, 2026-11-18: build every
screen and interaction the specs call for that is not already built, with mock data, and
leave review for one final pass before the branch is merged. This page is the list of
screens and the order they are built in. Each row's Status is updated as it lands.

## Where the work stands (handover, 2026-11-18)

- Steps 0 and 1 are complete and were checked (type check, unit tests, build) when merged.
- Steps 2, 3 and Chat were stopped part-way and merged as they were, in commits titled
  "UI work in progress". **The merged branch has not been type-checked, tested or built.**
  Start by running `bun run check` and `bun run build` in `apps/agentic-org-ui` and fixing
  what they report, then walk each step's rows against the running app to see which
  screens exist.
- Steps 4 to 9 have not started.
- Each builder followed [the builder brief](v1-ui-builder-brief.md): file ownership, the
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
  folders ([project structure](../design/project-structure.md) section 4). Shared visual
  primitives live in `src/lib/components/ui`.
- **Mock data is JSON, and stays out of production builds.** Each module has an `api.ts`
  that declares the calls a screen needs; its production export refuses every call, as
  today. The mock for it is `e2e/fakes/modules/<module>.ts`, which answers from JSON files
  in `e2e/fakes/data/<module>/`. The dev server swaps it in by name. Replacing a mock with
  the real web API later means writing the production `api.ts`; no screen changes.
- **To see the screens:** in `apps/agentic-org-ui`, run `bun run dev:mock`, open
  `/welcome` and choose **Open**.
- **Design.** Builders work as the principal designer
  (`.agent-kit/project-skills/principal-designer/`). Product specs outrank it, above all
  `docs/specs/app-shell-ux.md` section 6a.
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
| Teams index: tree of teams, lead, counts, projects | `/teams` | In progress (stopped part-way) |
| Members directory: people and agents, filter, Can sign in | `/teams/members` | In progress (stopped part-way) |
| Org chart: one reporting tree, owner bar, Hand over ownership | `/teams/org-chart` | In progress (stopped part-way) |
| Team page: header, members; tabs Work, Projects, Activity, Conversations, Processes | `/teams/[teamId]` | In progress (stopped part-way) |
| Member profile: header and actions; tabs Overview, Work, Conversations, Activity, Runs, Processes, Role, Cost, Notes and memory | `/members/[memberId]` | In progress (stopped part-way) |
| Supervision and execution view, attempt history, Run now | member profile | In progress (stopped part-way) |
| Session log viewer (messages, tool calls, files, usage) | `/members/[memberId]/runs/[runId]` | In progress (stopped part-way) |
| Sign-in access dialogs for a person: Invite, New link, Remove access | member profile | In progress (stopped part-way) |
| Hire: refused view, requests list, Allow hiring switch | Teams | In progress (stopped part-way) |

## Step 3 · Organization settings

| Screen | Route | Status |
| --- | --- | --- |
| General: name, workspace, runtime, recovery code row, administrator | `/settings` | Done |
| Offices: Add office, invitation, Approve a join, Remove, Rename, unplaced agents, outbox | `/settings/offices` | In progress (stopped part-way) |
| Agent setup: list and one agent's setup, health | `/settings/agents`, `/settings/agents/[memberId]` | In progress (stopped part-way) |
| Roles: list, role page, editor, New role | `/settings/roles`, `/settings/roles/[roleId]` | In progress (stopped part-way) |
| Processes: Standards tab, editor, development processes | `/settings/processes` | In progress (stopped part-way) |
| Members and access | `/settings/access` | In progress (stopped part-way) |
| Costs: prices, organization cost | `/settings/costs` | In progress (stopped part-way) |
| Updates | `/settings/updates` | In progress (stopped part-way) |

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
| Chat: question, approval and hand-off cards, browse channels, pins, mute, system notices, Inbox link, states | `/chat` | In progress (stopped part-way) |

## Step 9 · Closing

Bring the existing browser tests in line with the moved routes, capture a screenshot of
every screen at wide and narrow widths, and update `docs/FEATURES.md` rows with what the
UI now shows against mock data.

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
