@AGENTS.md

## Tools for agents

Start Claude Code at the repository root; it is the entry point for all work, including
the UI. These tools make agents efficient here. A session-start hook
(`.claude/hooks/check-dev-tools.sh`) reports any that are missing. When it does, tell
the user before starting work: name each missing tool, what it is for and the command
that fixes it. Do not silently work around a missing tool.

| Tool | What it gives agents | Set up with |
| --- | --- | --- |
| UI dependencies | Svelte MCP server, type checks, lint, tests | `cd apps/agentic-org-ui && bun install --frozen-lockfile` |
| Playwright browsers | UI browser tests | `cd apps/agentic-org-ui && bunx --no-install playwright install chromium webkit firefox` |
| Node (`.node-version`) and Bun (`packageManager` in the UI `package.json`) | UI toolchain | install the pinned versions |
| rustup and the pinned toolchain | Rust toolchain from `rust-toolchain.toml` | <https://rustup.rs>, then `rustup toolchain install` at the repository root |
| rust-analyzer | Rust navigation through the LSP tool | `rustup component add rust-analyzer` at the repository root |
| typescript-language-server with TypeScript 6 | TypeScript navigation through the LSP tool (TypeScript 7 has no `tsserver`) | `npm i -g typescript-language-server@6.1.0 typescript@6.0.3` |
| python3 | agent kit checks (`.agent-kit/runtime/kit_hooks.py check`) | install Python 3 |
| Repowise code index | `starter_kit_context` `lookup`: search, symbols, relationships and history | `python3 .agent-kit/runtime/kit_context.py setup --query Client --expect-path crates/org-client/src/lib.rs` |
| Hindsight memory (local `hindsight-embed` daemon) | `starter_kit_context` `recall`: reviewed project memory | install as in "Local memory daemon" in `.agent-kit/ADOPTION.md`, then `python3 .agent-kit/runtime/kit_memory.py setup` (also restarts a stopped daemon) |
| cargo-deny 0.20.2, Gitleaks 8.30.1 | required local checks | Homebrew on macOS; see `DEVELOPMENT.md` |
| playwright-cli | browser skill | `bun add --global --exact @playwright/cli@0.1.24` |

Repository configuration supplies the rest; approve it when Claude Code first asks:

- `.mcp.json`: the Svelte MCP server, run from the lockfile-pinned
  `apps/agentic-org-ui/node_modules`, never downloaded at run time; and the agent
  kit's `starter_kit_context` server (the launcher from `.agent-kit/context-mcp.json`).
  Use its `lookup` for code discovery before broad searches and its `recall` for
  project memory, as `.agent-kit/reference/CONTEXT_TOOLS.md` describes.
- `.claude/skills/svelte-*` and `.claude/agents/svelte-file-editor.md`: links to the UI
  app's Svelte 5 skills and editor agent. Follow `apps/agentic-org-ui/AGENTS.md` for
  Svelte documentation and autofixer use.
- `.claude/settings.json`: the `typescript-lsp` and `rust-analyzer-lsp` plugins and the
  tool check.

`DEVELOPMENT.md` has the full list of checks and versions.
