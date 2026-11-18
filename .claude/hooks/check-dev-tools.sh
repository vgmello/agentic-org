#!/usr/bin/env bash
# Session-start check for the tools agents need in this repository.
# Prints nothing when everything is present; otherwise lists what is missing and
# how to fix it, so Claude can tell the user before starting work.
# CLAUDE.md lists the same tools. Keep the two in step.
set -u

root="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
ui="$root/apps/agentic-org-ui"
home="${HOME:-}"
cd "$root" 2>/dev/null || exit 0
missing=()

need() { # need <name> <reason> <fix>
	missing+=("- $1: $2. Fix: $3")
}

has() { command -v "$1" >/dev/null 2>&1; }

want_node="$(cat .node-version 2>/dev/null)"
want_node="${want_node:-from .node-version}"
if ! has node; then
	need node "runs the UI tooling and the Svelte MCP server" "install Node $want_node"
elif [ "$(node -v)" != "v$want_node" ]; then
	need node "found $(node -v), the repository pins $want_node (.node-version)" "install Node $want_node"
fi

want_bun="$(sed -n 's/.*"packageManager": *"bun@\([^"]*\)".*/\1/p' "$ui/package.json" 2>/dev/null)"
want_bun="${want_bun:-from packageManager in apps/agentic-org-ui/package.json}"
if ! has bun; then
	need bun "the UI package manager" "install Bun $want_bun"
elif [ "$(bun -v)" != "$want_bun" ]; then
	need bun "found $(bun -v), the repository pins $want_bun" "install Bun $want_bun"
fi

if [ ! -f "$ui/node_modules/@sveltejs/mcp/dist/index.mjs" ]; then
	need "UI dependencies" "the Svelte MCP server, type checks and tests run from apps/agentic-org-ui/node_modules" \
		"\`cd apps/agentic-org-ui && bun install --frozen-lockfile\`"
fi

# Playwright keeps its browsers in a per-user cache; the installed playwright-core
# names the revisions it needs.
case "${PLAYWRIGHT_BROWSERS_PATH:-}" in
0) pw_cache="$ui/node_modules/playwright-core/.local-browsers" ;;
"") if [ "$(uname)" = Darwin ]; then pw_cache="$home/Library/Caches/ms-playwright"
	else pw_cache="${XDG_CACHE_HOME:-$home/.cache}/ms-playwright"; fi ;;
/*) pw_cache="$PLAYWRIGHT_BROWSERS_PATH" ;;
*) pw_cache="$ui/$PLAYWRIGHT_BROWSERS_PATH" ;; # tests run from the UI folder
esac
pw_json="$ui/node_modules/playwright-core/browsers.json"
if [ -f "$pw_json" ] && has node; then
	# One line per browser listing every acceptable revision, since some
	# platforms (older macOS for webkit) use a revisionOverrides entry.
	while read -r browser revisions; do
		[ -n "$browser" ] || continue
		found=
		for rev in $revisions; do
			[ -d "$pw_cache/$browser-$rev" ] && found=1
		done
		if [ -z "$found" ]; then
			need "Playwright browsers" "the UI browser tests need chromium, webkit and firefox at the revisions the installed Playwright expects" \
				"\`cd apps/agentic-org-ui && bunx --no-install playwright install chromium webkit firefox\`"
			break
		fi
	done <<EOF_PW
$(node -e '
	const b = require(process.argv[1]).browsers;
	for (const n of ["chromium", "firefox", "webkit"]) {
		const e = b.find((x) => x.name === n);
		if (e) console.log(n, e.revision, ...Object.values(e.revisionOverrides || {}));
	}' "$pw_json" 2>/dev/null)
EOF_PW
fi

if ! has rustup; then
	need rustup "builds and checks the Rust workspace (toolchain from rust-toolchain.toml)" "install rustup from https://rustup.rs"
else
	# Never let rustup download a toolchain or component from this check.
	export RUSTUP_AUTO_INSTALL=0
	# ~/.cargo/bin/rust-analyzer is a rustup proxy, so ask rustup about the pinned toolchain.
	toolchain="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml 2>/dev/null)"
	if [ -n "$toolchain" ] && ! rustup toolchain list 2>/dev/null | grep -q "^$toolchain-"; then
		need "Rust $toolchain" "the toolchain pinned in rust-toolchain.toml" "\`rustup toolchain install $toolchain\` (from the repository root)"
	elif [ -n "$toolchain" ] && ! rustup component list --installed --toolchain "$toolchain" 2>/dev/null | grep -q '^rust-analyzer'; then
		need rust-analyzer "Rust code navigation for the LSP tool" "\`rustup component add rust-analyzer\` (from the repository root)"
	fi
fi

has typescript-language-server || need typescript-language-server "TypeScript code navigation for the LSP tool" \
	"\`npm i -g typescript-language-server@6.1.0 typescript@6.0.3\`"
if has typescript-language-server; then
	# TypeScript 7 no longer ships tsserver, which the language server needs.
	if ! has npm || [ ! -f "$(npm root -g 2>/dev/null)/typescript/lib/tsserver.js" ]; then
		need "global TypeScript" "typescript-language-server needs a TypeScript 6 with tsserver in npm's global packages" \
			"\`npm i -g typescript@6.0.3\`"
	fi
fi

has python3 || need python3 "runs the agent kit checks (.agent-kit/runtime/kit_hooks.py check)" "install Python 3"

# The agent kit's starter_kit_context MCP server (.mcp.json) serves Repowise lookup
# and Hindsight recall. Read this checkout's setup receipts and check the recorded
# memory daemon process; never probe or start services.
ready() { # ready <receipt> <status>
	grep -q "\"$2\"" "$1" 2>/dev/null && grep -qF "\"root\": \"$root\"" "$1" 2>/dev/null
}
ready .agent-kit/logs/context/setup.json CONTEXT_READY ||
	need "Repowise code index" "the starter_kit_context lookup tool has no index for this checkout" \
		"\`python3 .agent-kit/runtime/kit_context.py setup --query Client --expect-path crates/org-client/src/lib.rs\`"
memory=.agent-kit/logs/memory/hindsight/setup.json
pid="$(sed -n 's/.*"pid": *\([0-9][0-9]*\).*/\1/p' "$memory" 2>/dev/null)"
if ! has hindsight-embed; then
	need "Hindsight memory" "the starter_kit_context recall tool needs the local hindsight-embed daemon" \
		"install it as in the \"Local memory daemon\" section of .agent-kit/ADOPTION.md, then \`python3 .agent-kit/runtime/kit_memory.py setup\`"
elif ! ready "$memory" HINDSIGHT_READY || [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
	need "Hindsight memory" "the local hindsight-embed daemon is not set up or not running for this checkout" \
		"\`python3 .agent-kit/runtime/kit_memory.py setup\`"
fi
has cargo-deny || need cargo-deny "required local check (cargo deny --locked check)" "install cargo-deny 0.20.2 (see DEVELOPMENT.md)"
has gitleaks || need gitleaks "required local secret scan" "install Gitleaks 8.30.1 (see DEVELOPMENT.md)"
has playwright-cli || need playwright-cli "the playwright-cli browser skill" "\`bun add --global --exact @playwright/cli@0.1.24\`"

if [ ${#missing[@]} -gt 0 ]; then
	echo "Development tools missing in this checkout (Node, Bun, Playwright browsers and Rust are also checked against their pinned versions):"
	printf '%s\n' "${missing[@]}"
	echo "Before starting work, tell the user which tools are missing, why each matters and the fixes above (see CLAUDE.md, \"Tools for agents\"). Without them you work less efficiently or cannot run the required checks."
fi
exit 0
