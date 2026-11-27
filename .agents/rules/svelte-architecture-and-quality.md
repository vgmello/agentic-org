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
| Legacy files `> 700` lines (`large-files.allow`) | **strictly shrink-only**; delete once `<= 700` | `file-size-ratchet.test.ts` + CI `ui-large-files-list` |
| Plain `.css` imports (incl. `?inline`/`?raw`) & `<style>@import` outside `src/lib/styles/**` & root `+layout.svelte` | **banned** (breaks Svelte CSS scoping & `css_unused_selector`) | `eslint.config.js` + `file-size-ratchet.test.ts` |

- Count includes `<style>` blocks (`\n` count + trailing line). Never move `<style>` into an imported `.css` file to dodge the cap.
- Never place production code under `testing/`, `mocks/`, `fixtures/`, or `*.test.ts`.
- Allowlist entries can never be traded for new large files; new paths are allowed only for genuine `git diff -M` renames with `newLines <= oldLines`.
- Split by real responsibility: cohesive child Svelte components (with scoped `<style>`), `.svelte.ts` state/loader controllers, and pure `.ts` domain helpers — never `part2.svelte` or `misc.ts`.

---

## 2. Bundle Budgets & Non-UI Barrels

- **Never re-export Svelte components from `modules/<domain>/index.ts`.** Keep `index.ts` lightweight (types, pure helpers, API singletons); put shared cross-module Svelte components in `modules/<domain>/ui.ts` or explicit component paths (human/review check + enforced indirectly via bundle budgets).
- **`src/lib/platform/**` and `src/routes/(shell)/+layout.svelte` may never import domain barrels** (any `$lib/modules/<x>`, `/index`, or relative `../modules/<x>` form — enforced by `eslint.config.js` and tested in `gate-self-test.test.ts`).
- **Lazy-load heavy shell overlays** (command palette modal, creation dialogs, companion drawers) via dynamic `import()` on first open.
- **Shrink-only gzip budgets** (`apps/agentic-org-ui/bundle-budget.json`, enforced fail-closed by `scripts/check-build-assets.ts` + `bundle-budget.test.ts`):
  - `(shell)` and `(onboarding)` first-paint closures (including `client/entry.js`, `app.js`, root layout, and group layout) must stay within their JS, CSS, and total gzip budgets (shrink-only).
  - Every nested `(shell)` `+layout.svelte`, `(shell)` `+page.svelte`, and `(onboarding)` `+page.svelte` incremental JS closure must stay `<= 150 kB` gzip (legacy routes/layouts above `150 kB` are locked shrink-only in `bundle-budget.json`; stale overrides fail the build).

---

## 3. Layered Module Graph & Wire Choke-Point

Enforced via real TypeScript + Svelte AST import parsing across tracked and untracked files in `src/lib/platform/tooling/module-graph.test.ts`:

- **Zero cycles (`[]`)** across all `module/*` (including bare barrel imports), `components`, non-shell `platform/*`, and `src/lib/*` root modules (`lib-root:*`).
- **Layer boundaries:**
  - `platform/{http,auth,tooling,organizations}` never imports `modules/*` or `platform/shell`; non-Svelte `platform/*` never imports `components/**`.
  - `components/**` never imports `modules/*` (legacy `components -> platform/shell` exceptions are locked shrink-only in `layer-exceptions.allow`).
  - Cross-module imports go through public entries (`$lib/modules/<domain>` or `ui.ts`); legacy cross-module `/features/` imports are locked shrink-only in `cross-module-features.allow`.
- **Single untrusted-wire decode choke-point:** Use `$lib/platform/http/decode` (`asRec`, `asStr`, `asNum`, etc.) and `$lib/platform/http/wire`; never copy-paste local `asRec` decoders (review check).
- **No test fakes in production:** Never import types or values from `engine.ts`, `reducer.ts`, `world.ts`, or `testing/` into production code (enforced by `productionImportGuard`).

---

## 4. Security, Auth, CSP & Built-in Agent Immutability

- **Single auth & SSE choke-point (enforced by `eslint.config.js`):** All authenticated HTTP & SSE calls go through `appSession.controller()` (`read`, `submit`, `subscribeEvents`, `openSseStream`). Raw `fetch`, `window`/`globalThis`/`self.fetch`, destructured `fetch`, `EventSource`, `'X-Org-Session-Proof'` literals, and `SESSION_PROOF_HEADER` imports are banned outside `session-controller.ts`, `events.ts`, `json-http.ts`, and `wire.ts`.
- **Zero raw DOM HTML sinks (enforced by `eslint.config.js`):** Zero `{@html ...}`, `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `eval`, or `new Function()`.
- **Single safe-URL helper:** Validate every server-supplied or query-derived navigation target through `$lib/platform/http/safe-url` (`safeInternalPath` / `splitSafeHref`) or `$lib/platform/auth/return-target` (`safeReturnTarget`). Every `eslint-disable-next-line svelte/no-navigation-without-resolve` must route its pathname through `resolve()` and carry `-- <reason>`.
- **Links & buttons (enforced by `eslint.config.js`):** `svelte/no-target-blank` and `svelte/button-has-type` are errors.
- **Built-in agents (`HR` and `Tess` / `chief_of_staff`) are immutable (review + unit tests):** Check `isBuiltInAgent(member)` (trusting `member.builtin_key` exclusively when present) in both UI controls and client action helpers — never allow rename, handle change, pause/resume, home-team move, or offboard/delete.
- **Device-only storage (`localStorage` & `sessionStorage` — enforced by `eslint.config.js`):** Banned in new production files (global, `window`/`globalThis`/`self` member, or destructured) unless routed through a caller-scoped device-store helper registered with `appSession.onSessionEnded()`. Never claim device-only notes were posted to the office.
- **Strict CSP (`config/csp/html.txt`):** Zero `data:` URIs in built CSS/JS (`check-build-assets.ts`).

---

## 5. Svelte 5 Runes, State, Async & Lifecycle Hygiene

- **Runes enforced by compiler:** `svelte.config.js` compiles all `src/**` with `runes: true`. Never use Svelte 4 syntax (`export let`, `$:`, `on:`, `$app/stores`).
- **Typed async promise hygiene (enforced by `eslint.config.js`):** `@typescript-eslint/no-floating-promises` and `@typescript-eslint/no-misused-promises` are errors across all production `src/**/*.ts` and `src/**/*.svelte.ts` modules.
- **State discipline (review check):**
  - Use `$state.raw` for server snapshots and read-only fetched lists/objects.
  - Use `$state.snapshot` when passing reactive state across non-Svelte boundaries (`JSON.stringify`, `structuredClone`, external libs, HTTP bodies).
  - Any module-level `$state` holding session/org data MUST register cleanup via `appSession.onSessionEnded()`.
  - Prefer `$derived` / `$derived.by` and event handlers over `$effect`. Every `$effect` must synchronize with a real external system (DOM, timer, network subscription, browser event) — never copy props or mirror state.
- **Stable `{#each}` keys (review check):** Always key by a stable, unique ID deduplicated at the decoder/model boundary. Never use index keys or index-suffixed keys (`${label}-${idx}`) on reorderable, filterable, or interactive lists (index keys are allowed only for static, non-interactive text segments such as highlight spans).
- **Async race & resource teardown (review + controller race tests):** Every async loader uses a monotonic request generation counter + `AbortController` signal, ignores stale results after org/route switch or `onDestroy`, and pairs every listener, timer, `requestAnimationFrame`, and stream reader (`try/finally` `cancel()` + `releaseLock()`) with teardown.
- **Suppressions ratchet (enforced by `suppressions-ratchet.test.ts`):**
  - Inline ESLint configuration comments (`/* eslint ... */`, `/* eslint-env ... */`, `/* global ... */`) and file-wide/block `eslint-disable` comments are **strictly forbidden**.
  - Only line-scoped `eslint-disable-next-line` / `eslint-disable-line` with `-- <reason>` and `svelte-ignore` with an immediately preceding reason comment are permitted, and per-file counts of `eslint`, `svelte`, and `$effect` are locked shrink-only in `suppressions.allow`.

---

## 6. Accessibility, CSS Scoping & 7-Theme Tokens

- **Icon-only controls require a tooltip (owner rule, 2026-11-24 — review + Playwright):** Every icon-only button, link, or toggle MUST provide an accessible name (`aria-label`) AND a hover + keyboard-focus tooltip dismissible with `Escape` (WCAG 1.4.13, `bits-ui` `Tooltip` outside modals, or `$lib/components/ui/tooltip`'s `ModalTooltip` inside `<Dialog>`/`<Sheet>` because Bits UI `Tooltip.Content` mounts a `PopperLayer` `FocusScope` that pauses the modal focus trap). Panel `Escape` handlers must ignore `Escape` while `[role="tooltip"]` is open.
- **Scoped CSS only:** Keep component styles inside `<style>` blocks; use `:global(...)` only nested under a scoped parent selector.
- **Semantic tokens across all 7 themes (enforced by `look-tokens.test.ts`):** Zero hard-coded colors anywhere in `src/` (including `.svelte` and `.css`).
- **Reduced motion (review + Playwright):** Every custom animation/transition must respect `@media (prefers-reduced-motion: reduce)` / `motion-reduce:`.

---

## 7. Testing & Error Boundaries

- **Controller race tests:** Every `.svelte.ts` loader/controller must have a unit test proving stale in-flight responses are ignored after an org/scope switch or dispose.
- **Regression test per fix:** Every bug fix ships with a focused test that fails without the fix.
- **New routes:** Must render explicit loading, empty, and error states (and rely on `+error.svelte` / `<svelte:boundary>` where failures are isolated), plus a Playwright smoke test covering `prefers-reduced-motion: reduce`, axe accessibility checks, and at least two themes.

---

## 8. Verification Gate (`bun run gate`)

Before every UI commit, run the single mechanical gate from `apps/agentic-org-ui/`:

```sh
bun run gate
```

(`bun run format:check && bun run check && bun run lint && bun run test && bun run build` — runs Prettier baseline check, `svelte-check`, hardened `eslint --max-warnings=0`, all Vitest suites including `file-size-ratchet`, `module-graph`, `suppressions-ratchet`, `bundle-budget`, `gate-self-test`, `look-tokens`, and `ui-gate`, plus production `vite build` + fail-closed `check-build-assets`.)

**Human-only verification (cannot be automated by `bun run gate`):**
1. Inspect live Playwright screenshots across themes (including reduced motion) for any visible UI change.
2. Spot-check keyboard navigation, focus restoration, and Escape dismissal on any new overlay/drawer/dialog.
3. Confirm no scope shortcuts or silent fallbacks were introduced, then run a fresh local `code-reviewer` pass before merging to `v1`.
