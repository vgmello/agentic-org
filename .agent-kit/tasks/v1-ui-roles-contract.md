# Slice contract: v1-ui-roles — Roles list, role page, editor, New role

Branch `v1-ui-roles` (worktree `agentic-org-ui-wt/roles`), base `f68b9efb`. Owner direction 2026-11-19: build
fast and complete, production ready except the mock APIs; reviews run after the commit.

## Outcome

A person in Organization settings can see the organization's shared roles (name, revision, how many employees
reference each), open one to read its sections, the employees that reference it, its revision history and its
record provenance, edit it with a revision check that never loses input, create a new role, and ask the HR lead
to draft one when the HR lead can run. All against the mock roles API; the production API refuses every call
(`unavailableCall`) and the screens say so.

## Sources (owning text wins; copy word for word where given)

- `docs/design/organization-data/01-organization-structure.md` (roles table: `id`, optional unique `key`,
  `schema_version` 1, `revision`, `name`, `body_markdown`, created/updated attribution) and `04-agent-structure.md`
  (`role_id` on employees, `role_text` kept apart). The data contract wins over the UX spec.
- `docs/specs/roles-ux.md` §0 decisions 1, 2, 3, 5, 7; §1 flow 1 (list, New role…, ask HR, role page, states);
  §3 ROLE-1, ROLE-5, ROLE-9 (read side), ROLE-10.
- `docs/specs/app-shell-ux.md` §0.1 (Roles lives in Organization settings), §1, §2 (320 px), §4 (titles),
  §6a (look, states, focus, controls).
- Plan row: `docs/plans/v1-ui-screens.md` step 3 "Roles: list, role page, editor, New role".

## Routes

- `/settings/roles` — the roles list. Title "Roles · Organization settings · Agentic Org".
- `/settings/roles/[roleId]` — one role. Title "<Role name> · Roles · Organization settings · Agentic Org";
  "Role not found · Roles · …" when it does not exist; "Role · Roles · …" while loading.

## Acceptance criteria

1. RL-1 List: h2 "Roles" with the real count; each row shows the role's name (one stretched link to its page),
   its key when it has one, `rev N` and the employee count ("4 employees", "1 employee", "no employees yet"),
   sorted by name. Wide: revision and count are aligned columns; below 640 px they join the meta line.
2. RL-2 New role… (the one primary action) opens the editor for a new record: Name (required), Key (optional;
   lowercase letters, digits and hyphens), Markdown body prefilled with the usual sections (Mission,
   Responsibilities, Working style, Boundaries). Create role → the new role's page. Refusals (duplicate key,
   invalid name) show inline on their field, focus it and keep every input.
3. RL-3 Ask HR to draft…: when the HR lead can run, a dialog asks what the role should do and the HR lead's
   draft opens in the New role editor, marked as HR's draft, for the person to review before creating; nothing is
   created by the draft alone. When the HR lead can't run the button is disabled with the visible, associated
   reason "The HR lead can't run right now, so it can't draft roles." (§1, our-org-in-the-app §2a).
4. RL-4 Role page: one named back link "Roles"; h2 the role's name; key, revision and last update (who, when);
   the body rendered from Markdown as headings, lists and paragraphs (never as HTML); Employees with this role
   (avatar, name linked to their profile, the word Agent or Person, title); History (each revision with who and
   when, the current one marked, earlier text one step away); Record (ID, file `roles/<id>.md`, key, schema
   version, created and updated attribution) (ROLE-1, ROLE-5).
5. RL-5 Edit (the page's one primary action) opens the editor with the current values and states the cost:
   "Saving makes rev N+1" and how many employees reference the role. Save role sends the revision the person
   saw. Unchanged values cannot be "saved" as a new revision.
6. RL-6 Revision conflict: the editor keeps the person's input, shows the newer version (revision, name, body)
   and the button names the effect ("Save over rev N"); no input is lost (ROLE-10).
7. RL-7 A key that a development process in use names cannot be changed or removed: the key field is read-only
   with the reason naming the development process (decision 7).
8. RL-8 States (ROLE-10): loading skeleton shaped like the rows; no roles ("Roles describe employees' work.
   Create one, or ask HR to draft one." — the HR clause only when the HR lead can run, so empty copy never
   promises HR can run); error with Retry; production unavailable (the section's NotBuilt placeholder, never
   sample data); not found; organization data unreachable ("Can't reach organization data. Showing the last copy
   from <time>." with Try again; changes are paused, the actions say why); invalid details ("<role>: details not
   recognized (<field>)." with Fix, which opens the editor at that field); viewer without edit rights sees no
   New role, Ask HR, Edit or Fix (hidden, §0.1).
9. RL-9 Focus (§6a): after client navigation to a role page, focus lands on the role's h2 (after the shell's own
   heading focus); closing the editor or dialog returns focus to its opener; creating a role moves to and focuses
   the new role's heading; a refusal focuses its field.
10. RL-10 Layout at 1440, 1024, 640, 390, 320 and 320 px with 200% text: no sideways scroll, controls 44 px
    below 640 px, state pills keep whole words, editor full width below 640 px with its buttons visible.
11. RL-11 No role grants authority: the editor and page say a role describes work and grants no tools, budgets or
    authority (decisions 3, 5).

## States per screen

List: loading · ready · empty (HR can / can't run) · error · unavailable · stale · invalid row · read-only viewer.
Role page: loading · ready · not found · error · unavailable · stale · invalid · read-only · key locked ·
no employees · single revision.
Editor: new · HR draft · edit · fix · saving (busy label, width kept) · field refusal · other failure · conflict.
Ask HR: idle · drafting (busy) · failure (input kept) · HR can't run (refused by the server).

## Architecture and ownership

- Module `src/lib/modules/roles/`: `types.ts`, `api.ts` (interface + production object of `unavailableCall`),
  `index.ts` (public: the two page components), `present.ts` (+ `present.test.ts`: body blocks, counts, draft
  checks, state words), `features/{list,role-page,editor,ask-hr}/`.
- Routes `src/routes/(shell)/settings/roles/+page.svelte` and `[roleId]/+page.svelte`: thin.
- Mock `e2e/fakes/modules/roles.ts` + `e2e/fakes/data/roles/roles.json`; employees from the org mock
  (`$lib/org/api`) so IDs match member profiles; role IDs match the Hire mock's role choices. Switches:
  `?mock=slow|fails|empty|unavailable|stale|viewer|hr-off|conflict|invalid`.
- `e2e/roles.spec.ts`, registered in `e2e/gate-manifest.json` (shell-look category, which already owns
  `settings/roles/**`).
- Not edited: shared components, ui primitives, nav, shell, settings frame, `package.json`, `docs/plans/*`,
  other modules.

## Non-goals (and why)

- Per-agent role overrides, approval routing, drift review, composition with provenance, role defaults for model
  or runtime, generated harnesses (roles-ux §4, ROLE-2/3/4/6/11).
- Profile › Role and choosing an employee's role (ROLE-9 write side): owned by the member profile slice
  (`/members/[memberId]` Role tab); this slice shows who references a role but does not reassign.
- Deleting a role: no spec flow exists.
- `agent-os role list/inspect` (ROLE-8) and instruction composition (ROLE-7): office/CLI work, not UI.
- Creating roles from a development process's definition (decision 7): server side; the UI shows the key lock.

## Checks

`bun run check`, `bun run test`, `bun run lint` (only the nine known Chat-route errors), `bun run format:check`,
`bun run build`; `e2e/roles.spec.ts` on Chromium; svelte-autofixer clean on every `.svelte` written; screenshots
at 1440 and 390 px of every screen and state in `agentic-org-ui-wt/checkpoints/shots/roles/`.
