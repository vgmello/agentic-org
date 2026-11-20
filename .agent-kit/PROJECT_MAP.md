# Agentic Org map

Navigation hints, checked against the source on 2026-11-17. Source and the owning
documents decide behavior; recheck before relying on a detail.

- Start with [AGENTS.md](../AGENTS.md), [START-HERE](../docs/START-HERE.md), the
  [implementation plan](../docs/implementation-plan.md),
  [architecture rules](../docs/architecture-rules.md) and the
  [documentation authority map](../docs/design/documentation-authority.md).
- `Cargo.toml` has 24 workspace members; `rust-toolchain.toml` pins Rust 1.99.1.
  `apps/agentic-org` composes `crates/org-host` (`serve`); `apps/agentic-org-office`
  is the office worker; `apps/agentic-org-web` serves the browser application and
  `/api/v1`; `apps/agent-os` is the agents' command line.
- `crates/org-office` holds the office domain and storage. `org-work` and `org-mailbox`
  own their policies and ports; `org-process` the process rules; `org-docs` parses
  Markdown documents with front matter; `org-copilot` the copilot's rules; `org-client` and
  `org-socket` support local IPC; `org-supervise` process supervision; `org-sync` and
  `org-sync-proto` replication between offices; `org-release` release verification;
  `org-pki` recovery escrow; `org-agent-runtime-claude` the sans-I/O Claude Code
  adapter. Shared contracts: `org-proto`, `org-office-launch`, `org-agent-runtime`,
  `org-host-control`.
- `crates/org-spool` is the file queue `agent-os` still uses; increment B6 deletes it.
- `apps/agentic-org-ui` is static SvelteKit 2, Svelte 5, TypeScript 6, Tailwind 4 and
  shadcn-svelte, with Bun. Read its `AGENTS.md` and `components.json`. No Node backend.
- `.github/workflows/ci.yml` runs Rust on macOS and Linux, the UI, dependency, secret
  and browser/CSP checks. `.agent-kit/verification.json` is a fast gate, not the full
  merge gate.
- What each executable does today: [executable migration](../docs/design/executable-migration.md).
- [Tooling capsule](maps/tooling.md), [active decisions](DECISIONS.md),
  [skill catalog](skill-catalog.json), [current task](tasks/v1-state.md).

Recheck after an interface, dependency, executable or architecture change. Prepare
indexes per worktree.
