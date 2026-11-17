# Tooling and verification capsule

Reviewed at b1256c97 on 2026-11-15.

The target is the whole monorepo. WORKFLOW.md adds task/review gates. Root AGENTS.md retains team rules and the managed pointer; scoped UI guidance still applies. .codex/config.toml and five native roles select Sol and a two-child limit. Roles inherit host permissions; the review wrapper explicitly launches read-only review processes.

Fast gate: python3 .agent-kit/runtime/kit_hooks.py check. Full required Rust format/Clippy/tests remain merge gates; frontend work also needs its checks/tests/build and relevant Playwright/CSP/WebKit journeys from CI. Longer browser suites are separate from the 300-second fast gate.

Repowise uses isolated Python 3.12 and checkout-local caches. Its structural retrieval is not typed LSP parity. Existing native Rust/Svelte tooling is preserved. Rust semantic work uses the existing rust-lsp skill; Svelte changes require official Svelte MCP documentation/autofixer and maintained UI skills.

Hindsight uses Docker/Linux containers, embedded pg0, a persistent volume and local CPU models. Committed accepted Git knowledge is authoritative. Preserve unrelated containers/volumes and Podman. Private receipts/logs are ignored and checkout-bound. Saved Codex login does not prove allowance; native hooks/MCP need actual client trust/restart verification.

See reference/ADOPT.md for setup, ADOPTION.md for results and tasks/v1-state.md for current work. Reassess stack/runtime/ownership changes; never copy successful receipts between checkouts.
