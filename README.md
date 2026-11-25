# Agentic Org

Agentic Org is a self-hosted server that runs an organization of people and AI agents—teams, projects, tasks, reviews, chat, and spend—on macOS and Linux (including containers). People interact through a web browser; agents use the `agent-os` CLI and run headless, started and supervised by their office worker.

## Architecture & Repository Layout

```text
apps/
  agentic-org/                 Host CLI (`agentic-org serve`, `setup`, instance & org management)
  agentic-org-office/          Office worker daemon (runs agent turns, store, mailbox, and sync)
  agentic-org-web/             Browser BFF & HTTP/HTTPS server serving the UI and `/api/v1`
  agent-os/                    Agent-facing CLI used inside supervised agent turns
  agentic-org-ui/              Static browser application (SvelteKit 2, Svelte 5), served by `agentic-org-web`
  agentic-org-web-prototype/   Standalone static HTML/CSS UI prototypes (never bundled into production)
crates/
  org-host/                    Host library composed by `agentic-org serve`
  org-office/                  Core office domain, projections, and durable SQLite/file store
  org-agent-runtime*/          Pluggable agent runtimes (Claude, Codex, Agy)
  org-proto/, org-pki/, ...    Shared protocol, PKI, supervision, sync, and platform libraries
config/
  csp/                         Shared Content-Security-Policy header templates for BFF & UI tests
deploy/
  container/                   Container image and deployment definitions
docs/
  architecture/                System and architecture HTML guides
  design/organization-data/    Canonical SQL schema and executable schema verification tests
```

## Build & Test

See [DEVELOPMENT.md](DEVELOPMENT.md) for pinned toolchains (`rust-toolchain.toml`, `.node-version`, Bun `1.4.4`), security scanners (`cargo-deny`, `gitleaks`), and browser test suites.

### Rust workspace

```sh
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo deny --locked check
```

### Web UI (`apps/agentic-org-ui`)

```sh
cd apps/agentic-org-ui
bun install --frozen-lockfile
bun run lint
bun run format:check
bun run check
bun run test
bun run build
```

## Contributing & Security

- Please read [CONTRIBUTING.md](CONTRIBUTING.md) and our [Code of Conduct](CODE_OF_CONDUCT.md) before opening a pull request.
- To report a security vulnerability—especially involving authentication, PKI, agent sandboxing, or the web BFF/CSP boundary—please follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

## License

Agentic Org is licensed under the **Functional Source License, Version 1.1, Apache 2.0 Future License (`FSL-1.1-ALv2`)**—see [LICENSE](LICENSE) for the full terms.

### What this license means in plain English

- **Free for personal and commercial internal use:** Anyone—including for-profit companies of any size—can self-host, modify, and run Agentic Org for free inside their own organization (including using it internally to build and ship their own unrelated commercial products).
- **Free for professional services & education:** Agencies and consultants may provide paid setup, customization, or support services to organizations running Agentic Org internally, and anyone may use it for non-commercial education or research.
- **No competing SaaS or reselling:** You may **not** make Agentic Org available to third parties as a competing commercial product or hosted/managed service (SaaS).
- **Automatic conversion to Apache-2.0 after 2 years:** Every release irrevocably converts to the permissive **Apache License 2.0** two years after it is made available.

### Why we chose `FSL-1.1-ALv2`

We want Agentic Org to be openly inspectable, easy to self-host without paywalls or license keys, and safe for teams to adopt long-term without vendor lock-in—while ensuring third-party cloud providers cannot simply take our work and sell it as a competing hosted SaaS. `FSL-1.1-ALv2` keeps self-hosting and internal business use completely free while protecting the sustainability of the project, and guarantees that every version becomes standard Apache-2.0 open source after two years.

### Third-party notices & UI carve-out

See [NOTICE](NOTICE) for third-party attributions and scope exclusions. While `DescriptionList.svelte` and `StatTile.svelte` have been rewritten cleanly, a full file-level audit of older screens in `apps/agentic-org-ui/` is still tracked under `TASK-122`; until complete, any remaining Tailwind Plus-derived UI patterns stay subject to Tailwind Plus license terms and are excluded from the `FSL-1.1-ALv2` and future `Apache-2.0` grants.
