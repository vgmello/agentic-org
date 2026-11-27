---
trigger: glob
globs: "**/*.rs, **/*.ts, **/*.svelte, **/Cargo.toml"
description: Agentic Org v1 workspace architecture, VSA/subdomain folder layout, Two-CLI contract, and mechanical coupling boundaries
paths:
  - "**/*.rs"
  - "**/*.ts"
  - "**/*.svelte"
  - "**/Cargo.toml"
---

# Architecture and Folder-Layout Rules (Operational Truth)

This committed rule is the operational architecture projection of `doc-7`, `doc-8`, and `doc-68` (in parent `agentic-org-project/backlog/docs/`). Because EU build VMs, standalone clones, and managed Agentic Org agents (`--setting-sources ""`) do not mount parent `backlog/` or user-local plugins, this file travels with the code repository and governs every change alongside `.agents/rules/rust-security-and-modularity.md`.

## 1. Four Thin Binaries & Workspace Layering (Non-Negotiable #7 & #15)

- **`apps/agentic-org`** — Human/operator CLI + `serve` composition of `crates/org-host`.
- **`apps/agentic-org-office`** — Thin office worker binary (<=100 LOC `main.rs`) composing `crates/org-office-server`.
- **`apps/agentic-org-web`** — Browser HTTP/SSE BFF and static asset server (`tests/dependency_closure.rs` enforces zero `org-host`, `org-office`, `org-client`, or `rusqlite` in its dependency closure).
- **`apps/agent-os`** — Thin agent CLI binary (<=50 LOC `main.rs`) composing `crates/org-agent-cli` + `crates/org-command`.
- **No runnable app depends on another app's library.** Reusable logic belongs in `crates/org-*`.
- **`apps/agentic-org-web-prototype`** is a standalone design sandbox; never import it from production code.

## 2. Explicit Two-CLI Contract (Do NOT Mix Them)

The workspace intentionally runs **two distinct CLI contracts**:

1. **Human / Operator CLI (`apps/agentic-org` + `crates/org-cli-shell`):**
   - Follows **1:1 VSA** (`apps/agentic-org/src/features/<noun>/<verb>.rs`; group into two-tier `features/<domain>/<noun>/<verb>.rs` when scaling past 15 nouns).
   - Uses `clap` + dynamic shell completion, tri-modal execution (TTY prompts / flags / `--json` + `-q`), `thiserror`/`CliDiagnostic` (no `miette`), and `org-platform::dirs` (no `etcetera`).
   - `features::*` imports ONLY `crate::*` root shell/host helpers and `org-cli-shell` — **0 cross-feature imports, 0 cycles**.
2. **Agent CLI (`apps/agent-os` + `crates/org-agent-cli` + `crates/org-command`):**
   - Organized internally as **Two-Tier Subdomain-Grouped VSA**:
     - `runtime/` — leaf headless kernel (`Ce = 0` internal; NEVER imports `features::*`).
     - `features/<domain>/` — cohesive bounded-context slices (`Ce = 1 -> runtime`; **0 cross-feature imports, 0 cycles**), co-locating each command's `Command` row, field schema, `Use when:` guidance, dispatch handler, and unit tests.
     - `registry.rs` — single composition point assembling `CommandTable`, aliases, guidance, and dispatch.
     - `lib.rs` — narrow intentional `pub` API (never `pub mod features` or raw internal slices).
   - **Preserves `doc-28` / `doc-96` headless machine-first contract (NEVER apply interactive `rust-cli-vsa` UX here):**
     - Single `org_command::CommandTable` (zero `clap`, zero positional args except explicit operator shorthands, zero TTY prompts/ANSI/wizards/completion cache/TOML config).
     - Stdout-only `{ok,data,meta,error}` JSON/YAML/compact envelopes via `org_command` (preserving CVE-2021-25743 escaping on ALL outputs).
     - Fixed exit codes `0`/`1`/`2`, server-side `--dry-run`, `<= 4 KiB` launch catalogue, and `<= 1 ms` p50 `session attach` hot path.

## 3. Domain Modules, Vertical Slices & Planned Variation

- Prefer **`application -> domain module -> feature/use case`** for business workflows.
- **Journaled `org-office` domain families** follow one uniform VSA shape:
  `<family>/{mod.rs, records.rs, ports.rs, projector.rs, features/<op>.rs, testkit.rs}` + SQLite adapter in `crates/org-office-sqlite/src/<family>.rs`.
  - Never import another domain's `features::*` across domain boundaries (share narrow public capabilities or small domain-neutral helpers instead).
  - Never put `rusqlite` or raw SQL outside `crates/org-office-sqlite/src/` or `crates/org-host-sqlite/src/` (`engine_boundary.rs`).
- **Keep leaf contracts and hexagonal adapters native (do NOT force into `features/`):**
  - Wire/protocol contracts (`org-proto`, `org-sync-proto`, `org-host-control`, `org-office-launch`) stay flat domain-schema leaves (`I ≈ 0.00`).
  - OS & vendor runtime adapters (`org-platform`, `org-platform-host`, `org-agent-runtime`, `org-agent-runtime-{claude,codex,agy}`, `org-socket`, `org-client`, `org-git`) stay capability-per-module hexagonal adapters.
  - Pure libraries (`org-store`, `org-pki`, `org-release`, `org-docs`, `org-process`, `org-sync`, `org-backup`, `org-copilot`) stay cohesive responsibility modules.
- **Planned variation behind consumer-owned adapters from day one:** OS differences live exclusively in `org-platform` (`libc`-only) & `org-platform-host` (`platform_conditionals.rs` enforces 0 allowlist entries); vendor agent differences live behind `org-agent-runtime` capabilities (never branch on `"claude"`/`"codex"`/`"agy"` in orchestration code).
- **Nothing legacy before v1 ships (Non-Negotiable #14):** replace cleanly rather than keeping dead shims, parallel crates, or generic `common.rs` / `utils.rs` / `helpers.rs` dumping grounds.

## 4. Browser UI Layout (`apps/agentic-org-ui`)

- Thin `src/routes/` navigation entry points; domain modules under `src/lib/modules/<domain>/{index.ts, types.ts, api.ts, features/<feature>/}`.
- Cross-module imports MUST go through the target module's `index.ts` public API with **zero circular dependencies** between modules.
- Shared primitives only under `src/lib/components/ui/` (never domain-specific panels or modals); HTTP/SSE transport only under `src/lib/platform/`.
- E2E fakes live strictly under `e2e/fakes/` (`vite.config.ts` enforces `productionImportGuard({ mode: 'error' })`).

## 5. Coupling Budgets & Automated CI Gates

- **Coupling guardrails:** Aim for efferent coupling `Ce <= 5` (`>7` MUST justify, `>10` MUST design-review); avoid stability inversions `I(source) + 0.30 < I(target)`; **zero circular dependencies** at crate and intra-crate module levels.
- **Keep all mechanical CI gates green (zero allowlist growth):**
  - `cargo test --locked -p org-platform --test file_size_ratchet` (<=800 target / <=1,000 hard cap; `.github/large-files.allow` = 0)
  - `cargo test --locked -p org-platform --test platform_conditionals` (0 platform `cfg` outside `org-platform*`; `.github/platform-conditionals.allow` = 0)
  - `cargo test --locked -p org-platform --test rust_hygiene` (`#![forbid(unsafe_code)]`, reasoned `#[allow]`)
  - `cargo test --locked -p org-platform --test libc_only`
  - `cargo test --locked -p org-office --test engine_boundary` and `-p org-host --test engine_boundary`
  - `cargo test --locked -p agentic-org-web --test dependency_closure`
  - `cargo test --locked -p org-platform --test vsa_structure` *(ships with `TASK-213`: CLI VSA boundaries + workspace intra-crate module-cycle ratchet + `org-office` cross-domain reach-in ratchet + module layout: no `include!`, no `#[path]` outside the declaring module's directory, no ungated module under a test-reserved name, no crate-module glob import outside the crate root)*
