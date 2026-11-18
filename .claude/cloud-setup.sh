#!/bin/bash
# Paste into the Claude Code cloud environment's Setup script (run from the checkouts' parent).
# Agentic Org cloud setup: Rust + Svelte 5 toolchains, language servers, CI tools.
# Each step warns instead of aborting, so one failed download never blocks a session.
set -uo pipefail
export PATH="/root/.local/bin:/root/.cargo/bin:/root/.bun/bin:/usr/local/go/bin:$PATH"
BIN=/root/.local/bin; mkdir -p "$BIN"
warn() { echo "SETUP WARNING: $*" >&2; }

# Node 26.13.0 (.node-version); the image ships 22, so link 26 ahead of /opt/node22.
if ! node --version 2>/dev/null | grep -q '^v26.13.0$'; then
  ( curl -fsSL https://nodejs.org/dist/v26.13.0/node-v26.13.0-linux-x64.tar.xz -o /tmp/node.txz &&
    echo "d28a96af6c38596f6d9d97c0ee531c13d238d757742387d1928de822d959e997  /tmp/node.txz" | sha256sum -c - &&
    rm -rf /opt/node26 && mkdir -p /opt/node26 &&
    tar -xJf /tmp/node.txz -C /opt/node26 --strip-components=1 && rm -f /tmp/node.txz &&
    for b in node npm npx; do ln -sf /opt/node26/bin/$b "$BIN/$b"; done ) || warn "node 26"
fi

# Bun 1.4.4 (package.json packageManager).
bun --version 2>/dev/null | grep -q '^1.4.4$' ||
  (curl -fsSL https://bun.sh/install | bash -s bun-v1.4.4) || warn "bun"

# Language servers used by the Claude Code LSP plugins, plus the browser-skill CLI.
bun add --global --exact typescript@6.0.3 typescript-language-server \
  svelte-language-server @playwright/cli@0.1.24 || warn "global JS tools"

# mcp-language-server: Codex's bridge to the same language servers (.codex/config.toml).
command -v mcp-language-server >/dev/null ||
  GOBIN="$BIN" go install github.com/isaacphi/mcp-language-server@v0.1.1 || warn "mcp-language-server"

# cargo-deny 0.20.2 and gitleaks 8.30.1 (the CI versions).
if ! cargo-deny --version 2>/dev/null | grep -q 0.20.2; then
  d=cargo-deny-0.20.2-x86_64-unknown-linux-musl
  ( curl -fsSL "https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/$d.tar.gz" |
      tar -xz -C /tmp && install -m755 "/tmp/$d/cargo-deny" "$BIN/cargo-deny" ) || warn "cargo-deny"
fi
if ! gitleaks version 2>/dev/null | grep -q 8.30.1; then
  ( curl -fsSL https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_linux_x64.tar.gz -o /tmp/gl.tgz &&
    echo "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb  /tmp/gl.tgz" | sha256sum -c - &&
    tar -xzf /tmp/gl.tgz -C "$BIN" gitleaks ) || warn "gitleaks"
fi

if [ -d agentic-org ]; then
  cd agentic-org
  # Expose the committed skills to Claude Code (.claude/skills is generated and ignored).
  mkdir -p .claude/skills
  # The svelte plugin already supplies the svelte-* skills to Claude.
  for d in .agents/skills/*/; do
    n=$(basename "$d"); case "$n" in svelte-*) continue ;; esac
    ln -sfn "../../.agents/skills/$n" ".claude/skills/$n"
  done
  # Repowise lookup behind the starter_kit_context MCP (ignored .agent-kit/tools).
  python3 .agent-kit/runtime/kit_context.py setup --query is_uuid_v7 \
    --expect-path crates/org-proto/src/identity.rs >/dev/null || warn "repowise lookup"
  # Rust toolchain from rust-toolchain.toml, rust-analyzer, and the musl target CI uses.
  rustup toolchain install --no-self-update || warn "rust toolchain"
  rustup component add rust-analyzer rust-src || warn "rust-analyzer"
  rustup target add x86_64-unknown-linux-musl || warn "musl target"
  cargo fetch --locked || warn "cargo fetch"
  # UI dependencies and the browsers its pinned @playwright/test expects.
  cd apps/agentic-org-ui
  bun install --frozen-lockfile || warn "bun install"
  PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD= bunx --no-install playwright install --with-deps \
    chromium webkit firefox || warn "playwright browsers"
fi
exit 0
