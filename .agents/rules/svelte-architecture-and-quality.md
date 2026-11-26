---
paths:
  - "apps/agentic-org-ui/**"
---

# Svelte architecture, security and quality rules (9/10 bar)

These rules apply to every change under `apps/agentic-org-ui/`. They exist because the
2026-11-26 Claude Opus `xhigh` UI audit found 42 production files over 700 lines, 2 source
dependency cycles, barrel imports pulling 2.1 MB of JS into every shell page's first paint,
18 duplicated wire parsers, an unguarded Chat reload race, and missing UI/client guards on
built-in agents (Milestone `m-7`, `TASK-225`..`TASK-230`).

`backlog/docs/doc-8 - Architecture-Rules.md` (parent `agentic-org-project`) still governs
domain boundaries. This file adds the enforceable Svelte 5 / SvelteKit rules inside
`apps/agentic-org-ui/`.

---

## 1. File size ratchet: aim for <= 500 lines; 700 is the hard cap

| Rule | Value |
| --- | --- |
| Target for a production `.svelte` / `.ts` / `.js` / `.css` file | **500 lines or fewer** |
| Hard cap for any unlisted production file | **700 lines** |
| Legacy files over 700 lines | listed in `apps/agentic-org-ui/large-files.allow`; **a listed file may only shrink** |
| Enforced by | `src/lib/platform/tooling/file-size-ratchet.test.ts` (Vitest + CI `ui-large-files-list` shrink-only job) |

- Every line counts (`\n` count, plus trailing unterminated line), including `<style>` blocks.
- Production vs. non-production is decided by `isNonProductionModule` in
  `src/lib/platform/tooling/production-import-guard.ts`. Never place production code under
  `testing/`, `mocks/`, `fixtures/`, or `*.test.ts` to bypass the cap.
- **When touching a listed file:** keep net line count `<= 0`. If new behavior is needed,
  extract a cohesive responsibility (`<= 500 LOC`) first, update `large-files.allow` to the
  exact new line count, and delete the entry once `<= 700`.
- **No mechanical splitting** (`part2.svelte`, `misc.ts`). Split by responsibility:
  - `.svelte.ts` state/loader controller
  - cohesive sub-components (header, toolbar, list/card, form/dialog)
  - pure domain/mapping helpers (`.ts`)

```sh
cd apps/agentic-org-ui
bunx --no-install vitest run src/lib/platform/tooling/file-size-ratchet.test.ts
# Print exact counts over cap:
UI_FILE_SIZE_RATCHET_PRINT=1 bunx --no-install vitest run src/lib/platform/tooling/file-size-ratchet.test.ts
# Run CI shrink-only comparison against origin/v1:
UI_FILE_SIZE_RATCHET_BASE=$(git merge-base HEAD origin/v1) \
  bunx --no-install vitest run src/lib/platform/tooling/file-size-ratchet.test.ts
```

---

## 2. Bundle hygiene & non-UI barrels

- **Never re-export Svelte components from a domain's default `index.ts` barrel** when
  `platform/**` or other modules import types/APIs from it. A barrel forces Rollup/Rolldown
  to bundle every re-exported feature component and stylesheet into the importing chunk.
  - Keep `modules/<domain>/index.ts` lightweight (types, pure helpers, API singletons).
  - Put shared cross-module Svelte components in explicit feature paths or `modules/<domain>/ui.ts`.
  - **`platform/**` may never import domain barrels** (enforced by ESLint `no-restricted-imports`).
- **Lazy-load heavy shell overlays** (command palette results, creation dialogs, expanded
  companion panels) via dynamic `import()` on first open.
- **Target budgets:** Shell first-paint JS `<= 280 kB` gzip (`<= 200 kB` once overlays are
  lazy-loaded); route-specific JS `<= 150 kB` gzip on top of the shell.

---

## 3. Zero source dependency cycles & single wire choke-point

- **Zero module cycles (`doc-8`).** Enforced by `src/lib/platform/tooling/module-graph.test.ts`.
  Existing legacy cycles (if any remain during Wave 2) are explicitly allowlisted as shrinking
  debt; no new cycle edge is ever allowed.
- **Shared components (`src/lib/components/**`) never import `$lib/modules/**`.** Pass data
  and callbacks via props/snippets.
- **Never import cross-module internals** (`$lib/modules/<other>/features/...`). Always import
  from the owning module's public entry (`$lib/modules/<other>` or `$lib/modules/<other>/ui`).
- **One untrusted-wire decode choke-point.** Use shared helpers in `$lib/platform/http/decode`
  (and `$lib/platform/http/wire`); never copy-paste local `asRec` / `asStr` / `asNum` decoders
  in new adapters.
- **Contract types never live in test fakes.** Never import types or values from `engine.ts`,
  `reducer.ts`, `world.ts`, or `testing/` into production code (`productionImportGuard` enforces
  runtime isolation; keep types in `types.ts` / `rules.ts`).

---

## 4. Security, auth, CSP & built-in agent immutability

- **All authenticated HTTP & SSE calls go through `appSession.controller()`** (`read`,
  `submit`, `subscribeEvents`, `openSseStream`). Never call raw `globalThis.fetch` directly in
  `src/lib/modules/**`, and never leak `X-Org-Session-Proof` outside `session-controller.ts`.
- **Zero `{@html ...}` or `innerHTML`** in production Svelte components. Render structured
  segments via plain Svelte text interpolation. Validate every server-supplied navigation URL
  through a safe same-origin path check rejecting `//`, `/\`, external schemes, and ASCII
  control characters.
- **Built-in agents (`HR` and `Tess` / `chief_of_staff`) are immutable.** Always check
  `isBuiltInAgent(member)` in both UI menus/dialogs AND client action helpers:
  - Never allow rename, handle change, pause/resume, home-team move, or offboard/delete.
  - Trust `member.builtin_key` exclusively when present.
- **Device-only (`localStorage`) state must be honest and caller-scoped.** Never claim
  device-only notes/comments were "posted" to the office, and clear caller-scoped local state
  on sign-out or account switch (`appSession.onSessionEnded`).
- **Strict CSP (`config/csp/html.txt`):** No `data:` URIs in built CSS/JS (`check-build-assets`).
  Any new browser media preview (`blob:`) requires an explicit CSP update + test.

---

## 5. Svelte 5 reactivity, async races & lifecycle hygiene

- **Runes only:** Never introduce Svelte 4 syntax (`export let`, `$:`, `on:`, `$app/stores`).
- **Prefer `$derived` / `$derived.by` and event handlers over `$effect`.** Never use `$effect`
  merely to copy props into `$state` or mirror one `$state` variable into another.
- **Guard every async loader against races and unmount:** Use a monotonic request generation
  counter + `AbortController` signal, debounce rapid live-event refetches, and ignore stale
  responses after org switch, route change, or `onDestroy`.
- **Clean up every resource:** Every `addEventListener`, `setInterval`, `setTimeout`,
  `requestAnimationFrame`, `AbortController`, and stream reader (`try/finally` `cancel()` +
  `releaseLock()`) must have a paired teardown.
- **Disambiguate `{#each}` keys** whenever data comes from server/model/user input
  (e.g. `` `${label}-${idx}` ``) to prevent Svelte 5 `each_key_duplicate` runtime crashes.

---

## 6. Accessibility, reduced motion & 7-theme tokens

- **Icon-only controls require a tooltip (owner rule, 2026-11-24):** Every icon-only button,
  link, or toggle MUST have an accessible name (`aria-label`) AND a hover + keyboard-focus
  tooltip dismissible with `Escape` (WCAG 1.4.13, `bits-ui` `Tooltip`). Panel-level `Escape`
  handlers must ignore `Escape` while `[role="tooltip"]` is open.
- **Zero hard-coded colors in `.svelte` files.** Use semantic CSS tokens (`var(--exec-*)`,
  `var(--tone-*)`, Tailwind semantic utilities) verified across all 7 themes (`look-tokens.test.ts`).
- **Respect `prefers-reduced-motion: reduce`** on every custom transition/animation.

---

## 7. Pre-commit 7-point self-check & periodic audit cadence

Before **every** UI commit, verify all 7 points:
1. File sizes: `file-size-ratchet.test.ts` green; no new file `> 700` LOC (target `<= 500`).
2. Module graph & barrels: `module-graph.test.ts` green; no new cycles; no component barrels pulled into `platform/**`.
3. Auth & XSS: zero raw `fetch`, zero `{@html}`, safe URL checks, built-in HR/Tess guards intact.
4. Async & lifecycle: monotonic gen + `AbortController` on loaders; every listener/timer/rAF cancelled.
5. Keys & reactivity: no state-syncing `$effect`; unique `{#each}` keys; `svelte-autofixer` returns 0 issues.
6. Accessibility & themes: tooltips on all icon-only controls; `prefers-reduced-motion`; semantic tokens only.
7. Gates green: `bun run check`, `bun run lint`, `bunx --no-install vitest run`, `bun run build` (+ `check-build-assets`), and live screenshots for visible UI changes.

### Periodic Architecture Audit Cadence (Keep Score >= 9/10)
- **Trigger-based (primary):** Run a fresh read-only Opus `xhigh` UI architecture audit after
  any major UI milestone or wave (`> 1,000 LOC` net UI change, new route group, or new domain
  module).
- **Standing safety net (low frequency):** Run a lightweight biweekly audit (1st & 15th of
  each month) checking ratchet shrink progress, module cycles, shell/route gzip budgets, and
  prototype delta — creating follow-up tasks only if any dimension drops below 9/10.
