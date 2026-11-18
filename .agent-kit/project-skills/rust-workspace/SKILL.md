---
name: rust-workspace
description: Implement or review Agentic Org Rust changes with pinned checks, crate ownership and library composition boundaries.
---

Cargo.toml, rust-toolchain.toml and CI pin Rust 1.99.1, edition 2021 and exact shared dependencies. Read the affected crate's manifest/public API before adding edges. Apps compose libraries; policy contracts stay with their owner. Review public protocol/runtime consumers and update affected trait implementations together. Replace rather than adapt: no migration, compatibility shim, retained protocol version or old name.

For semantic Rust navigation use the existing rust-lsp skill/tool. Report missing access and use compiler/source evidence without claiming typed parity. Repowise is structural discovery; do not install another index.

At root run cargo fmt --check, cargo clippy --locked --workspace --all-targets -- -D warnings and cargo test --locked --workspace. Targeted tests aid iteration, not final merge acceptance. CI runs the same Clippy and tests on macOS and Linux with no package excluded; the host refuses to run as root, so tests run as an ordinary user. Tests use temporary roots. Every operating-system difference lives in crates/org-platform, behind a contract with macOS, Linux and refusing adapters (ADR-0017, docs/design/linux-host.md): no other file gains a cfg, cfg_attr or cfg! on target_os, target_vendor, target_env or target_family, crates/org-platform/tests/platform_conditionals.rs fails on one, and .github/platform-conditionals.allow only shrinks (lower a file's count when it loses predicates). Inspect cancellation, cleanup, errors and idempotency where affected.

Review dependency direction at consistent crate/domain level, downstream callers and failure tests. Agree a contract, credential or durability change that another increment depends on with the other lead before dependent work. Official references on demand: https://doc.rust-lang.org/1.99.1/book/ and https://doc.rust-lang.org/cargo/commands/cargo-test.html . Pinned toolchain/CI remain execution evidence.
