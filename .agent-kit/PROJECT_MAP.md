# Agentic Org map

Reviewed 2026-11-15 against integration base 73a39c58. Navigation guidance; source and owning contracts decide behavior.

- Start with AGENTS.md, docs/architecture-rules.md, docs/design/documentation-authority.md and docs/design/organization-data/README.md. Codex owns implementation across the repository and uses local kit subagents/reviews; integrate into v1-integration. No offline-owner messaging or approval gate applies.
- Current feature-checkout Cargo.toml defines 23 Rust workspace members, including org-pki and org-release; rust-toolchain.toml pins Rust 1.99.1. apps/agentic-org composes org-host; apps/agentic-org-office composes the worker; apps/agentic-org-web is the browser gateway; apps/agent-os is the employee CLI. This count describes the pending checkout, not an accepted integration.
- crates/org-office integrates office domain/storage. org-work and org-mailbox own policies/ports; org-client and org-socket support IPC. Shared contracts: org-proto, org-host-control (private host/browser/CLI control), org-sync-proto, org-office-launch and org-runtime.
- apps/agentic-org-ui is static SvelteKit 2/Svelte 5/TypeScript 6 with Bun, Tailwind 4 and shadcn-svelte. Read its scoped AGENTS.md and components.json. No Node application backend.
- .github/workflows/ci.yml defines Rust/macOS/Linux, frontend, dependency, secret and browser/CSP checks. .agent-kit/verification.json is a bounded fast gate, not the complete merge gate.
- docs/adr/ remains canonical. Selected data design differs from implemented migrations/commands; see docs/design/executable-migration.md.
- ADR-0009 selects the independently reviewed complete-release verification boundary. Its pure org-release library is imported from its isolated worktree with focused checks passing; host/web consume its verified inventory without a gateway-to-host dependency. Archive staging, durable activation and platform admission remain host integration work. Architecture readiness is not production-release evidence; the crate import is a component checkpoint and does not establish whole-feature or production acceptance.
- [Tooling capsule](maps/tooling.md), [active decisions](DECISIONS.md), [skill catalog](skill-catalog.json), [adoption checkpoint](tasks/adoption-state.md).

Reassess after interface, dependency, executable or architecture changes. Prepare indexes per worktree. Preserve the installed ~/.local/bin/org-agent and its Phase A spool; do not use them to coordinate this solo workflow or contact offline developers.
