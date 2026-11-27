---
paths:
  - "apps/agentic-org-ui/**"
---

# Svelte 5 & SvelteKit Architecture, Security, and Quality Rules

These rules apply to every change under `apps/agentic-org-ui/`. `backlog/docs/doc-8 - Architecture-Rules.md` (parent `agentic-org-project`) governs cross-domain boundaries; this file defines the enforceable UI contract.

---

## 1. Component & File Size Ratchet (`<= 500` LOC target)

| Rule | Value | Enforced by |
| --- | --- | --- |
| New / unlisted production files | **`<= 500` lines** | `file-size-ratchet.test.ts` (vs. `merge-base origin/v1`) |
| Existing legacy files `501–700` lines | **may never grow** | `file-size-ratchet.test.ts` (base comparison) |
| Legacy files `> 700` lines (`large-files.allow`) | **strictly shrink-only**; delete once `<= 700` | `file-size-ratchet.test.ts` + CI `ui-large-files-list` |
| Plain `.css` imports under `src/lib/{modules,components,platform}` | **banned** (breaks Svelte CSS scoping & `css_unused_selector`) | `eslint.config.js` + `file-size-ratchet.test.ts` |

- Count includes `<style>` blocks (`\n` count + trailing line). Never move `<style>` into an imported `.css` file to dodge the cap.
- Never place production code under `testing/`, `mocks/`, `fixtures/`, or `*.test.ts`.
- Allowlist entries can never be traded for new large files; new paths are allowed only for genuine `git diff -M` renames with `newLines <= oldLines`.
- Split by real responsibility: cohesive child Svelte components (with scoped `<style>`), `.svelte.ts` state/loader controllers, and pure `.ts` domain helpers — never `part2.svelte` or `misc.ts`.

---

## 2. Bundle Budgets & Non-UI Barrels

- **Never re-export Svelte components from `modules/<domain>/index.ts`.** Keep `index.ts` lightweight (types, pure helpers, API singletons); put shared cross-module Svelte components in `modules/<domain>/ui.ts` or explicit component paths.
- **`src/lib/platform/**` and `src/routes/(shell)/+layout.svelte` may never import domain barrels** (any `$lib/modules/<x>`, `/index`, or relative `../modules/<x>` form — enforced by ESLint).
- **Lazy-load heavy shell overlays** (command palette modal, creation dialogs, companion drawers) via dynamic `import()` on first open.
- **Shrink-only gzip budgets** (`apps/agentic-org-ui/bundle-budget.json`, enforced fail-closed by `scripts/check-build-assets.ts` + `bundle-budget.test.ts`):
  - Shell first-paint JS + CSS closure must stay within `bundle-budget.json` (tightly ratcheted to measured + ~5%, shrink-only).
  - Every `(shell)` page route's incremental JS closure must stay `<= 150 kB` gzip (legacy routes above 150 kB are locked shrink-only in `routeOverrides`).

---

## 3. Layered Module Graph & Wire Choke-Point

Enforced via real TypeScript + Svelte AST import parsing in `src/lib/platform/tooling/module-graph.test.ts`:

- **Zero cycles (`[]`)** across all `module/*`, `components`, and non-shell `platform/*` units.
- **Layer boundaries:**
  - `platform/{http,auth,tooling,organizations}` never imports `modules/*` or `platform/shell`.
  - `components/**` never imports `modules/*` (legacy `components -> platform/shell` exceptions are locked shrink-only in `layer-exceptions.allow`).
  - Cross-module imports go through public entries (`$lib/modules/<domain>` or `ui.ts`); legacy cross-module `/features/` imports are locked shrink-only in `cross-module-features.allow`.
- **Single untrusted-wire decode choke-point:** Use `$lib/platform/http/decode` (`asRec`, `asStr`, `asNum`, etc.) and `$lib/platform/http/wire`; never copy-paste local `asRec` decoders.
- **No test fakes in production:** Never import types or values from `engine.ts`, `reducer.ts`, `world.ts`, or `testing/` into production code (`productionImportGuard`).

---

## 4. Security, Auth, CSP & Built-in Agent Immutability

- **Single auth & SSE choke-point:** All authenticated HTTP & SSE calls go through `appSession.controller()` (`read`, `submit`, `subscribeEvents`, `openSseStream`). Raw `fetch`, `window.fetch`, `globalThis.fetch`, and `X-Org-Session-Proof` are banned outside `session-controller.ts`, `events.ts`, and `json-http.ts` (enforced by ESLint).
- **Zero raw DOM HTML sinks:** Zero `{@html ...}`, `innerHTML`, `outerHTML`, `insertAdjacentHTML`, or `eval` (enforced by ESLint).
- **Single safe-URL helper:** Validate every server-supplied or query-derived navigation target through `$lib/platform/http/safe-url` (`safeInternalPath` / `splitSafeHref`) or `$lib/platform/auth/return-target` (`safeReturnTarget`). Every `eslint-disable svelte/no-navigation-without-resolve` must route its pathname through `resolve()` and carry `-- <reason>`.
- **Links & buttons:** `svelte/no-target-blank` and `svelte/button-has-type` are errors.
- **Built-in agents (`HR` and `Tess` / `chief_of_staff`) are immutable:** Check `isBuiltInAgent(member)` (trusting `member.builtin_key` exclusively when present) in both UI controls and client action helpers — never allow rename, handle change, pause/resume, home-team move, or offboard/delete.
- **Device-only storage (`localStorage`):** Banned in new files by ESLint unless routed through a caller-scoped device-store helper registered with `appSession.onSessionEnded()`. Never claim device-only notes were posted to the office.
- **Strict CSP (`config/csp/html.txt`):** Zero `data:` URIs in built CSS/JS (`check-build-assets.ts`).

---

## 5. Svelte 5 Runes, State, Async & Lifecycle Hygiene

- **Runes enforced by compiler:** `svelte.config.js` compiles all `src/**` with `runes: true`. Never use Svelte 4 syntax (`export let`, `$:`, `on:`, `$app/stores`).
- **State discipline:**
  - Use `$state.raw` for server snapshots and read-only fetched lists/objects.
  - Use `$state.snapshot` when passing reactive state across non-Svelte boundaries (`JSON.stringify`, `structuredClone`, external libs, HTTP bodies).
  - Any module-level `$state` holding session/org data MUST register cleanup via `appSession.onSessionEnded()`.
  - Prefer `$derived` / `$derived.by` and event handlers over `$effect`. Every `$effect` must synchronize with a real external system (DOM, timer, network subscription, browser event) — never copy props or mirror state.
- **Stable `{#each}` keys:** Always key by a stable, unique ID deduplicated at the decoder/model boundary. Never use index keys or index-suffixed keys (`${label}-${idx}`) on reorderable, filterable, or interactive lists (index keys are allowed only for static, non-interactive text segments such as highlight spans).
- **Async race & resource teardown:** Every async loader uses a monotonic request generation counter + `AbortController` signal, ignores stale results after org/route switch or `onDestroy`, and pairs every listener, timer, `requestAnimationFrame`, and stream reader (`try/finally` `cancel()` + `releaseLock()`) with teardown.
- **Suppressions ratchet (`suppressions-ratchet.test.ts`):** Every `eslint-disable` must carry `-- <reason>`, every `svelte-ignore` must have an explanatory reason comment immediately before it, and per-file counts of `eslint-disable`, `svelte-ignore`, and `$effect` are locked shrink-only in `suppressions.allow`.

---

## 6. Accessibility, CSS Scoping & 7-Theme Tokens

- **Icon-only controls require a tooltip (owner rule, 2026-11-24):** Every icon-only button, link, or toggle MUST provide an accessible name (`aria-label`) AND a hover + keyboard-focus tooltip dismissible with `Escape` (WCAG 1.4.13, `bits-ui` `Tooltip`). Panel `Escape` handlers must ignore `Escape` while `[role="tooltip"]` is open.
- **Scoped CSS only:** Keep component styles inside `<style>` blocks; use `:global(...)` only nested under a scoped parent selector.
- **Semantic tokens across all 7 themes:** Zero hard-coded colors anywhere in `src/` (including `.svelte` and `.css` — enforced by `look-tokens.test.ts`).
- **Reduced motion:** Every custom animation/transition must respect `@media (prefers-reduced-motion: reduce)` / `motion-reduce:`.

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

(`bun run check && bun run lint && bun run test && bun run build` — runs `svelte-check`, hardened `eslint --max-warnings=0`, all Vitest suites including `file-size-ratchet`, `module-graph`, `suppressions-ratchet`, `bundle-budget`, `look-tokens`, and `ui-gate`, plus production `vite build` + fail-closed `check-build-assets`.)

**Human-only verification (cannot be automated by `bun run gate`):**
1. Inspect live Playwright screenshots across themes (including reduced motion) for any visible UI change.
2. Spot-check keyboard navigation, focus restoration, and Escape dismissal on any new overlay/drawer/dialog.
3. Confirm no scope shortcuts or silent fallbacks were introduced, then run a fresh local `code-reviewer` pass before merging to `v1`.
