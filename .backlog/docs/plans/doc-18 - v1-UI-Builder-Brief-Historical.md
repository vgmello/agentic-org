---
id: doc-18
title: v1 UI Builder Brief (Historical)
type: guide
created_date: '2026-11-21T00:16:00Z'
updated_date: '2026-11-21T00:16:00Z'
tags:
  - plans
  - ui
  - historical
---
# Common brief for v1 UI screen builders

> [!NOTE] Review note (2026-11-21) — Historical / Superseded
> This brief was written for an earlier batch of parallel `v1-ui` builders under a temporary review waiver, and contains hardcoded session trailers (`Co-Authored-By: Claude Fable 5.2`, `Claude-Session: ...`) and a checkout-specific path (`/home/user/agentic-org`). Per current `AGENTS.md` and `docs/workstreams/ui.md`, per-feature reviews are required and new UI work branches from `origin/v1`. Retained for historical context only; archive or replace before dispatching new UI builders.

Job: IMPLEMENT, as the principal designer. You build one step of `docs/plans/v1-ui-screens.md`.
The owner waived review until one final pass, so no kit task registration is needed; you
are authorized to commit and push on YOUR branch only.

## Where you work
- Your own git worktree and branch (given in your assignment). Never switch branches,
  never touch `/home/user/agentic-org` (the lead's checkout) or another step's worktree.
- App: `apps/agentic-org-ui` inside your worktree (dependencies installed).
- Commit after each screen works; push your branch (`git push -u origin <branch>`).
  End every commit message with exactly these trailers and put no model name elsewhere:
  Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015ExbMnPtGdYtxh9p9nqVU8

## Read first (and little else)
1. `.agents/skills/caveman/SKILL.md` (your report only).
2. `apps/agentic-org-ui/src/lib/components/README-screens.md`: how a screen is added here
   (module layout, mock convention, shared parts, page skeleton, class strings). Follow it.
3. `docs/specs/app-shell-ux.md` section 6a (the look) and the specs named in your assignment.
4. The persona files your agent definition names. The UI/UX Pro Max companion skill is disabled (owner,
   2026-11-18): do not check for, install or use it.
5. One existing page as a pattern, e.g. `src/routes/(shell)/teams/(views)/members/+page.svelte`.

## Rules
- **File ownership.** You own only the route folders and module folders named in your
  assignment, plus `e2e/fakes/modules/<your modules>.ts` and `e2e/fakes/data/<your modules>/`.
  Other steps run in parallel. Do NOT edit shared files: `src/lib/nav.ts`, `Sidebar.svelte`,
  `src/lib/platform/shell/*`, `src/routes/(shell)/+layout.svelte`, the shared parts in
  `src/lib/components/` (PageHeader, TabsNav, ...), `src/lib/components/ui/*`, `icons.ts`,
  `e2e/vite-composition.ts`, `package.json`, `docs/plans/*`. If you need a change there, do
  without it and list it in your report. You MAY import and reuse anything.
- **Layout.** Routes are thin. Types, api and state live in `src/lib/modules/<module>/`;
  each interaction is a folder under `features/<feature>/`. Reuse existing dialogs and
  sheets (`src/lib/components/*`, `src/lib/modules/*`) before writing new ones.
- **Mock data.** Declare the calls in `src/lib/modules/<module>/api.ts`; the production
  export refuses every call (`unavailableCall`). Put the mock in
  `e2e/fakes/modules/<module>.ts`, answering from JSON in `e2e/fakes/data/<module>/`.
  Use field names from the data contracts in `docs/design/organization-data/` where the
  entity exists there (read only the file you need). Mutations update the mock in memory so
  every interaction can be exercised. Reuse the IDs of the existing org mock (projects,
  teams, members) by reading them through `$lib/org/api` in your fake, as
  `e2e/fakes/modules/shell.ts` does. Never import a fake or JSON from `src/`.
- **Every state is real:** loading (skeleton), empty, error with Retry, and the states the
  spec names. Copy comes from the spec word for word where it gives it.
- **Lint needs** `resolve()` from `$app/paths` on every href and goto (cast computed paths
  `as '/'`). Page headings are `h2`; the top bar owns the `h1`. Title per app-shell §4.
- **Not in v1** items in a spec are not built. Create organization, Sign in, Register and
  the session dialogs belong to others: do not touch `(onboarding)` or `platform/auth`.
- **Existing Playwright specs:** do not run the suites, do not edit or delete them.

## Checks before each push (in `apps/agentic-org-ui`)
`bun run check`, `bun run test`, `bun run build`, and `bunx --no-install eslint <your files>`
with no errors in your files; `bunx --no-install prettier --write <your files>`.
(`bun run lint` has 25 known errors in auth files that are not yours.)

## Look at it
Start `bun run dev:mock -- --port <your port> --strictPort`, open `/welcome`, click the
first "Open", visit your routes. Take screenshots with Playwright (`require('playwright')`,
`chromium.launch()`) at 1440 and 390 px wide of every screen and main state into
a scratch folder outside the repository,
look at them, run the persona's consistency audit, and fix what is wrong. Stop your dev
server when done.

## Report (under 35 lines)
Routes and what each shows; modules and mock files added; shared-file changes you need the
lead to make; spec points you could not build and why; guesses about field value you made;
check results; screenshot paths.
