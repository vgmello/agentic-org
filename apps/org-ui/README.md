# apps/org-ui: Agentic Org

The retained browser application uses Svelte 5, static SvelteKit 2, TypeScript and Tailwind v4, managed with Bun. Licensed UI stays inside this product application.

The Tauri shell, native installation/login controls and existing-team import are removed. Canonical domain fixtures and readers remain in `crates/org-office`, `crates/org-work` and `crates/org-proto`; `src/lib/office/wire-shapes.test.ts` retains the independent TypeScript wire gate.

Production and ordinary `bun run dev` show an explicit unavailable state for areas without a gateway adapter. No real HTTP routes or sign-in/setup behavior are claimed yet; those follow the approved gateway contract. Office, organization and work imports have no automatic browser mock fallback.
Other gated features retain sample factories until their scheduled module cutovers; the shell keeps those areas unavailable.

```sh
bun install --frozen-lockfile
bun run dev
bun run check
bun run test
bun run build
bun run test:e2e
```

Playwright starts an explicitly composed test-only fake server plus a separate static production build. Retained UI workflows run against the test fakes; production checks prove unserved areas stay unavailable and browser flags cannot enable fakes. Screenshots and reports stay ignored under `test-results/` and `playwright-report/`.

See [TECH-STACK.md](TECH-STACK.md), the [standalone design](../../docs/design/standalone-server.md), [project structure](../../docs/design/project-structure.md) and [removal inventory](../../docs/design/standalone-server-removal-inventory.md). Server setup and release packaging remain separate work.


## Gateway CSP browser gate (Part A)

The production build keeps assets external and checks emitted CSS and JavaScript for data URIs. It also refuses test CSP report directives, collector routes or middleware markers in built HTML, CSS and JavaScript. `bun run check:build-assets` also runs that check independently and fails when `build/` is missing. SSE parsing strings such as `data:` remain allowed.

```sh
bunx playwright install chromium webkit firefox
CSP_GW_PORT=4273 bun run test:e2e:csp-gateway --workers=2
```

This command builds the gateway's `csp_harness` example, reads its executable path from Cargo JSON, builds the UI, and runs Chromium, WebKit and Firefox against `http://127.0.0.1:<CSP_GW_PORT>`. Custom `CARGO_TARGET_DIR` values are supported. The harness uses the production HTTP service with an unavailable API backend. Each browser checks fresh routes and reload, exact CSP headers and bootstrap execution; deliberately blocked scripts prove capture works across documents and pages.

Capture screenshots stay ignored in `test-results-csp-gateway/`. Run this command, `test:e2e` and other builds sequentially within one worktree because they share build output. Part B runs the full interactive CSP sweep separately.

## Interactive CSP gate (Part B)

```sh
CSP_PW_PORT=4373 bun run test:e2e:csp --workers=2
```

The separate CSP config runs the existing Chromium UI suite against its own `vite dev --mode e2e` server, plus a route/dialog/sheet/toast smoke in Chromium, native WebKit and Firefox. Each browser's setup project first proves that a blocked inline script does not execute and reaches the report collector. The ordinary `test:e2e` configuration stays unchanged.

Only this server's explicit `ORG_UI_CSP=1` flag enables the serve-only policy middleware. It reads the shared production policy files, hashes each HTML response's inline scripts after normalizing line endings, strips conditional request headers, and adds the test-only report URI and script sampling. Non-HTML responses, including worker scripts, receive their own policy. No directive is relaxed. The server uses `node_modules/.vite-csp`; its screenshots stay ignored under `test-results-csp/`, and both CSP output directories are excluded from Vite's watcher.

Smoke tests wait for one quiet second before closing their browser. Teardown waits three seconds and fails for any remaining violation, collector error or HTML response without a policy header. Only reports matching a setup control's unique marker are cleared. Parts A and B are both required for UI changes after this companion lands; gateway changes also run Part A. These gates prove CSP behavior, with sign-in/Origin and worker execution coverage owned by their following features.
