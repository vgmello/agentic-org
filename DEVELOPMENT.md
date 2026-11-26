# Development tooling

Use the Rust toolchain in `rust-toolchain.toml`, Node in `.node-version`, and Bun
1.4.4. CI uses the same versions. Install UI dependencies with
`bun install --frozen-lockfile` in `apps/agentic-org-ui`.

## Required local checks

From the repository root:

```sh
cargo fmt --check
CARGO_BUILD_JOBS=2 cargo clippy --locked --workspace --lib --bins --message-format=short -- -D clippy::too_many_lines
CARGO_BUILD_JOBS=2 cargo clippy --locked --workspace --all-targets --keep-going --message-format=short -- -D warnings
CARGO_BUILD_JOBS=2 cargo clippy --locked --workspace --release --lib --bins --message-format=short -- -D clippy::too_many_lines
CARGO_BUILD_JOBS=2 cargo clippy --locked --workspace --release --lib --bins --keep-going --message-format=short -- \
  -D clippy::undocumented_unsafe_blocks -D clippy::allow_attributes_without_reason
CARGO_BUILD_JOBS=2 cargo test --locked --workspace
cargo deny --locked check
gitleaks git --redact
```

CI runs the same workspace Clippy and tests on macOS and Linux, with no package
excluded (`.github/workflows/ci.yml`). On Linux the host starts office workers since
increment X2 of the implementation plan (`backlog/docs/doc-5 - v1-Implementation-Plan.md` in parent `agentic-org-project`), keeps a
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

## Cargo target directories and disk use

Several agents often build side by side, each in its own worktree, and every worktree
has its own `target/`. Keep each one bounded:

- **Line tables only.** The root `Cargo.toml` sets `[profile.dev] debug = "line-tables-only"`,
  and the `test` profile inherits it. Panics and backtraces still show every function,
  file and line, and `target/debug` is about 70% smaller than with full debug info. Build
  with `CARGO_PROFILE_DEV_DEBUG=full` when you need a debugger session.
- **Constrained machines.** On a machine with 4 cores, about 8 GiB of memory and no swap,
  build with `CARGO_BUILD_JOBS=2`, as the checks above do. For a one-off check or test run
  in a short-lived worktree, also set `CARGO_INCREMENTAL=0`. Incremental caches are often
  the biggest part of a debug `target/`, and they only pay off when you edit and rebuild
  in the same worktree.
- **One target per worktree.** Concurrent worktrees should not share one mutable
  `CARGO_TARGET_DIR`. Cargo holds a build lock, so parallel builds queue behind each other.
  Builds with different flags (`RUSTFLAGS`, features, `CARGO_PROFILE_*`) replace each
  other's artifacts and rebuild every time. A shared target also matches artifacts by
  workspace-relative paths, so building another checkout of an older commit can silently
  replace the artifacts of crates you changed, and your tests then run old code. If you
  ever shared a target, run `cargo clean -p <crate>` for each crate you changed before you
  trust a result.
- **Worktree hygiene.** `git worktree remove` deletes the ignored `target/` along with the
  worktree. Office-managed task worktrees are torn down the same way. A worktree you keep
  but no longer build in still holds its `target/`. Remove that with `cargo clean`, or let
  the pruning script below find it.

`scripts/prune-cargo-targets.sh` removes Cargo target directories that have not been used
for a number of days:

```sh
scripts/prune-cargo-targets.sh --dry-run --root ~/projects   # what would go, and its size
scripts/prune-cargo-targets.sh --root ~/projects             # remove targets idle 7+ days
scripts/prune-cargo-targets.sh --max-age-days 0 --root ~/projects   # every idle target
```

It considers only directories that hold Cargo's `CACHEDIR.TAG` and lie strictly inside a
`--root`. It follows no symlinks, skips `.git` and `node_modules`, and finds target
directories up to `--max-depth` levels below a root (default 12, deep enough for
`~/projects` and the office's runtime worktree roots). A target is idle when nothing
anywhere in it changed within `--max-age-days`; with `0` its age is not checked at all. It
never removes a target that is in use: one with a held Cargo lock at any depth (a build,
check, test or run), one whose workspace has a running `cargo`, `rustc`, `rustdoc`, Clippy
or `rust-analyzer` process, or one a running process names on its command line (such as a
test binary). To remove a target it takes every Cargo lock in it, renames it aside within
its parent (`.cargo-prune-*`) while holding them, and only then deletes the renamed tree, so
a build that starts meanwhile builds from scratch and the deletion never touches the new
`target/`. In the rare case that a build creates a new lock in the moment before that
rename, the script puts the directory back. If a new `target/` already exists by then, it
leaves the old one at its `.cargo-prune-*` name for a later run instead. It prints the space
reclaimed, and a second run removes nothing more. It needs `python3`.
`scripts/test-prune-cargo-targets.sh` runs its self-test on throwaway fixtures.

## Lint and formatting

Clippy warns on `dbg!()`, `todo!()`, and `unimplemented!()` in the targets it
checks, including test targets. The local and CI Clippy gates use `-D warnings`
to make these findings errors. Use assertions in tests and explicit error paths
for unsupported production behavior. New Rust workspace members must set
`[lints]` with `workspace = true` in their manifests.

The production Clippy command checks the 200-line limit for library and binary
functions. Test-only code can keep longer scenarios and fixtures. Keep functions
focused and extract coherent steps as their responsibilities grow. The release
Clippy commands check code that builds with debug assertions disabled. They check
length separately so missing safety documentation or reasons does not hide
production function length findings.

The all-targets gate requires safety documentation and a reason on lint
suppressions. Use safe code where practical, reuse existing safe workspace
helpers (`std::process::id()`, `org_socket::effective_uid()`,
`org_socket::rights::set_buffer_sizes`, `org_proto::zero_bytes`), and lock
unsafe-free crates with `#![forbid(unsafe_code)]`. When unsafe is necessary,
add a `// SAFETY:` comment directly on the `unsafe` block explaining why its
preconditions hold. Use a narrowly scoped `#[expect(lint_name, reason = "...")]`
for unconditional exceptions (or `allow(..., reason = "...")` when a lint fires
only on a subset of targets/profiles), and remove expectations when they no
longer apply. `crates/org-platform/tests/rust_hygiene.rs` (run in workspace tests)
enforces cross-platform `allow` reasons and `write_volatile` confinement via
`syn`/`proc_macro2`. When editing Linux platform
adapters on macOS, also run
`cargo clippy --locked -p org-platform -p org-platform-host --all-targets --all-features --target x86_64-unknown-linux-gnu -- -D warnings`.
The [unsafe documentation pilot inventory](docs/unsafe-documentation-pilot.md)
records the starting findings for cleanup.

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
| Instructions | `AGENTS.md` (with `.claude -> .agents` bootstrap) | `AGENTS.md` |
| Required-tools check (`.agents/check-tools.sh`) | session start and Edit/Write hooks in `.agents/settings.json` | run with `--codex` before work |
| Svelte MCP, pinned `@sveltejs/mcp` | `.mcp.json` | `.codex/config.toml` |
| Skills (`.agents/skills`) | via `.claude -> .agents` symlink | `.agents/skills` |
| Rust, TypeScript and Svelte language servers | LSP plugins in `.agents/settings.json` (Svelte: the repository's `svelte-lsp` plugin in `.agents/marketplace`) | `mcp-language-server` entries in `.codex/config.toml` |

Trust the project when Claude Code asks, so it installs the plugins and starts the
servers. Install the binaries both harnesses run:

```sh
rustup component add rust-analyzer rust-src
bun add --global --exact typescript@6.0.3 typescript-language-server@6.1.0 svelte-language-server@0.18.6
go install github.com/isaacphi/mcp-language-server@v0.1.1
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
