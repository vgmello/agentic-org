# Contributing to Agentic Org

Thank you for your interest in contributing to Agentic Org!

## Prerequisites & Local Setup

Agentic Org pins its Rust, Node, and Bun toolchains so local checks match CI. See [DEVELOPMENT.md](DEVELOPMENT.md) for the exact versions and full verification commands.

Quick check before opening a pull request:

```sh
# Rust workspace
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo deny --locked check

# Web UI
cd apps/agentic-org-ui
bun install --frozen-lockfile
bun run lint
bun run format:check
bun run check
bun run test
bun run build
```

## Pull Request Guidelines

- Branch from `v1` (or `main` for release hotfixes, when applicable) and keep changes focused on a single logical scope.
- Include tests for new behavior and regression tests for bug fixes. Never write to a real user's organization directory from tests—always use temporary directories.
- Keep pull request descriptions concise using the repository PR template (**Why**, **What**, **Notes**, **Breaking changes**).
- Do not commit secrets, credentials, or personal local paths. Run `gitleaks git --redact` if you have Gitleaks installed.
- Note on UI components: `apps/agentic-org-ui` contains Tailwind Plus components licensed for use inside this application; do not extract them into standalone crates or packages.

## Licensing of Contributions

By submitting a contribution to this repository, you agree that your contribution is licensed under the repository's [LICENSE](LICENSE) (`FSL-1.1-ALv2`, converting to `Apache-2.0` two years after release).

## Code of Conduct

All contributors and community members are expected to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).
