---
name: rust-workspace
description: Implement or review Agentic Org Rust changes with pinned checks, crate ownership and library composition boundaries.
---

Cargo.toml, rust-toolchain.toml and CI pin Rust 1.99.1, edition 2021 and exact shared dependencies. Read the affected crate's manifest/public API before adding edges. Apps compose libraries; policy contracts stay with their owner. AGENTS.md owns paths; trait changes require implementer agreement; public protocol/runtime contracts go through ada.

For semantic Rust navigation use the existing rust-lsp skill/tool. Report missing access and use compiler/source evidence without claiming typed parity. Repowise is structural discovery; do not install another index.

At root run cargo fmt --check, cargo clippy --locked --workspace --all-targets -- -D warnings and cargo test --locked --workspace. Targeted tests aid iteration, not final merge acceptance. Linux CI has explicit host exclusions/allowances; do not apply them to macOS. Tests use temporary roots. Inspect cancellation, cleanup, errors, idempotency and versioned codecs where affected.

Review dependency direction at consistent crate/domain level, downstream callers and failure tests. Escalate contract/ownership or credential/durability changes. Official references on demand: https://doc.rust-lang.org/1.99.1/book/ and https://doc.rust-lang.org/cargo/commands/cargo-test.html . Pinned toolchain/CI remain execution evidence.
