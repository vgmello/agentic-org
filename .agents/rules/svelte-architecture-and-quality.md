---
paths:
  - "apps/agentic-org-ui/**"
---

# Svelte 5 & SvelteKit Architecture, Security, and Quality Rules

These rules apply to every change under `apps/agentic-org-ui/`. `backlog/docs/doc-8 - Architecture-Rules.md` (parent `agentic-org-project`) governs cross-domain boundaries; this file defines the enforceable UI contract.

Every automated claim below is verified fail-closed by `src/lib/platform/tooling/gate-self-test.test.ts` and executed on every `bun run gate`.

---

## 1. Component & File Size Ratchet (`<= 500` LOC target)

| Rule | Value | Enforced by |
| --- | --- | --- |
| New / unlisted production files (tracked or untracked) | **`<= 500` lines** | `file-size-ratchet.test.ts` (vs. `merge-base origin/v1`) |
| Existing legacy files `501–700` lines | **may never grow** | `file-size-ratchet.test.ts` (base comparison) |
| Legacy files `> 700` lines (`large-files.allow`) | **`0` entries** (shrink-only; never re-add) | `file-size-ratchet.test.ts` + CI `ui-large-files-list` |
| Plain `.css` imports (incl. `?inline`/`?raw`) & `<style>@import` outside `src/lib/styles/**` & root `+layout.svelte` | **banned** (breaks Svelte CSS scoping & `css_unused_selector`) | `eslint.config.js` + `file-size-ratchet.test.ts` |

- Count includes `<style>` blocks (`\n` count + trailing line). Never move `<style>` into an imported `.css` file to dodge the cap.
- Never place production code under `testing/`, `mocks/`, `fixtures/`, or `*.test.ts`.
- Allowlist entries can never be traded for new large files; new paths are allowed only for genuine `git diff -M` renames with `newLines <= oldLines`.
- Split by real responsibility: cohesive child Svelte components (with scoped `<style>`), `.svelte.ts` state/loader controllers, and pure `.ts` domain helpers — never `part2.svelte` or `misc.ts`.

---

## 2. Bundle Budgets & Non-UI Barrels

- **Never re-export Svelte components from `modules/<domain>/index.ts` (enforced by AST gate in `module-graph.test.ts`).** Keep `index.ts` strictly non-UI (types, pure helpers, API singletons); put shared cross-module Svelte components in a focused `modules/<domain>/ui.ts` or direct component path. Avoid nested feature `index.ts` barrels that pull sibling tabs into nested layouts.
- **`src/lib/platform/**` and `src/routes/(shell)/+layout.svelte` may never import domain barrels** (any `$lib/modules/<x>`, `/index`, or relative `../modules/<x>` form — enforced by `eslint.config.js` and tested in `gate-self-test.test.ts`).
- **Lazy-load heavy shell overlays** (command palette modal, creation dialogs, companion drawers) via dynamic `import()` on first open.
- **Shrink-only gzip budgets** (`apps/agentic-org-ui/bundle-budget.json`, enforced fail-closed by `scripts/check-build-assets.ts` + `bundle-budget.test.ts`):
  - `(shell)` and `(onboarding)` first-paint closures (including `client/entry.js`, `app.js`, root layout, and group layout) must stay within their JS, CSS, and total gzip budgets (shrink-only).
  - Every nested `(shell)` `+layout.svelte`, `(shell)` `+page.svelte`, and `(onboarding)` `+page.svelte` incremental JS closure must stay `<= 150 kB` gzip (`nestedLayoutOverrides: {}` and `routeOverrides: {}` — **zero overrides allowed**; never add a route/layout override instead of splitting barrels or lazy-loading).

---

## 3. Layered Module Graph & Wire Choke-Point

Enforced via real TypeScript + Svelte AST import parsing across tracked and untracked files in `src/lib/platform/tooling/module-graph.test.ts`:

- **Zero cycles (`[]`)** across the entire graph: all `module/*` (including bare barrel imports), `components`, **`platform/*` (including `platform/shell` and `platform/workspace`)**, and `src/lib/*` root modules (`lib-root:*`).
- **Layer boundaries (`layer-exceptions.allow = 0`, `cross-module-features.allow = 0`):**
  - Keep pure workspace utilities/stores in `src/lib/platform/workspace/` (zero module dependencies) and shared shell chrome UI in `src/lib/components/shell/`, never creating cycles through `platform/shell`.
  - `platform/{http,auth,tooling,organizations,workspace}` never imports `modules/*` or `platform/shell`; non-Svelte `platform/*` never imports `components/**`.
  - `components/**` never imports `modules/*` or `platform/shell` (`layer-exceptions.allow` is locked at `0`).
  - Cross-module imports go only through public entries (`$lib/modules/<domain>` or `ui.ts`); cross-module `/features/` imports are banned (`cross-module-features.allow` is locked at `0`).
- **Single untrusted-wire decode choke-point:** Use `$lib/platform/http/decode` (`asRec`, `asStr`, `asNum`, etc.) and `$lib/platform/http/wire`; never copy-paste local `asRec` decoders (review check).
- **No test fakes in production:** Never import types or values from `engine.ts`, `reducer.ts`, `world.ts`, or `testing/` into production code (enforced by `productionImportGuard`).

---

## 4. Security, Auth, CSP & Built-in Agent Immutability

- **Single auth & SSE choke-point (enforced by `eslint.config.js`):** All authenticated HTTP & SSE calls go through `appSession.controller()` (`read`, `submit`, `subscribeEvents`, `openSseStream`). Raw `fetch`, `window`/`globalThis`/`self.fetch`, destructured `fetch`, `EventSource`, `'X-Org-Session-Proof'` literals, and `SESSION_PROOF_HEADER` imports are banned outside `session-controller.ts`, `events.ts`, `json-http.ts`, and `wire.ts`.
- **Zero raw DOM HTML sinks (enforced by `eslint.config.js`):** Zero `{@html ...}`, `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `eval`, or `new Function()`.
- **Single safe-URL helper:** Validate every server-supplied or query-derived navigation target through `$lib/platform/http/safe-url` (`safeInternalPath` / `splitSafeHref`) or `$lib/platform/auth/return-target` (`safeReturnTarget`). Every `eslint-disable-next-line svelte/no-navigation-without-resolve` must route its pathname through `resolve()` and carry `-- <reason>`.
- **Links & buttons (enforced by `eslint.config.js`):** `svelte/no-target-blank` and `svelte/button-has-type` are errors.
- **Built-in agents (`HR` and `Tess` / `chief_of_staff`) are immutable (review + unit tests):** Check `isBuiltInAgent(member)` / `isBuiltInHandle(kind, handle, role, builtinKey)` (trusting `member.builtin_key` exclusively when present, falling back to normalized built-in handles `hr`, `tess`, `chief-of-staff`, `tess-cos`, `tess-cos-N`) across UI controls (including org-chart `ChartEmployee.builtinKey`), client action helpers (`assertMutableAgent`), **and AI/Tess proposal guards** (`isProtectedBuiltInProposal` matching handles, display labels, AND known built-in member IDs) — never allow rename, handle change, **role change**, pause/resume, home-team move, or offboard/delete. Ship unit tests for every guard.
- **Device-only storage (`localStorage` & `sessionStorage` — enforced by `eslint.config.js`):** Banned in new production files (global, `window`/`globalThis`/`self` member, or destructured) unless routed through a caller-scoped device-store helper registered with `appSession.onSessionEnded()` that also sweeps org-scoped keys (including `uiPrefs.clearFavorites()` and all legacy org-scoped prefixes) on sign-out. Never label device-only notes as server-posted (e.g., use `"Save local note"`, not `"Send comment"`).
- **Strict CSP (`config/csp/html.txt`):** Zero `data:` URIs in built CSS/JS (`check-build-assets.ts`).

---

## 5. Svelte 5 Runes, State, Async & Lifecycle Hygiene

- **Required Svelte 5 skills & autofixer:** Follow the repo's vendored `svelte-core-bestpractices` and `svelte-code-writer` skills (do not edit vendored copies in `.agents/skills/`). Prefer `{@attach ...}` over `use:action` for new DOM attachments (Svelte 5.29+), and run the pinned Svelte MCP `svelte-autofixer` on every new or modified `.svelte` component before committing.
- **Runes enforced by compiler:** `svelte.config.js` compiles all `src/**` with `runes: true`. Never use Svelte 4 syntax (`export let`, `$:`, `on:`, `$app/stores`).
- **Typed async promise hygiene (enforced by `eslint.config.js`):** `@typescript-eslint/no-floating-promises` and `@typescript-eslint/no-misused-promises` are errors across all production `src/**/*.ts` and `src/**/*.svelte.ts` modules.
- **State & streaming discipline (review check):**
  - Use `$state.raw` for server snapshots and read-only fetched lists/objects.
  - Use `$state.snapshot` when passing reactive state across non-Svelte boundaries (`JSON.stringify`, `structuredClone`, external libs, HTTP bodies).
  - Mutate active streaming items in place (`O(1)`) on SSE token chunks instead of copying the full message/turn array (`O(N^2)`) on every delta.
  - Any module-level `$state` holding session/org data MUST register cleanup via `appSession.onSessionEnded()`.
  - Prefer `$derived` / `$derived.by` and event handlers over `$effect`. Every `$effect` must synchronize with a real external system (DOM, timer, network subscription, browser event) — never copy props, mirror state, or reset dialog/sheet form fields in an `$effect` (reset dialog/sheet state in `onOpenAutoFocus` / open handlers and clear `busy`/errors in `try/finally` blocks, even on early `!live()` returns; avoid `<Dialog.Root>`/`<Dialog.Content>` directly in favor of `$lib/components/ui/dialog`'s `DialogContent` so WebKit focus restoration is automatic).
- **Stable `{#each}` keys (review check):** Always key by a stable, unique ID deduplicated at the decoder/model boundary. Never use index keys or index-suffixed keys (`${label}-${idx}`) on reorderable, filterable, or editable/interactive lists (index keys are allowed only for static, non-interactive text segments such as highlight spans).
- **Async race & resource teardown (review + controller race tests):** Every async loader (including shared `Resource<T>` and domain live controllers like `HomeLiveState` and `ShellData`) uses a monotonic request generation counter + `AbortController` signal **forwarded all the way to the underlying HTTP/API transport** (not a dead local controller), exposes a `dispose()`/`destroy()` teardown method that aborts in-flight work, checks org/generation/aborted on **both** success and error paths (`catch`), ignores stale results after org/route switch or `onDestroy`, and pairs every listener, timer, `requestAnimationFrame` (`cancelAnimationFrame` in cleanup), and stream reader (`try/finally` `cancel()` + `releaseLock()`) with teardown.
- **Suppressions ratchet (enforced by `suppressions-ratchet.test.ts`):**
  - Inline ESLint configuration comments (`/* eslint ... */`, `/* eslint-env ... */`, `/* global ... */`) and file-wide/block `eslint-disable` comments are **strictly forbidden**.
  - Only line-scoped `eslint-disable-next-line` / `eslint-disable-line` with `-- <reason>` and `svelte-ignore` with an immediately preceding reason comment are permitted, and per-file counts of `eslint`, `svelte`, and `$effect` are locked shrink-only in `suppressions.allow`.

---

## 6. Accessibility, CSS Scoping & 7-Theme Tokens

- **Icon-only controls require a tooltip (owner rule, 2026-11-24 — review + Playwright):** Every icon-only button, link, or toggle (including message action triggers, reaction buttons, and inline search toggles — never native `title` alone) MUST provide an accessible name (`aria-label`) AND a hover + keyboard-focus tooltip dismissible with `Escape` (WCAG 1.4.13). Use `bits-ui` `Tooltip` outside modals, and `$lib/components/ui/tooltip`'s `ModalTooltip` inside `<Dialog>`/`<Sheet>` (Bits UI `Tooltip.Content` mounts a `PopperLayer` `FocusScope` that pauses the modal focus trap and breaks `Tab` wrap). Use `ModalTooltip`'s `{@attach}` lifecycle, `disabled={menuOpen}` on dropdown triggers, capture-phase `Escape`, touch-device guard, and `side="bottom-start"` near left edges to prevent viewport clipping. Panel `Escape` handlers must ignore `Escape` while `[role="tooltip"]` is open.
- **Overlay close focus restoration (including WebKit pointer-open):** `<DialogContent>` and `<SheetContent>` use `captureOverlayOpener(ref)` from `$lib/components/focus` (which checks `document.activeElement` for keyboard opens and falls back to the document capture-phase `pointerdown` interactive control tracker for WebKit mouse opens where `document.activeElement` stays on `<body>`, skipping `[role="menu"]`/`[role="listbox"]` items so triggers aren't clobbered), restoring focus on close only when `isConnected` and usable (not `disabled` / `inert` / hidden) so focus never lands on `<body>`.
- **Scoped CSS only:** Keep component styles inside `<style>` blocks; use `:global(...)` only nested under a scoped parent selector (e.g. `.chat-day:global(.is-stuck)`, never top-level `:global(...)`).
- **Semantic tokens across all 7 themes (enforced by `look-tokens.test.ts`):** Zero hard-coded colors anywhere in `src/` (including `.svelte`, `.css`, **and `.ts`** files). Keep the default theme attribute (`data-ui-theme="ocean"`) set on `src/app.html` so pre-hydration first paint never flashes unthemed CSS.
- **Reduced motion (review + Playwright):** Every custom animation/transition must respect `@media (prefers-reduced-motion: reduce)` / `motion-reduce:`.

---

## 7. Testing & Error Boundaries

- **100% `.svelte.ts` controller & security test coverage:** Every `.svelte.ts` state/loader/context controller must have a unit test proving stale in-flight responses are ignored, `AbortSignal` cancels superseded calls, and `appSession.onSessionEnded()` cleanup actually fires. Every security guard (`assertMutableAgent`, `isProtectedBuiltInProposal`, storage sweeps) must have a regression test.
- **Regression test per fix:** Every bug fix ships with a focused test that fails without the fix.
- **New routes:** Must render explicit loading, empty, and error states (and rely on `+error.svelte` / `<svelte:boundary>` where failures are isolated), plus a Playwright smoke test covering `prefers-reduced-motion: reduce`, axe accessibility checks, and at least two themes.

---

## 8. Verification Gate (`bun run gate`)

Before every UI commit, run the single mechanical gate from `apps/agentic-org-ui/`:

```sh
bun run gate
```

(`bun run format:check && bun run check && bun run lint && bun run test && bun run build && bun run audit:cadence` — runs Prettier baseline check, `svelte-check`, hardened `eslint --max-warnings=0`, all Vitest suites including `file-size-ratchet`, `module-graph`, `suppressions-ratchet`, `bundle-budget`, `gate-self-test`, `look-tokens`, and `ui-gate`, production `vite build` + fail-closed `check-build-assets`, plus the periodic audit cadence counter.)

**Human-only verification (cannot be automated by `bun run gate`):**
1. Inspect live Playwright screenshots across themes (including reduced motion) for any visible UI change.
2. Spot-check keyboard navigation, focus restoration, and Escape dismissal on any new overlay/drawer/dialog.
3. Confirm no scope shortcuts or silent fallbacks were introduced, then run a fresh local `code-reviewer` pass before merging to `v1`.

---

## 9. Periodic 6-Dimension Independent Audit Cadence

Official team process: `backlog/docs/processes/doc-105 - Process-Periodic-UI-6-Dimension-Audit.md` (parent `agentic-org-project`).

Mechanical gates prevent structural regressions immediately, but subtle drift (tooltip CSS loss, `$effect` form resets, incomplete `AbortSignal` chains, dead code) accumulates across iterations. Run a full independent 6-dimension audit (bar: **`>= 9.0/10` across all 6 dimensions, 0 open High/Medium findings**) whenever **any** trigger fires (tracked automatically by `apps/agentic-org-ui/audit-checkpoint.json` and reported on every `bun run gate`):

| Trigger | Threshold | Enforced / Surfaced by |
| --- | --- | --- |
| UI iteration count | Every **`10` non-merge commits** touching `apps/agentic-org-ui/src/**` since `lastPassSha` | `bun run audit:cadence` (inside `bun run gate`) |
| Elapsed time with UI churn | **`14` calendar days** with `>= 1` UI commit since `lastPassDate` | `bun run audit:cadence` (inside `bun run gate`) |
| Large UI change volume | **`1,500` changed lines** (insertions + deletions) in `apps/agentic-org-ui/src/**` since `lastPassSha` | `bun run audit:cadence` (inside `bun run gate`) |
| Major architectural event | Before closing a UI milestone, or after adding a new module, shell layout, or auth/CSP change | Lead agent / reviewer checklist |

**Required cadence procedure when `bun run audit:cadence` reports `ACTION REQUIRED` (or a milestone closes):**
1. Run `bun run gate` and a **fresh internal `code-reviewer` pass** first to catch obvious issues cheaply.
2. Run **`bun run audit:ui`** (invokes `claude -p --model claude-opus-5-5 --effort xhigh --permission-mode plan` against `lastPassSha..HEAD` with the strict 6-dimension rubric; writes the report to `/tmp/agentic-org-ui-audit-<sha>.md`).
3. Fix every High/Medium finding and any dimension `< 9.0/10`, re-run `bun run gate` + fresh internal review + `bun run audit:ui` until verdict is **`PASS`**.
4. Update `apps/agentic-org-ui/audit-checkpoint.json` with the verified `PASS` commit SHA, date, and 6-dimension scores in the next commit.
