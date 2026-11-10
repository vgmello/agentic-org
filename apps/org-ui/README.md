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
