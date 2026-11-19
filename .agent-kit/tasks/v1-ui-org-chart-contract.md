# v1 UI · Org chart: delivery contract

Branch `v1-ui-org-chart`, from the step-2 integration branch. Plan row:
`docs/plans/v1-ui-screens.md` Step 2, "Org chart: one reporting tree, owner bar, Hand over ownership"
(`/teams/org-chart`). Delivery rules: `docs/architecture-rules.md` and `docs/independent-delivery.md`.

## Outcome (usable feature)

A person opens Teams › Org chart and sees who reports to whom: the owner bar on top, then one vertical
reporting tree of every active employee (people and agents, the owner's person and the HR lead included),
each once. They can open anyone's profile, change someone's manager, move someone's home team and add a
person. The owner can hand over ownership to another person with an account. Every state is shown truthfully;
the production build answers every read and change with `unavailable` until the web API serves it.

## Requirements (source of truth)

- `docs/specs/offices-and-teams-ux.md` §7.0, §7.1, §7.4, §7.6, §7.8, §7.10, §8 (OWNER-1..3, REPORT-1..5,
  TEAM-2, TEAM-10, TEAM-11, TEAM-12, PEOPLE-3, PEOPLE-4, PEOPLE-6).
- `docs/specs/our-org-in-the-app.md` §5 (names open the profile, one presence vocabulary) and §7 (agents
  come only from Hire: no Add agent anywhere).
- `docs/specs/app-shell-ux.md` §1, §2, §4, §6a.
- Data contract `docs/design/organization-data/README.md` wins over the UX specs: `reports_to_employee_id`
  is an explicit employee reference; `home_team_id` is required; nothing on the employee record grants
  ownership.

## Acceptance criteria

1. **Owner bar (OWNER-1, §7.1).** Above the tree: initials, the owner's name ("(you)" when it is the viewer),
   "Owner of <organization>", "Full access to this organization.", a **Profile** link, and, for the owner
   only, **Hand over ownership…**. Exactly one owner. The owner's person is also in the tree, once, in its
   recorded reporting position, with its home team (OWNER-2).
2. **Hand over ownership (OWNER-2).** Lists active people with an account other than the owner. The dialog
   reads "Make Sam Ortiz the owner of Lumen Labs? You'll stay in Lumen Labs as a person, without the owner's
   access." and asks for the owner's password. Wrong password: inline error on the field, focus there, input
   kept, "Nothing changed". Success only after the server confirms: toast, owner bar shows the new owner and
   the action disappears for the former owner, without a page reload. No candidates: say why and how to get
   one (Invite to sign in from a person's profile), no functioning-looking button.
3. **One reporting tree (REPORT-1, TEAM-11).** Draws explicit `reports_to` only. Employees without an explicit
   manager are grouped under **No explicit manager** below the owner bar; no manager is invented. Every active
   employee appears exactly once. A reference to someone who is not an active employee, or a reporting loop,
   never hides anyone: the employee is shown under No explicit manager with the problem in words. No zoom, no
   Fit, no chart search, no teams/people tabs.
4. **Nodes (REPORT-2, TEAM-12, PEOPLE-6, §7.0).** Initials, name (a link to `/members/:id`, spoken
   "Nadia Haddad, agent"), Agent or Person as a word, `@handle`, one home team, number of explicit reports
   ("2 reports", omitted at 0), agents: office and freshness or **Unplaced**; people never show an office.
   Paused employees say Paused in words. A team lead also shows "Leads <team>" (both sources of
   responsibility, REPORT-5).
5. **Explanation (REPORT-5).** Always: "Reporting lines are explicit. Team leads also manage their team's
   subtree; ordinary ancestor-team members do not."
6. **Keyboard tree (TEAM-10).** Every node has a collapse/expand control when it has reports (`aria-expanded`,
   names the employee), a profile link and an actions menu. Arrow Up/Down move between node links, Right
   expands or goes to the first report, Left collapses or goes to the manager, Home/End first/last visible.
   Large organization (>50 nodes): nodes below level two start collapsed.
7. **Change manager (REPORT-3, REPORT-4).** From the node menu. Offers active employees plus **No explicit
   manager**; the employee itself is absent; employees who report to them (directly or below) are disabled
   with "Reports to <name>". Saves with the expected revision. Success: toast names the new manager, the node
   moves and keeps focus. Conflict or loop refused by the server: input kept, current lines refreshed, the
   message says what changed. Changing a manager never changes team leads, and the reverse.
8. **Move home team (TEAM-6, §7.6).** From the node menu: the member module's existing reviewed dialog.
9. **Add a person (PEOPLE-3, PEOPLE-4, §7.4).** Page action; the teams module's existing Add person sheet
   (name, handle, contact, home team, reports to), home team starting at the owner's top-level team. Added:
   the node appears once at its manager (or No explicit manager), is focused and highlighted for two seconds;
   toast "Added Sam Ortiz. Agents can now reach Sam Ortiz." only after HQ accepted it. The owner's person is an
   eligible manager there (§7.1).
10. **No Add agent (our-org §7).** No Add agent on this page or anywhere; the obsolete AddAgentSheet and the
    `/teams/members?add=1` URL helpers are deleted. Hire an agent links to the Hire flow.
11. **States (§6a, §7.10).** Loading: owner-bar-shaped skeleton then skeleton nodes, labelled Loading, "Still
    loading" after 2 s. Error: "Couldn't load the org chart" with the reason and Retry. Unavailable: said in
    place, nothing invented. Empty (no employees besides the owner's absence cannot happen; an organization
    whose read returns no employees): explains the page and offers Add a person. Live changes (any
    `invalidate('app:org')`) re-read in place keeping the shown tree, the collapsed set and focus.
12. **Titles, headings, focus (§4, §6a).** Title "Org chart · Teams · Agentic Org". Page heading h2. Dialogs
    restore focus to their opener (or the moved node). Busy buttons keep width, use the -ing word and
    `aria-busy`.
13. **Layout (§2, §6a).** 1440, 1024, 640, 390, 320 px and 320 px at 200% text: no sideways page scroll, 44 px
    targets below 640 px, pills keep whole words, indentation caps so deep trees keep a readable text column.

## Routes

- `/teams/org-chart`: the page (thin route: load the structure for the add/move dialogs, render the module).
- `/org-chart`: existing redirect, unchanged.

## Ownership (one writer: this session)

- New: `src/lib/modules/org-chart/**`, `e2e/fakes/modules/org-chart.ts`, `e2e/fakes/data/org-chart/**`,
  `src/lib/modules/teams/index.ts` (public entry for the teams module).
- Rewritten: `src/routes/(shell)/teams/(views)/org-chart/**`, `e2e/org-chart.spec.ts`.
- Small additive edits: `src/lib/modules/teams/features/team-page/AddPersonToTeam.svelte` (owner eligible as
  manager; `onadded` receives the new member; optional trigger variant), `src/lib/modules/members/index.ts`
  (export the existing MoveHomeTeamDialog), `src/lib/modules/employees/{index,urls}.ts`,
  `e2e/gate-manifest.json`.
- Deleted when nothing uses them: `AddAgentSheet`, the employees URL helpers and their tests, the old
  org-chart page's components in `src/lib/components`, and the old `e2e/org-chart-*.spec.ts` specs whose
  behaviour the spec withdrew (team canvas, people canvas, zoom/Fit, several memberships, Not in a team,
  Add agent); their still-valid requirements move into `e2e/org-chart.spec.ts`.

## Interfaces

- `orgChartApi.chart(organizationId) → OrgChart` (`organization`, `viewer`, `ownerId`, `employees[]` with
  `revision`, `reportsToId`, `homeTeam`, `leads`, `office`, `canSignIn`, `status`).
- `orgChartApi.changeManager(organizationId, employeeId, managerId | null, expectedRevision)`: refuses
  `conflict` (stale revision or loop) and `not_found`.
- Hand over uses `membersApi.handOverOwnership` (members public index): one existing contract, not a copy.

## Non-goals (and where they live)

- Team hierarchy editing (New team §7.5, Move team §7.7, archive/delete §7.10): the Teams index and team
  page (team hierarchy is not drawn on this reporting tree, §7.2/§7.8).
- Edit, Pause/Resume, Offboard, sign-in access: the member profile. Deactivate a person: no API exists yet.
- Drag-to-change-manager: a shortcut the spec allows, not required; the menu path is the required one.
- Fixing other areas' browser specs that still open the old page (tasks, non-chat-icons, dialog-title-words):
  plan step 9.

## Verification

Unit tests for the tree, eligibility, keyboard moves and hand-over candidates; `e2e/org-chart.spec.ts`
(chromium) for the flows and states; `bun run check|test|format:check|build`, `bun run lint` (only the nine
known Chat errors); screenshots at 1440 and 390 px of every state into
`/Users/vgmello/projects/agentic-org-ui-wt/checkpoints/shots/org-chart/`.
