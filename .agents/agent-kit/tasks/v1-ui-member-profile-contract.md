# Slice contract: v1-ui-member-profile — member profile, supervision, session log, sign-in access

## Outcome and acceptance criteria

- User outcome: opening any person or agent shows their profile at `/members/[memberId]`, with what they are doing,
  how they run, their runs and session logs, and (for a person) their sign-in access — all against the mock API.
- Plan rows (docs/plans/v1-ui-screens.md step 2): Member profile; Supervision and execution view, attempt history,
  Run now; Session log viewer `/members/[memberId]/runs/[runId]`; Sign-in access dialogs (Invite, New link, Remove
  access).
- Specs (owning text wins, copy word for word): our-org-in-the-app.md §5 (profile, quick view, card), §9
  (deallocate: pause, offboard), §10–11 (accessibility, acceptance); employee-supervision-ux.md §1–3; agent-run-modes-
  ux.md §1, §3, §4 (RUN-4/5 Run now: note, busy, cannot_start); req-05 (session log: messages, tool calls, files,
  usage; tool-call and file toggles); offices-and-teams-ux.md §7.0, §7.4 (person access dialogs, SIGN-25/36/37) and
  server-installation-and-access-ux.md §4a; roles-ux.md (Role tab); processes-ux.md "Where" (Processes tab);
  app-shell-ux.md §4 (titles) and §6a (look, states, focus).
- Tabs: Overview, Work, Conversations, Activity, Runs, Processes, Role, Notes and memory, Cost. Cost belongs to step 7:
  the tab exists and shows the honest NotBuilt placeholder until then. Runs/Role/supervision only for agents; access
  only for people with or without an account.
- Every state real: loading skeleton, empty (spec copy), error with Retry, unavailable; refused/busy/conflict states
  the specs name. Keyboard and focus: dialogs return focus to their opener; tabs in the URL.
- Fix the fake gaps the audit found: inviting sets `access`; New link and Remove access refuse a member without an
  account or who is not a person; Hand over ownership is out of scope here (org chart slice).
- Architecture: thin routes; everything in src/lib/modules/members (types, api, features/<feature>/); mocks only in
  e2e/fakes/modules/members.ts + e2e/fakes/data/members/. Use src/lib/components Resource/Loaded/ConfirmDialog/
  CopyButton/focus/classes/pills. The sign-in link display (`OneTimeLink`) lives in settings/features/access; if the
  profile needs it, move it to a shared place both use (no cross-module internals), and say so.
- Session-log parsers (members/features/session-log) are used by the viewer through the api, not only by fakes.
- Non-goals: Members directory, Org chart, Hire, Team page (other slices); Cost tab content (step 7).
- Checks: bun run check, test, lint (only the nine known Chat errors), format:check, build; svelte-autofixer clean
  on every .svelte written; screenshots at 1440 and 390 px of each tab and dialog and of loading/empty/error.
