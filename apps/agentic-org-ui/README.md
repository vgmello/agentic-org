# apps/agentic-org-ui

The browser application of Agentic Org: a static SvelteKit 2 application with
Svelte 5, TypeScript and Tailwind CSS 4, built with shadcn-svelte on Bits UI and
managed with Bun. It builds to static assets that `agentic-org-web` serves; it has no
server of its own. Licensed Tailwind Plus components stay inside this application.

## What it does today

No screen talks to a real server yet: `agentic-org-web` does not open a listener
(increment A3 of the [implementation plan](../../docs/implementation-plan.md)).

- A production build, and ordinary `bun run dev`, show every area without a server
  adapter as unavailable. No flag, query parameter or stored value enables test data
  in a production build.
- The screens and flows exist and run against test fakes, which only the explicit
  `e2e` dev mode composes (`e2e/vite-composition.ts`). Each increment connects its
  screens to `/api/v1`.

See [TECH-STACK.md](TECH-STACK.md) for the stack and screenshot conventions, and
[AGENTS.md](AGENTS.md) for the Svelte rules that apply here.

## Checks

```sh
bun install --frozen-lockfile
bun run dev
bun run check
bun run test
bun run build
bun run test:e2e
```

Playwright starts the fake-backed dev server and a separate static production build.
Product flows run against the fakes; the production project proves that unserved
areas stay unavailable. Screenshots and reports stay in the ignored `test-results/`
and `playwright-report/`. [The tiered UI gate](e2e/README-gate.md) selects which
browser tests a change needs.

## Content security policy checks

The production build keeps assets external and refuses data URIs in emitted CSS and
JavaScript, and any test-only CSP report directive, collector route or middleware
marker in the built output. `bun run check:build-assets` runs that check on its own
and fails when `build/` is missing.

**Against the web server** (`test:e2e:csp-gateway`):

```sh
bunx playwright install chromium webkit firefox
CSP_GW_PORT=4273 bun run test:e2e:csp-gateway --workers=1
```

It builds the `csp_harness` example of `apps/agentic-org-web`, builds the UI, and
runs Chromium, WebKit and Firefox against `http://127.0.0.1:<CSP_GW_PORT>`. The
harness uses the production HTTP service with a backend that refuses every API call.
Each browser checks fresh routes and reload, the exact CSP headers and the bootstrap
script; deliberately blocked scripts prove that violations are captured. Output stays
in the ignored `test-results-csp-gateway/`.

**Interactive** (`test:e2e:csp`):

```sh
CSP_PW_PORT=4373 bun run test:e2e:csp --workers=1
```

It runs the Chromium UI suite against its own `vite dev --mode e2e` server, plus a
route, dialog, sheet and toast smoke test in Chromium, WebKit and Firefox. Each
browser first proves that a blocked inline script does not run and reaches the
report collector. Only this server's `ORG_UI_CSP=1` enables the test-only policy
middleware, which reads the shared production policy files and relaxes no directive.
Output stays in the ignored `test-results-csp/`.

Teardown fails on any remaining violation, collector error or HTML response without a
policy header. UI changes run both; changes to `apps/agentic-org-web` run the first.
Run them, `test:e2e` and builds one at a time within a worktree: they share build
output.
