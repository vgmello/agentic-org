# Agentic Org map

Reviewed 2026-11-15 against base b1256c97. Navigation guidance; source and owning contracts decide behavior.

- Start with AGENTS.md, docs/architecture-rules.md, docs/design/documentation-authority.md and docs/design/organization-data/README.md. Ownership and release gates survive adoption.
- Cargo.toml defines 21 Rust workspace members; rust-toolchain.toml pins Rust 1.99.1. apps/agentic-org composes org-host; apps/agentic-org-office composes the worker; apps/agentic-org-web is the browser gateway; apps/agent-os is the employee CLI.
- crates/org-office integrates office domain/storage. org-work and org-mailbox own policies/ports; org-client and org-socket support IPC. Public contracts: org-proto, org-sync-proto, org-office-launch, org-runtime.
- apps/agentic-org-ui is static SvelteKit 2/Svelte 5/TypeScript 6 with Bun, Tailwind 4 and shadcn-svelte. Read its scoped AGENTS.md and components.json. No Node application backend.
- .github/workflows/ci.yml defines Rust/macOS/Linux, frontend, dependency, secret and browser/CSP checks. .agent-kit/verification.json is a bounded fast gate, not the complete merge gate.
- docs/adr/ remains canonical. Selected data design differs from implemented migrations/commands; see docs/design/executable-migration.md.
- [Tooling capsule](maps/tooling.md), [active decisions](DECISIONS.md), [skill catalog](skill-catalog.json), [adoption checkpoint](tasks/adoption-state.md).

Reassess after ownership, interface, dependency, executable or architecture changes. Prepare indexes per worktree. Installed ~/.local/bin/org-agent and its Phase A spool remain live coordination tools.
