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
increment X2 of the [implementation plan](.backlog/docs/doc-5%20-%20v1-Implementation-Plan.md), keeps a
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

Agents discover the committed skills in `.agents/skills`, among them shadcn-svelte,
Microsoft's Playwright CLI and the Svelte skills `svelte-code-writer` and
`svelte-core-bestpractices`. `skills-lock.json` records their sources. `AGENTS.md`
("Tools for agents") lists every tool agents need; they are required, and work does
not start until `.agents/check-tools.sh` passes. Follow the UI's
`AGENTS.md` for Svelte documentation and autofixer use.

Both harnesses get the same tools from committed configuration:

| Tool | Claude Code | Codex |
| --- | --- | --- |
| Instructions | `CLAUDE.md` imports `AGENTS.md` | `AGENTS.md` |
| Required-tools check (`.agents/check-tools.sh`) | session start and Edit/Write hooks in `.claude/settings.json` | run with `--codex` before work |
| Repowise lookup (`starter_kit_context`) | `.mcp.json` | `.codex/config.toml` |
| Svelte MCP, pinned `@sveltejs/mcp` | `.mcp.json` | `.codex/config.toml` |
| Skills (`.agents/skills`) | committed links in `.claude/skills` | `.agents/skills` |
| Rust, TypeScript and Svelte language servers | LSP plugins in `.claude/settings.json` (Svelte: the repository's `svelte-lsp` plugin in `.claude/marketplace`) | `mcp-language-server` entries in `.codex/config.toml` |
| Kit hooks (secret-file guard, format, verify, checkpoint) | `.claude/settings.json` | `.codex/hooks.json` |
| Roles | `.claude/agents` | `.codex/agents` |

Trust the project when Claude Code asks, so it installs the plugins and starts the
servers. Install the binaries both harnesses run:

```sh
rustup component add rust-analyzer rust-src
bun add --global --exact typescript@6.0.3 typescript-language-server@6.1.0 svelte-language-server@0.18.6
go install github.com/isaacphi/mcp-language-server@v0.1.1
python3 .agent-kit/runtime/kit_context.py setup --query is_uuid_v7 --expect-path crates/org-proto/src/identity.rs
```

Codex needs `mcp-language-server`; Claude Code does not. Run `bun install` in
`apps/agentic-org-ui` before the Svelte MCP starts. `bun run mcp:svelte` starts the
same pinned server by hand.

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

### The principal designer agent

For a new or changed screen, component or flow in `apps/agentic-org-ui`, and for UX
reviews, a lead may dispatch the `principal-designer` agent when it is useful. It is defined
for both tools:
`.claude/agents/principal-designer.md` for Claude Code and
`.codex/agents/principal-designer.toml` for Codex. Both are thin: they point at one persona,
`.agent-kit/project-skills/principal-designer/`, so the knowledge is maintained in one
place. The assignment names the job: design (propose), implement (build) or review
(read-only). The agent sorts what a screen shows by what the data is worth to the person,
weighs Fitts's, Hick's and Jakob's laws, proximity and the isolation effect while building,
and runs a measured consistency audit before handing over. Product specs outrank it. The
lead still owns the increment, file ownership and the review gates in `AGENTS.md`.

The persona's third-party UI/UX Pro Max companion skill is disabled (owner, 2026-11-18): agents
do not check for, install or use it. Its former instructions stay commented out in the persona
file for reference.
