# v1 UI · Projects (plan step 4, without the Work tab)

Branch `v1-ui-projects`, worktree `agentic-org-ui-wt/projects`. Delivery contract for reviewers.

## Outcome

A person can see every project with its accountable team, contributors, work and bug counts and (when they may
see amounts) its investment; create a project, which by default creates its own team; open a project and read its
brief, counts, features line and latest activity; see its teams and members, activity and conversations; and change
its name and slug, accountable team, contributing teams, repositories (each with its own integration branch) and
development process. Usable against the mock API (`bun run dev:mock`); production refuses every call honestly.

## Requirements (source → acceptance)

| Ref | Requirement | Where |
| --- | --- | --- |
| PRJ-1, PKG-15 | Tabs: Overview, Work, the development process's tabs, Features/Releases when held and not given, Docs, Team, Activity, Conversations (+ Settings). Tabs for experiences not served are never shown. | `projectTabs()` (pure, unit tested with every flag on); served today: Overview, Work, Team, Activity, Conversations, Settings |
| PRJ-1 / lead | Work tab exists and links `/projects/[id]/work`, which says "Work isn't built yet" / "It's coming in this release." | `WorkNotBuilt` |
| PRJ-2 | Overview: brief, real open/blocked counts, five latest activity entries; honest empty states; no invented lead/health/due date | `ProjectOverview` |
| PRJ-2 | Team tab separates accountable/contributing teams from member rows | `ProjectTeam` |
| PRJ-2, §3 | Activity / Conversations: "No activity yet" / "No conversations yet" | `ProjectActivity`, `ProjectConversations` |
| PRJ-3, §3 | Header: Projects back link, one accountable team, optional contributors, real done/total | `ProjectFrame` |
| PRJ-3, TEAM-8/14, §7.9 | Settings: accountable owner team (required, never cleared; change previews affected tasks), contributing teams, repositories each with its own required integration branch; teamless never accepted | `ProjectSettings` |
| PRJ-4 | Unavailable accepting service: last data readable, labelled stale, writes disabled with a visible reason | stale notice + disabled fieldsets |
| PRJ-5 | List: accountable team apart from contributors; open bug counts by severity including Not set | `ProjectsList` |
| COST §1, decision 6 | Projects list Investment column: total to date, this month beside it; only to viewers allowed amounts (others: no column); unknown is "—" with reason; incomplete totals say so | `projects/investment.ts` via `$lib/modules/costs` |
| features-ux | Overview tile "Features: 2 in progress · 1 planned · 5 live" | Overview |
| §7.9, TEAM-8 | New project creates "New team: <name>" by default; an existing team can be chosen (prefilled from `?team=`) | `NewProjectSheet` |
| PKG-4/5, dev-processes §3 | Development process select: current processes and "None" (no drafts/deprecated); before confirming, show roles it will create and existing roles it will use and sections it adds; nothing is hired or started | Settings + New project, list through `$lib/modules/processes` |
| app-shell §4 | Titles: "Projects · Agentic Org", "<Project> · Projects · Agentic Org", "<Tab> · <Project> · Projects · Agentic Org" | frame |
| app-shell §6a | Skeletons with Loading/Still loading, error with Retry, unavailable, not found, busy -ing labels, refusals inline on the field, conflict keeps typed values, confirmations name the effect; tokens only; 44 px targets below 640; no sideways scroll at 320 / 200% | all |
| app-shell §0 | Route changes focus the destination heading; tab change focuses the tab heading after the shell's own focus | frame |

## Routes

`/projects` · `/projects/[id]` (Overview) · `/projects/[id]/work` · `/projects/[id]/team` · `/projects/[id]/activity` ·
`/projects/[id]/conversations` · `/projects/[id]/settings`.

## States

Loading skeleton shaped like the page; unavailable (production); error with Retry; empty (no projects); project not
found; stale copy (writes disabled with reason); viewer without edit rights (no New project, Settings read-only with
reason, no amounts); refusals on the field (name, slug, branch); revision conflict; busy labels.

Mock switches (`?mock=`, comma-joined): `slow`, `fails`, `empty`, `unavailable`, `viewer`, `stale`, `conflict`
(first save conflicts), `save-fails`.

## Non-goals (and why)

Work board/list (step 5), Docs/Features/Releases/Insights tabs and pages (steps 6–7), New task / Report bug in the
header (step 5 flows), the Status page and reports (not in v1), archiving a project (no flow in the specs for v1 UI),
seed roster on Overview (needs Hire/Assign from a project: not in this step's list; reported).

## Ownership

Owned: `src/lib/modules/projects/**`, `src/routes/(shell)/projects/**`, `e2e/fakes/modules/projects.ts`,
`e2e/fakes/data/projects/**`, `e2e/projects.spec.ts` (rewritten: the old one pinned the replaced screens),
`e2e/gate-manifest.json` (projects entries). Additive export in `src/lib/modules/processes/index.ts` (the dev
process catalogue read), declared to the lead.
