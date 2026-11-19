# Slice contract: v1-ui-offices — Organization settings › Offices

Branch `v1-ui-offices` (worktree `agentic-org-ui-wt/offices`), base `323d283b`. Owner direction 2026-11-19: build
fast and complete, production ready except the mock APIs; reviews run after the commit.

## Outcome

An owner in Organization settings › Offices sees every office of the organization (HQ first, the offices on HQ's
instance, the others A–Z), with each office's live status, where it runs and how many agents it holds; opens an
office's details; renames it; removes an office that isn't HQ, seeing the cost first; adds an office on this
instance (administrator only) or invites another machine (the invitation shown once); approves or declines a
computer that wants to join with its check code; cancels, renews and removes invitations; places or offboards
the agents left unplaced by a removed office; and retries or discards commands an office could not deliver
(the outbox). The status, restart and repair behaviour of the connected office (§3.3) is kept.

The office flows beyond the connected office run against the mock offices API (`e2e/fakes/modules/offices.ts`);
the production `officesApi` answers every call with `unavailableCall` and the page says so honestly: the
connection's own offices, their live status and restart still work, and the flows the web API doesn't serve
yet show "isn't built yet" copy instead of a dead control.

Usable outcome: the Offices screen as the spec draws it, in the mock; the real connected-office status in
production. Prerequisites declared: the web API's offices endpoints (not in this slice).

## Sources (owning text wins)

- `docs/design/organization-data/01-organization-structure.md` (office: id, slug, name, revision, role hq |
  satellite; agent placements) — wins over the UX text.
- `docs/specs/offices-and-teams-ux.md` §0 (decisions 3, 5, 7, 8, 9, 13), §1 words, §2 pills, §3.1 status
  vocabulary, §3.3 why an office stopped, §6.1–§6.5, acceptance OFC-1…OFC-13, OFC-29…OFC-31, OFC-35, OFC-37,
  OFC-39…OFC-48.
- `docs/specs/server-installation-and-access-ux.md` §4 (HQ browser access and satellites; joining is separate
  from setup, approvals work from any authorized browser).
- `docs/specs/app-shell-ux.md` §0.1 (Offices lives in Organization settings; not-built copy), §2 (320 px),
  §4 (titles), §6a (look, states, focus, controls).
- Plan: `docs/plans/v1-ui-screens.md` step 3 row "Offices" and build-order item 7.

## Route

- `/settings/offices` (and `/offices`, which redirects keeping the query). Title
  "Offices · Organization settings · Agentic Org". The route is thin: it renders `OfficesPage` from
  `$lib/modules/offices`; its load calls the module's `loadOfficeCounts`.

## Acceptance criteria

1. OF-1 List (OFC-1, §6.1): h2 "Offices" with the real count; description "Where <org>'s agents run. HQ keeps
   the records and approves new offices; other offices keep a copy."; order join requests, HQ, offices on the HQ
   instance, other offices A–Z, invitations and offices being added, unplaced agents, outbox; footer "Shown from
   HQ. Statuses include last-seen information." (OFC-48).
2. OF-2 Row (OFC-13, OFC-42): name (HQ marked), where it runs with freshness ("last seen 2 h ago",
   "· copying records"), agent count (skeleton while counted, "—" when it couldn't be), one status pill with icon
   and word; the row button's label reads "<name>, <role>, <where>, <status>[: reason], <n agents>, opens
   details" and never claims "(you're here)" unless the host says so; status changes are announced in a polite
   live region; countdowns don't tick in the label (OFC-46).
3. OF-3 Details sheet (§6.1, OFC-47): description list (name with Rename, role, where, status, last seen, sync,
   version, agents placed here, projects it follows); Remove office… only for an office that isn't HQ; one Close;
   focus back to the opener.
4. OF-4 Version note (§6.1): for an office whose status is Update needed, a quiet note above the list:
   "<office> runs an older version of Agentic Org, so new features wait until it updates." with a link-styled
   control that opens that office's row details.
5. OF-5 HQ-only hint (§6.1): with HQ alone and nothing pending, "Add an office to run agents on another machine,
   or to keep a copy of the records there." under the list.
6. OF-6 Add office (§6.2, OFC-2, OFC-3, OFC-7, OFC-29…OFC-31, OFC-35, OFC-37): the one primary action. The
   administrator chooses On this instance or On another machine; any other owner gets the invitation straight
   away, titled "Add an office on another machine", with no disabled option or hint. On this instance shows
   Creating, Synchronizing and Ready separately; a failure shows its reason and Try again; waiting for running
   agents names them, keeps the name read-only and keeps Cancel; closing leaves a pending row ("Waiting for 3
   agents to stop · on HQ instance") with Cancel ("Stop adding <name>? It wasn't added yet." Stop adding / Keep
   waiting), which reopens the form when chosen; HQ's refusal for a non-administrator shows the OFC-31 copy with
   Invite another machine and no Try again. Another machine: the invitation is shown once with Copy invitation,
   the expiry and the shown-once warning, and the CLI setup steps; closing leaves an Invited row with Cancel
   invitation and New invitation.
7. OF-7 Invitations (OFC-3, OFC-4, §6.3): an Invited row counts down its expiry; New invitation asks first when
   the current one still works ("The previous invitation stops working…"); an expired, declined or cancelled
   invitation shows New invitation and Remove.
8. OF-8 Approve a join (§6.3, OFC-5, OFC-6): highlighted Wants to join row with Review and approve; the dialog
   names the office, the address, when the invitation was made and used; one check-code field (case, spaces and
   dashes ignored; O→0, I/L→1; a U or other character refused as typed with "Codes use digits and letters,
   without U"); Approve enabled at 8 characters; a wrong code says the tries left; the third cancels the
   invitation; Decline asks once; approved → the office appears as "Starting · copying records" and a toast
   "<office> joined <org>."
9. OF-9 Remove office (§6.4, OFC-8, OFC-10): red-icon dialog naming the office, its agents (become unplaced),
   their tasks and the offline case; primary "Remove <office>" (destructive). Afterwards its agents show under
   Unplaced with Place… and Offboard… each.
10. OF-10 Unplaced (§6.4): "Unplaced · n agents" group with Place… (office picker) and Offboard… (the employees
    module's Offboard dialog through its public index).
11. OF-11 Outbox (§6.5, OFC-9): Pending and Blocked commands with their office, reason and age; Retry (busy
    label "Retrying…") and Discard (confirmed, naming what won't be sent).
12. OF-12 Status, restart and repair (§3.1, §3.3, OFC-11, OFC-12, OFC-39…OFC-46): unchanged from the existing
    page: Checking… until verified, Unreachable with Retry, Stopped with the reason and Restart office / Start
    office, the repair instructions for `unsupported_version`, the countdown rules.
13. OF-13 States (§6a): first load — the connection's rows at once with count skeletons and a skeleton row for
    the offices still being read; overview error — inline alert with Retry (the list stays); overview
    unavailable (production) — a quiet "Adding, approving and removing offices isn't built yet. It's coming in
    this release." note, and no Add office button (no dead control); count error — inline alert with Retry;
    a request or invitation that is gone — the refusal inline in its dialog; busy labels on every action
    (Approving…, Declining…, Removing…, Renaming…, Placing…, Retrying…, Discarding…).
14. OF-14 Focus (§6a): closing a dialog or the sheet returns focus to its opener; after removing a row, focus
    the neighbour; after an office is removed, focus the Unplaced heading.
15. OF-15 Layout: 1440, 1024, 640, 390, 320 px and 320 px at 200% text: no sideways scroll; 44 px targets below
    640 px; pills keep whole words.

## Mock switches

`e2e/fakes/modules/offices.ts`: `?mock=slow|fails|empty|unavailable` (empty: HQ alone, nothing pending),
plus `?admin=other`, `?add=waiting|refused|fails`. The older `?offices=fails|slow` stay as aliases.

## Non-goals

- The web API's offices endpoints and the Rust side (another increment).
- Needs-you items for joins and unplaced agents (Inbox), the org chart's Unplaced tag and office filter (their
  owners), promotion to HQ (§10), and the joining instance's own view (§5c, CLI).
- Moving `$lib/offices/{model,probe}.ts`: the shell's footer office line (a shared file this slice must not edit)
  uses them; they stay the shared office-status vocabulary.

## Ownership

This worktree owns `src/lib/modules/offices/**`, `src/routes/(shell)/settings/offices/**`,
`e2e/offices.spec.ts`, `e2e/fakes/modules/offices.ts`, `e2e/fakes/data/offices/**`, and the offices entries of
`e2e/gate-manifest.json`. It moves `src/lib/components/OfficeDetailsSheet.svelte`,
`src/lib/components/OfficeStatusBadge.svelte`, `src/lib/offices/OfficeRepairLocation.svelte` and the
`src/lib/components/testing/office-status.*` fixture into the module (offices-only code).
