# Slice contract: v1-ui-access — Settings › Members and access

## Outcome and acceptance criteria

- User outcome: the owner or the administrator sees, for one organization, who can sign in and with what authority:
  the owner and the administrator, people with an account, people with a pending invitation, and people with no account
  yet. They can invite a person to sign in, make a new sign-in link for a person with an account, or remove that
  person's access, each with the one-time link shown once or a confirmation that names the effect. Everyone else in the
  organization sees the same list without actions and is told who can change it.
- Plan row: docs/plans/v1-ui-screens.md, Step 3, "Members and access", route `/settings/access`.
- Specs (the spec stands; quoted words are the spec's):
  - app-shell-ux.md §0.1: Members and access is an included section; it becomes active only when built. Replace the
    placeholder NotBuilt page; the nav entry already exists in settings/+layout.svelte.
  - app-shell-ux.md §4 (titles), §6a (look), §1/§2 (navigation, 320 px).
  - server-installation-and-access-ux.md §4a.1 (accounts; one person per account per organization; no account for
    agents; "No browser screen lists all accounts of the instance"; New sign-in link; Remove access; owner's access
    cannot be removed), §4a.2 (registration and set-new-password links, 24 h, shown once), §4a.4 (account menu says
    "Administrator" for the administrator), §4a.6 (recovery code state is owner-only; the code never reaches the
    browser), SIGN-25, SIGN-28, SIGN-36, SIGN-37, SIGN-38.
  - offices-and-teams-ux.md §5b (who can do what), §7.1 (owner; Hand over ownership… lists people with an account;
    owner cannot remove own access), §7.4 (Invite to sign in, New sign-in link, Remove access…; wording copied).
  - our-org-in-the-app.md §7.1 (who may hire: owner and administrator into any team; an employee with Allow hiring
    into the teams they lead and the teams under them).
  - Data contract docs/design/organization-data/README.md and 04-agent-structure.md: an account is held by the host
    and, per organization, tied to exactly one person; a person needs no account; an account is never tied to an
    agent; the owner is whoever holds ownership at the host.
- Acceptance (this slice):
  - Page heading h2 "Members and access"; shell h1 is the area name ("Organization settings").
  - Owner and administrator are shown as facts at the top: owner name, administrator username and person (or "no
    person in this organization yet"), and "you" where true.
  - Three lists of people (agents never appear): "Can sign in", "Invited, not signed in yet", "No account yet". Each row:
    name, handle, home team, Owner / Administrator badges, state in words.
  - Actions for the owner or the administrator only (viewer.canManage): Invite to sign in (no account; also when an
    invitation is pending, with "Inviting again cancels the previous link."), New sign-in link (has account), Remove
    access… (has account, not the owner). The owner's row shows "Hand over ownership first." and links to the org chart,
    where ownership moves.
  - One-time link: shown once in the dialog (reused from members/features/sign-in-access), Copy link, closing reloads the
    list. The list never keeps the link.
  - Remove access…: ConfirmDialog naming the effect ("{who} is signed out everywhere and can't open {org}. {who} stays a
    person here.") with busy label "Removing access…".
  - Refusals (not_authorized, not_found, conflict) appear inline next to the action; nothing changes locally on refusal.
  - Viewers without manage rights see the lists, no action buttons, and one line: "Only the owner or the administrator
    changes sign-in access."
  - Who can do what: a short reference section with the §5b rows that concern access and hiring.

## States (each real, each reachable in the mock)

- Loading: skeleton shaped like the page (rows, Loading label, "Still loading" after 2 s via SkeletonRows).
- Unavailable: NotBuilt-style "not served by this server" via Loaded (`area` set) — production answers unavailableCall.
- Error: ErrorState "Couldn't load who can sign in" with Retry.
- Empty: no people in the organization: EmptyState "No people yet" with the link to Teams › Members (one real next
  action). Empty inside a single list ("No one is waiting for an invitation.") is a line, not an EmptyState.
- Action busy: "Making link…", "Inviting…", "Removing access…" with aria-busy.
- Refusal inline: role="alert" message under the section.
- Not found: an action on a person removed since the list loaded refuses with "That person isn't in this organization
  any more." and the list reloads.

## Routes

- `/settings/access` only: `src/routes/(shell)/settings/access/+page.svelte` (thin: title, organization id, component).

## Architecture and ownership

- New module `src/lib/modules/access/`:
  - `types.ts` — AccessOverview, AccessPerson, AccessState, IssuedLink (access's own shapes).
  - `api.ts` — `accessApi`: overview, issueLink, removeAccess. Production object answers every call with unavailableCall.
  - `index.ts` — public entry (route imports only from here).
  - `features/overview/` — AccessPage (loads, composes), list rows, lists by state, who-can-do-what section.
  - `present.ts` + `present.test.ts` — pure grouping (state partition, sort by name) and state words.
- Shared dialogs: `members/features/sign-in-access/SignInAccessDialogs.svelte` is the dialog host (one-time link,
  Remove access confirmation). It takes two callbacks (issue link, remove access) and reports each started action's
  outcome once (`linked`, `removed`, `cancelled`, `gone`). The access page mounts one host and keeps it across list
  reloads, so focus follows the person to its new row. `SignInAccessActions.svelte` (the profile's buttons plus a host)
  and SignInAccess keep the profile's Section and DescriptionList and behave as before. The members index exports both.
- Mock: `e2e/fakes/modules/access.ts`, data under `e2e/fakes/data/access/` (accounts, invitations, owner and
  administrator keyed by member id). People and names come from `orgApi.view` (the org snapshot), as the other fakes do.
  Switches: `?mock=slow|fails|empty|unavailable|viewer` (viewer = no manage rights).
- Coupling: access imports the members public index (the dialog host, expiryText). No `allowedCoupling` entry is needed:
  both are in the same e2e category and `ui-gate.test` passes.
- Not changed: settings module (its `access`, `issueLink`, `removeAccess` methods and `AccessOverview` in
  settings/types.ts are now unused by the UI; the settings owner should remove them: flagged to lead).

## Non-goals

- No browser list of all instance accounts (spec: forbidden).
- No hand-over of administrator on this page: it stays on General (`settings/features/general/HandOverDialog`), which
  already implements server-installation §4a.2. Link to it.
- No hand-over of ownership on this page: it stays on the org chart owner bar (offices §7.1). Link to it.
- No recovery code on this page (owner sees recovery state on General).
- No Allow hiring switch here: it is per person on the member profile and is not an access action.
- No email sending, no password forms, no "forgot password".

## Layout and copy rules

- Divided rows (`divide-y divide-white/5`), gutters `px-4 min-[640px]:px-6 min-[1024px]:px-8`, controls `h-9`,
  `max-[639px]:min-h-11` below 640 px, pills keep whole words, no shadows or gradients, one indigo primary (none needed
  here; actions are outline buttons).
- Colours only through shared helpers (`tone`, `classes`).

## Checks

- Svelte autofixer clean on every .svelte; list-sections/get-documentation consulted.
- bun run check, test, format:check, build; lint only the nine known Chat-route errors.
- `PW_PORT=<port> bunx --no-install playwright test e2e/access.spec.ts --project=chromium --workers=2`.
- Screenshots at 1440 and 390 px of every state into checkpoints/shots/access/ (dev:mock, __p6-auth then /settings/access).
- Measurements: consistency audit for the page's rows (height, padding, gaps), 320 px reflow, 200% text, keyboard focus.
