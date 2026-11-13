# Agentic Org browser UI: tech stack

V1 is a standalone Rust server on macOS/Linux serving a browser application from HQ.
See the [server design](../../docs/design/standalone-server.md),
[project structure](../../docs/design/project-structure.md) and
[removal inventory](../../docs/design/standalone-server-removal-inventory.md).
The Tauri shell, adapters, packages, workspace member and native installation scripts
have been removed. Tauri is deferred to v2. The import feature remains retired.

The application lives in `apps/org-ui` (package `org-ui`); the worker is
`apps/org-server` (`org-server`). Licensed UI remains inside this product application.
Versions are pinned in `package.json`; `bun.lock` is the frontend lockfile.

## Current implementation

| Concern | Choice | Notes |
| --- | --- | --- |
| UI framework | SvelteKit 2 + Svelte 5 runes | Static SPA, adapter-static, ssr=false, index.html fallback. |
| Language | Strict TypeScript | Runtime wire readers and feature adapters land separately. |
| Styling | Tailwind CSS v4 | CSS-first configuration and tokens in src/app.css; dark theme. |
| Look | Tailwind Plus application UI, ported to Svelte | Licensed inside apps/org-ui, never as a separate UI kit. |
| Components | shadcn-svelte on Bits UI | Shared primitives under src/lib/components/ui. |
| Icons and font | Lucide/Heroicons; bundled Inter Variable | No remote font dependency. |
| Package manager | Bun | Frozen install, check, test and build scripts. |
| Unit tests | Vitest | Domain behavior, test fakes, canonical vectors and platform boundaries. |
| Browser tests | Playwright Chromium | Retained flows use explicit test-owned dev composition; a second project checks the static production release. |
| Rust | Server, CLI and domain workspace members | No UI Rust workspace member remains. |
| Contracts | org-proto; legacy TypeScript mirrored in src/lib/proto.ts | Independent office/wire-shapes.test.ts remains. Shared org/work vectors live in crates/org-office/test-vectors and crates/org-work/test-vectors. |
| Production availability | Shell capability gate and unavailable APIs | No native global, query parameter or remembered sample data enables production fakes. |

Real HTTP feature adapters are separate deliveries. The shell defaults every product
route to unavailable until its adapter is explicitly composed. Framework 404s retain
the error page. Office, organization and work API defaults reject as unavailable;
remaining feature factory cutovers follow their module handoffs.

`e2e/vite-composition.ts` selects test fakes only for the explicit `e2e` dev-server
mode. Every build, including `--mode e2e`, retains production availability. Fakes
preserve browser-test query scenarios. They are not a server or persistence
implementation. Production authority belongs to the Rust domain owners.

## Screenshot evidence

Use the shared `e2e/shot.ts` helper: `await shot(page, 'screen-state')`. It waits for running finite transitions, then writes a full-page image to `test.info().outputPath('screen-state.png')`, isolated by test, worker and retry. Looping spinners and paused animations are excluded from settling because they never finish. The configured screenshot after every test remains enabled.

For a loading or other in-progress state, use `await shot(page, 'screen-loading', { settle: false })` so a finite transition cannot finish the state before capture. Assert the state first; control timers where necessary to keep transient states deterministic.

A full-page screenshot captures the page scroll area, but does not expand a fixed dialog's internal scroll area. Add a useful region with `await shot(page, 'screen-error', { context: panel })`: it also writes `screen-error-context.png` and may scroll that region into view. For long dialogs, capture the header and lower content at deliberate scroll positions as separate named states. Inspect the images before review and include their paths. Keep all output in ignored `test-results/` or `playwright-report/`; never commit screenshots.

## Run it

```sh
cd apps/org-ui
bun install --frozen-lockfile
bunx playwright install chromium
bun run dev                 # unserved areas are unavailable
bun run check
bun run test
bun run build
PW_PORT=4373 bun run test:e2e --workers=1
```

`test:e2e` builds before either server starts. The fake-backed dev server uses
`PW_PORT` (default 4173); production preview uses the next port. Choose a free pair
on the shared machine. Do not run another check or build against the same
`.svelte-kit` directory while those servers are running.

At the repository root, use `CARGO_BUILD_JOBS=2` with workspace Clippy and tests.
The toolchain is pinned by `rust-toolchain.toml`. Tests use temporary directories,
never a user's organization workspace.

## Follow-ups

- Gateway routes, wire readers and per-feature factories follow the approved contracts.
- TypeScript/Svelte lint and formatting tools are not configured; current checks use Svelte and TypeScript diagnostics.
- Browser security headers and server authorization belong to the gateway delivery; removed Tauri capabilities do not provide browser protection.
- English copy remains inline; keyboard and screen-reader behavior is covered by UI primitives and browser tests.
