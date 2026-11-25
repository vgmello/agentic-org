# Slice contract: v1-ui-members-directory — Members directory

## Outcome and acceptance criteria

- User outcome: Teams › Members (`/teams/members`) lists every person and agent in the organization, filterable, each
  opening their profile.
- Plan row (docs/plans/v1-ui-screens.md step 2): Members directory: people and agents, filter, Can sign in.
- Specs (copy word for word): offices-and-teams-ux.md §7.0, §7.1 (owner), §7.2 (directory), §8 acceptance; our-org-in-the-
  app.md §5 (record, quick view, card; AC-IA08 shared presence vocabulary) and §7 ("Agents come only from this flow");
  app-shell-ux.md §4 (titles), §6a.
- Today's page is the old "AI employees" table on the `$lib/org` snapshot with sample data — replace it. Build on
  `membersApi.directory` (members module) with `Resource`/`Loaded`; filter All · People · Agents (and search if the spec
  asks), rows: avatar, name linking to `/members/[id]`, kind in words, @handle, role, home team, presence (shared
  `PresenceBadge`), Owner badge, Can sign in, "Working on {task}" when known; at 390 px divided rows.
- Actions: "Add a person" (existing AddPersonSheet) and "Hire an agent" linking to `/teams/hire` (the Hire slice builds
  that route in parallel — link to it, do not build it). Remove "Add agent"/AddAgentSheet from this page (§7 forbids
  bringing in an agent that runs elsewhere). No "Sample data".
- Every state real: loading skeleton, empty (spec copy), filtered-empty, error with Retry, unavailable.
- Architecture: thin route; feature folder in src/lib/modules/members/features/directory/; mock in
  e2e/fakes/modules/members.ts + e2e/fakes/data/members/. Keep the members public index exports; add yours.
- Non-goals: org chart, hire flow, profile.
- Checks: bun run check, test, lint (only the nine known Chat errors), format:check, build; autofixer clean; screenshots
  at 1440 and 390 px of the list, filters and states.
