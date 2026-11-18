# Development tooling

Use the Rust toolchain in `rust-toolchain.toml`, Node in `.node-version`, and Bun
1.4.4. CI uses the same versions. Install UI dependencies with
`bun install --frozen-lockfile` in `apps/agentic-org-ui`.

## Required local checks

From the repository root:

```sh
cargo fmt --check
CARGO_BUILD_JOBS=2 cargo clippy --locked --workspace --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --locked --workspace
cargo deny --locked check
gitleaks git --redact
```

CI runs the same workspace Clippy and tests on macOS and Linux, with no package
excluded (`.github/workflows/ci.yml`). On Linux the host starts office workers since
increment X2 of the [implementation plan](docs/implementation-plan.md), keeps a
release build's secrets in a checked directory since X3 and confines the web process
since X4; the systemd service and the container follow in X5. `agentic-org serve` and
`setup` refuse to run as root, so run the tests as an ordinary user. A development
host admits a worker executable that its group can write only when that group is the
user's own private group (the default of an Ubuntu account with a umask of `002`);
the integration tests copy the programs they start into a private folder.

Install `cargo-deny` 0.20.2 and Gitleaks 8.30.1, for example through Homebrew.
CI pins the cargo-deny action by commit and verifies the Gitleaks binary checksum.
`deny.toml` checks advisories, licenses, wildcard dependencies and dependency
sources for the supported macOS and Linux targets. Workspace path dependencies
are allowed; unknown registry and Git sources are rejected.

Gitleaks scans complete history. `.gitleaksignore` lists exact historical matches
reviewed as public test keys, test credentials, key hashes, message IDs or prose.
It does not exclude any directory or file from future scanning. Never suppress a
real credential: revoke it and arrange removal instead.

From `apps/agentic-org-ui`:

```sh
bun run lint
bun run format:check
bun run audit:check
bun run check
bun run test
bun run build
bunx --no-install playwright install chromium webkit firefox
bun run test:e2e
bun run test:e2e:csp
bun run test:e2e:csp-gateway
PW_WEBKIT=1 bun run test:e2e --project=webkit --grep @journey
```

Run checks and builds sequentially with browser suites: they share `.svelte-kit`.
Tests must use temporary organization directories, never a person's organization.
CI runs the full Chromium suite, the CSP suite in four shards, real-gateway CSP
checks in all three browsers, and WebKit journeys. Failed tests retain Playwright
traces, screenshots and CI report artifacts. Accessibility checks use axe for
automatically detectable WCAG A/AA issues; keyboard tests remain necessary.

`bun run audit` reports all advisory severities. `audit:check` fails at moderate
and above, including development dependencies. Four low-severity findings in the
existing SvelteKit/cookie/deepmerge dependency graph remain visible in the full
report. The scoped minimatch override patches GHSA-g3hv-75wg-m249 without replacing the
older minimatch 3 API used by other tools.

## Lint and formatting

ESLint runs the recommended JavaScript, TypeScript and Svelte rules. Its
`eslint-suppressions.json` records existing per-file/rule debt. Additional findings
fail, and obsolete suppressions must be pruned with `eslint --prune-suppressions`.
Do not use `--suppress-all` during normal development.

`format:check` checks every supported tracked or untracked UI file. Only files
whose contents exactly match a SHA-256 entry in `scripts/format-baseline.json` can
skip formatting.
New or edited files must pass Prettier. Format individual files with
`bunx --no-install prettier --write <files>`; `bun run format` formats the whole UI.
The Playwright configuration preserves its literal layout because existing gate
guards parse that layout. Changes to baselines require explicit review; do not
regenerate `scripts/format-baseline.json` to make a failing check pass.

## AI tools

Agents discover the committed skills in `.agents/skills`, among them shadcn-svelte
and Microsoft's Playwright CLI. `skills-lock.json` records their sources. The existing
Svelte plugin supplies Svelte skills and MCP tools; the Rust LSP MCP supplies
rust-analyzer navigation, types, references and rename edits. Follow the UI's
`AGENTS.md` for Svelte documentation and autofixer use.

The project pins `@sveltejs/mcp`; `bun run mcp:svelte` starts that installed version.
For a manually configured Codex server, match the project's version:

```sh
codex mcp add svelte -- npx -y @sveltejs/mcp@0.1.30
```

The browser skill uses a separate global CLI so its Playwright dependency cannot
change the application's test runner. Install its pinned version:

```sh
bun add --global --exact @playwright/cli@0.1.24
```

Ensure Bun's global bin directory is on `PATH`. Start the app in its test mode
(`vite dev --mode e2e`, as the Playwright configuration does) to exercise
fake-backed flows; normal development shows areas without a server adapter as
unavailable. Browser-agent snapshots and local
session state belong in ignored `.playwright-cli/` or `.playwright/` directories.
Newly installed skills are available to an agent from its next turn.

`cargo-nextest` is optional for local test selection and reporting. It does not
replace the repository's required `cargo test --workspace` gate.
