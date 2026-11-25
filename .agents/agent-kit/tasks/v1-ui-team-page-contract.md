# Slice contract: v1-ui-team-page — team page and Teams index gaps

## Outcome and acceptance criteria

- User outcome: from Teams, opening a team shows `/teams/[teamId]`: who leads it, its members, purpose and its work,
  projects, activity, conversations and processes — against the mock API.
- Plan row (docs/plans/v1-ui-screens.md step 2): Team page: header, members; tabs Work, Projects, Activity,
  Conversations, Processes. Also the Teams index gaps the audit found: the "Hire requests" link must not be dead (point
  it where the Hire slice will build `/teams/hires`, or hide it until then — say which), team links reach the page.
- Specs (copy word for word): offices-and-teams-ux.md §7.2 (team pages), §7.5–7.7, §7.9–7.10, §8 acceptance;
  processes-ux.md "Where" (the Processes tab, decided in the plan: five tabs); our-org-in-the-app.md §5 member card,
  §8 (assign and change teams); app-shell-ux.md §4, §6a.
- Header: name/ref, lead, counts, purpose, "Members: N of M" (maxMembers); at most six member rows plus "All N
  members"; Hire or add; team actions that already exist on the index (rename, move, archive, change lead, delete)
  reachable from the page.
- Every state real: loading skeleton, empty per tab (spec copy), error with Retry, unavailable, not found / archived.
  Tabs in the URL; focus to the heading on tab change.
- Architecture: thin route; src/lib/modules/teams (features/team-page/... etc.); reuse members/components
  {WorkRows,ActivityRows,ConversationRows,ProcessRows} through the members module's public index, not deep paths
  (add the exports to members/index.ts; that file is shared with the member-profile slice: add only those exports).
  Mocks only in e2e/fakes/modules/teams.ts + e2e/fakes/data/teams/.
- Non-goals: Hire flow, Members directory, Org chart, member profile.
- Checks: bun run check, test, lint (only the nine known Chat errors), format:check, build; svelte-autofixer clean;
  screenshots at 1440 and 390 px of each tab and state.
