# Agentic Org desktop: tech stack

**V1 direction changed 2026-11-09:** the target is a standalone Rust server
on macOS/Linux serving the browser application from HQ. Tauri is deferred to v2.
See the [server design](../../docs/design/standalone-server.md) and [deletion inventory](../../docs/design/standalone-server-removal-inventory.md).
This documentation change specifies the pivot; the source described below is the
inspected pre-pivot implementation. Its Tauri/bundle commands are historical
development instructions, not installation instructions for the new v1. Do not
claim the new CLI, HTTP server or remote sync exists until implemented. No migration
of the undeployed desktop app or its data is required.

**Target naming:** retain the browser UI as `apps/org-ui` (package `org-ui`) and
rename the worker app/package/binary to `apps/org-server` / `org-server`. See the
[proposed project structure](../../docs/design/project-structure.md). This file's
current source paths and desktop commands are pre-pivot evidence; the docs PR
performs no directory moves. Licensed UI stays inside the renamed UI application.

What `apps/desktop` is built with, why, and what is still undecided. Versions are pinned exactly in `package.json` and `src-tauri/Cargo.toml`.

## Decided

| Concern | Choice | Notes |
| --- | --- | --- |
| Shell | **Tauri 2** (Rust) | macOS first. Native overlay title bar, menu-bar item, Dock hiding. |
| UI framework | **SvelteKit 2 + Svelte 5** (runes), static SPA | `adapter-static`, `ssr = false`, `index.html` fallback. |
| Language | **TypeScript** (strict) | |
| Styling | **Tailwind CSS v4** | CSS-first config in `src/app.css`. Dark only. |
| Look | **Tailwind Plus** application UI, ported to Svelte | Matches the approved prototype. Allowed **inside `apps/desktop` only** (see AGENTS.md). |
| Extra components | **shadcn-svelte** on **Bits UI** | Generated into `src/lib/components/ui`. Tokens in `app.css` map onto the Tailwind Plus palette. Add with `bunx shadcn-svelte@latest add <name>`. |
| Icons | `@lucide/svelte` (shadcn), Heroicons paths (ported sidebar) | |
| Font | Inter Variable, bundled (`@fontsource-variable/inter`) | The CSP forbids remote fonts. |
| Package manager / scripts | **bun** | `bun install`, `bun run dev`, `bun run check`, `bun run test`, `bun run tauri build`. Lockfile: `bun.lock`. |
| Unit tests | **Vitest** | Pure logic: the create flow, name rules, the mock. `bun run test`. |
| UI tests (navigation, flows) | **Playwright** (Chromium) against its own `bun run dev --port 4173` server on the TS mock (so it never collides with `tauri dev` on 1420) | Clicks and types through every flow and error state. `bun run test:e2e`. The Rust shell is covered by `cargo test`. |
| Rust | Workspace member `apps/desktop/src-tauri` | Toolchain pinned by `rust-toolchain.toml`. `cargo test -p agentic-org`. |
| Talking to an office | **UI → Tauri commands → Rust → `org-client` → Unix socket** | The WebView never touches the socket and has no network access (`connect-src` is IPC only). A WebView can't open Unix sockets, and a localhost TCP port would need its own auth and origin checks. Rust owns the connection, the login-service install and reconnects. A Tauri call costs well under a millisecond, so there is no speed reason to go direct. |
| Contract | `org-proto` types; TypeScript mirrored by hand in `src/lib/proto.ts` | Generated TS later. |
| Mocking | Two seams, same types: TS mocks (`src/lib/office/mock.ts`, `src/lib/org/api.ts`, browser) and Rust mocks (`src-tauri/src/mock.rs`, `org_engine.rs`, in the app) | `?mock=<scenario>` in the browser, `AGENTIC_ORG_MOCK=<scenario>` in the app. Scenarios: `not_installed`, `needs_approval`, `unreachable`, `storage_error`. Org data: `?org=empty` (owner only), `?org=fails`, `?org=fails_once`, `?org=slow` in the browser; `AGENTIC_ORG_MOCK_ORG=empty` in the app. The mock office plays **HQ**: it applies the org commands (teams, members, memberships, people, agents, reporting lines, project links) with `org-proto`'s rules and error order, answers a retried `command_id` the same, and keeps each organization's structure in `structures.json` (versioned; unknown versions are never overwritten). The browser reducer (`src/lib/org/reducer.ts`) and the Rust engine are tested against the same cases (`crates/org-office/test-vectors/org-commands.json` at the workspace root). Both are labelled "Sample data". |
| Screens built | Org chart (Teams, People, Reporting lines; search; edit teams, members, people, agents, managers, projects), Offices (read-only, HQ, status verified by a read-only ping), Welcome / Create / Open organization, the menu-bar tray | Tests are named after the UX criteria in `offices-and-teams-ux.md` (TEAM-, PEOPLE-, REPORT-, OWNER-, OFC-, DEAL-, RUN-). Playwright specs in `e2e/`, screenshots to `test-results/screens` (ignored by git). |
| Security | Strict CSP, deny-by-default custom commands, minimal capabilities, no fs/shell plugins | Permissions listed in `src-tauri/capabilities/default.json`: window dragging, the office commands (`src-tauri/build.rs` lists them) and the event listener, nothing else (not even `core:default`). `style-src` keeps `'unsafe-inline'` on purpose: Svelte transitions and Bits UI set inline styles, and Tauri's nonce injection would otherwise disable it (`dangerousDisableAssetCspModification`). Scripts stay `'self'`. |

## Run it

```
cd apps/desktop
bun install
bunx playwright install chromium   # once, for bun run test:e2e
bun run dev                 # the UI in a browser on the mock, http://localhost:1420
bun run tauri dev           # the real app, on the real office (AGENTIC_ORG_OFFICE=mock for the invented one)
bun run check && bun run test && bun run test:e2e
cargo test -p agentic-org   # from the repo root
bun run tauri build --bundles app
```

## Not decided yet

1. **Testing the packaged app.** Tauri's official WebDriver (`tauri-driver`) does not support macOS (WKWebView has no WebDriver). Options: a small smoke test that launches the `.app`, or a community WebDriver plugin (not vetted). Flows are covered by Playwright on the mock.
2. **Lint and format for TS/Svelte.** Not set up. Proposal: Prettier (+ svelte plugin) and ESLint (typescript-eslint, eslint-plugin-svelte), run in CI. Rust already has `cargo fmt` and clippy.
3. **TypeScript types from `org-proto`.** By hand for now. Options: `specta`/`tauri-specta` (generates command and type bindings) or `ts-rs`. Needs ada's decision, since it touches the contract crate.
4. **Distribution.** Code signing, notarization, DMG and auto-update (`tauri-plugin-updater`). None needed until there is something to ship.
5. **Login-service registration.** `SMAppService` has to be called from the app bundle. Who installs the office server is undecided (ADR-0001 follow-up).
6. **Menu bar and Dock hiding.** Planned as a follow-up PR (tray menu listing offices, `ActivationPolicy::Accessory` toggle).
7. **Logging and crash reporting.** Nothing yet. Proposal: `tracing` in Rust, logs to the app's log directory, no remote reporting.
8. **CI.** The repo has no workflows yet. A macOS runner is needed for the Tauri build.
9. **Windows/Linux.** Out of scope. macOS only for now.
10. **Accessibility and i18n.** Keyboard and screen-reader behaviour follows Bits UI and Tailwind Plus defaults. English only, strings inline.
