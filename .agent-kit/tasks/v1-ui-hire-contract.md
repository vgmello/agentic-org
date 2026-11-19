# Slice contract: v1-ui-hire — Hire an agent, hire requests, refused view

## Outcome and acceptance criteria

- User outcome: someone who may hire starts a hire, sees what will be created, confirms it and follows it to Done or
  to Try again; everyone else sees the refusal. Hire requests are listed with their states.
- Plan row (docs/plans/v1-ui-screens.md step 2): Hire: refused view, requests list, Allow hiring switch (the switch is
  already on the member profile, members/features/allow-hiring — reuse, don't rebuild).
- Specs (copy word for word): our-org-in-the-app.md §2a (HR lead and when it can't run), §7 (Describe, Suggestion,
  HR-can't-run / Fill in the details yourself, Review, Hiring, Done, Hiring stops partway, runtime not signed in /
  can't check, "Agents come only from this flow"), §7.1 (who may hire, refused copy, hire requests list and its states,
  team member limit refusal), §11 acceptance; ADR-0029 (docs/adr/0029-only-who-may-hire-hires.md); app-shell §4, §6a.
- Routes: `/teams/hire` (the flow; `?team=<id>` preselects the home team from the launching team) and `/teams/hires`
  (Hire requests). Both inside the Teams area.
- Entry points to add: the Teams index "Hire requests" button (removed until now) and a "Hire an agent" action on the
  Teams index; the team page header's "Hire an agent" next to "Add a person" (preset team); the top-bar New menu's
  "Hire an agent" must open `/teams/hire` instead of AddAgentSheet (src/lib/platform/shell/NewMenu.svelte — you are
  authorized to change only that item). Nothing may open AddAgentSheet anymore from these places.
- States: refused ("You don't have access to hire. Ask your team lead."), home team picker lists only teams the viewer
  may hire into, HR lead reading / still starting / stopped / can't run, Next disabled with the missing-fields reason,
  Review rows only for features that exist (no cost estimate row), Hiring steps in words, Done ("{name} is hired and in
  {team}." View profile primary → /members/[id]), partial failure with Try again (no reconfirm), team full refusal,
  requests list empty / loading skeleton / error "Can't load hire requests" + Retry / Drafting (open, Withdraw) / Not
  finished (Try again) / Hired / Withdrawn.
- Architecture: src/lib/modules/employees (types, api, features/hire/, features/hire-requests/); thin routes under
  src/routes/(shell)/teams/(views)/hire*/ or src/routes/(shell)/teams/hire*/ (keep the Teams view switch working);
  mock in e2e/fakes/modules/employees.ts + e2e/fakes/data/employees/ (fix: retry clears failed state; hireAccess honours
  ?mock=slow|fails). Import other modules only through their public index. Remove employees/AgentsTabs.svelte if still
  unused.
- Non-goals: org chart, members directory, roles editor; cost estimates (step 7).
- Checks: bun run check, test, lint (only the nine known Chat errors), format:check, build; autofixer clean; screenshots
  at 1440 and 390 px of every step and state.
