# Agentic Org desktop: tech stack

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
| Mocking | Two seams, same types: TS `OfficeApi` mock (`src/lib/office/mock.ts`, browser) and Rust `OfficeConnection` mock (`src-tauri/src/mock.rs`, in the app) | `?mock=<scenario>` in the browser, `AGENTIC_ORG_MOCK=<scenario>` in the app. Scenarios: `not_installed`, `needs_approval`, `unreachable`, `storage_error`. |
| Security | Strict CSP, deny-by-default custom commands, minimal capabilities, no fs/shell plugins | Permissions listed in `src-tauri/capabilities/default.json`: window dragging and the three office commands, nothing else (not even `core:default`). `style-src` keeps `'unsafe-inline'` on purpose: Svelte transitions and Bits UI set inline styles, and Tauri's nonce injection would otherwise disable it (`dangerousDisableAssetCspModification`). Scripts stay `'self'`. |

## Run it

```
cd apps/desktop
bun install
bunx playwright install chromium   # once, for bun run test:e2e
bun run dev                 # the UI in a browser on the mock, http://localhost:1420
bun run tauri dev           # the real app (Rust mock until org-client exists)
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
