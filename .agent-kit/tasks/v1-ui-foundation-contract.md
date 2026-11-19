# Slice contract: v1-ui-foundation — shared screen-state parts

## Outcome and acceptance criteria

- User outcome: none visible. Internal foundation for every v1 UI screen (docs/plans/v1-ui-screens.md, build order 1).
- Acceptance:
  - One `Resource` (src/lib/components/resource.svelte.ts) and one `Loaded` (src/lib/components/Loaded.svelte)
    replace the copies in `modules/members` and `modules/settings`. `Loaded` renders loading (skeleton),
    unavailable (NotBuilt with place+area, else an in-place line), error with Retry, empty, content.
  - Generic helpers used across modules move to `src/lib/components`: ConfirmDialog, CopyButton, focus.ts
    (neighbourFocus), classes.ts, pills.ts. Domain code (OneTimeLink, sign-in access) stays in its module.
  - No module imports another module's internals for these parts (teams→members, offices→settings removed).
  - Behaviour of Teams index, Settings General and Offices unchanged, except a failed reload of the Teams list
    now drops the stale list and shows the error (the settings rule: no old answer beside a failure).
  - README-screens.md documents the parts; gate manifest declares them shared.
  - `bun run check`, `bun run test`, `bun run build`, `bun run format:check` pass in apps/agentic-org-ui; lint has
    only the nine known Chat-route errors.
- Non-goals: moving team/people dialogs out of src/lib/components (done with the screens that own them).

## Plan and readiness

- Parent plan: docs/plans/v1-ui-screens.md "Build order". Pattern: the existing settings Resource/Loaded.
- Readiness: READY. No plan review needed: mechanical consolidation, no new interface beyond the two parts.

## Ownership

- Owner and author: lead session (Claude Opus 5.5, claude-opus-5-5), branch v1-ui-foundation from v1-ui bfcc07e4.
- Reviews: Codex review cycle (kit_codex.py), fresh final review.
