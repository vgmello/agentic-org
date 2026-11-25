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

## Licensing of Contributions (Contributor License Grant)

By submitting a pull request or other contribution to this repository, you confirm that:

1. You have the right to submit the contribution (either as original work or under compatible third-party terms).
2. You grant the project copyright holder (`Vitor M`) and its successors and assigns (including any future corporate entity operating Agentic Org) a perpetual, worldwide, non-exclusive, royalty-free, irrevocable copyright and patent license to use, modify, redistribute, sublicense, and offer commercial or hosted versions of your contribution—in addition to distributing it under the repository's [LICENSE](LICENSE) (`FSL-1.1-ALv2`, converting to `Apache-2.0` two years after release).

You retain ownership of the copyright in your own contributions.

## Code of Conduct

All contributors and community members are expected to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).
