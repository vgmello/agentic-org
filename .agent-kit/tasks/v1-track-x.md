# Track X: Linux and containers (X1a to X6)

The Track X lead's plan and state for milestone two of the
[implementation plan](../../docs/implementation-plan.md#milestone-two--linux-in-parallel-track-x).
The stable contract is [ADR-0017](../../docs/adr/0017-linux-and-container-hosting.md)
(criteria LNX-1 to LNX-6) and the [Linux host design](../../docs/design/linux-host.md);
the checklist row is FC-164. Every handoff and review of this track also carries
[independent delivery and dependency boundaries](../../docs/independent-delivery.md).

## How this track is run

- **Lead and models.** The lead is a Claude session (model identifier recorded in the
  session, not here). The kit's active profile names `gpt-6.1-sol` for every phase;
  this lead runs on another model, so, as AGENTS.md requires, every phase and gate is
  kept and run with fresh-context subagents of the lead's own model: plan,
  implementation, fresh read-only review, repairs, original-author validation and a
  separate fresh final review. The Codex wrapper (`kit_codex.py`), Repowise and
  Hindsight are not installed in the cloud container this lead runs in, so
  `kit_hooks.py check` cannot pass there (its `kit_hindsight_embed.py test` step
  fails on the missing tool); the remaining kit checks and the project checks are run
  directly and their results are recorded per increment below.
- **Where the work is.** Branch `claude/hopeful-noether-lsmbac`, started from
  `origin/v1-integration` at `032ea63`. The session that runs this lead may push only
  that branch; the merge into `v1-integration` is made by a session that may.
- **Systems.** Development and the full Linux checks run on Linux x86-64 (kernel
  6.18, as root in a container). macOS code is type-checked locally with
  `cargo check --target aarch64-apple-darwin` where the crate has no C build, and
  built and tested by the macOS CI job on every push. The signed-build, Keychain,
  Seatbelt and presence-dialog checks on a Mac are owed by the Track A engineer
  before X1c and X1d are relied on (design, section 7).
- **Writers.** One writer per file. Where an increment is split, each worker has its
  own worktree and file list, recorded under that increment.

## Increments

| Increment | State | Evidence |
| --- | --- | --- |
| X1a crate, `service` contract, check | Built 2026-11-18 | See below |
| X1b `fs`, `dirs`, `process`, `socket` | Not started | |
| X1c `admission`, `confinement`, `presence` | Not started | |
| X1d `credentials` | Not started | |
| X2 first worker on Linux (LNX-1) | Not started | |
| X3 host secrets on Linux (LNX-4) | Not started | |
| X4 web process confined on Linux (LNX-3) | Not started | |
| X5 service and container (LNX-2) | Not started | |
| X6 both systems in CI (LNX-5, LNX-6) | Not started | |

## X1a · The platform crate and the check

**Outcome.** An internal foundation; nothing a person can see. No change can add a
platform predicate outside `crates/org-platform`, and A2 has a `service` contract to
write the launchd registration behind.

**Prerequisites.** None. **Early production-path proof.** The check runs against the
real workspace sources in `cargo test --workspace` and as its own CI step on both
systems.

**Files owned.** New: `crates/org-platform/` (`Cargo.toml`, `src/lib.rs`,
`src/refusal.rs`, `src/service.rs`, `src/service/{macos,linux,refused,testing,tests}.rs`,
`tests/platform_conditionals.rs`, and the X1b module stubs `src/{dirs,fs,process,socket}.rs`),
`.github/platform-conditionals.allow`, `.github/scripts/platform_allowlist_shrinks.py`.
Edited: the workspace `Cargo.toml` and `Cargo.lock`, `.github/workflows/ci.yml` (one
step and one job), `docs/design/linux-host.md`, `docs/implementation-plan.md` (X1a),
`docs/FEATURES.md` and `docs/FEATURE-AREAS.md` (FC-164), this file, `v1-state.md`,
`.agent-kit/project-skills/rust-workspace/SKILL.md` (the platform rule for every Rust
worker) and `.agent-kit/skill-catalog.json` (`crates/org-platform` joins the critical
host-and-storage area; evidence hashes).
**Deleted.** Nothing.

**Decisions taken where the documents were silent** (each is written into the
[design](../../docs/design/linux-host.md), sections 1 and 3.4):

1. The check is a test of `org-platform`, not a separate script: `cargo test
   --workspace` runs it for every engineer and on both CI systems with no extra
   tooling, and CI names it as its own step for a fast, readable failure.
2. The allowlist carries a count per file, and the check requires the exact count.
   A file-only list would let a listed file gain predicates unnoticed; the plan's
   "Done when" requires failing on a new predicate.
3. The check works on each file's whole token stream, with `syn` as the gate that the
   file is Rust. A walk of the syntax tree missed predicates in places `syn` keeps as
   raw tokens (review findings R2, R3). It also counts `cfg_select!` and `cfg_match!`,
   raw identifiers, and any other use of the four target names, which are reserved
   for `org-platform`.
4. A second CI job compares the list with the commit where the branch left
   `v1-integration` and fails on an added path or a raised count. Without it "the list
   only shrinks" was a convention.
5. Every capability has the same layout: a contract module and `macos`, `linux` and
   `refused` adapters declared by `adapters!()`. The refusing adapter is compiled and
   tested on every target. "Linux" is GNU/Linux; another C library gets the refusing
   adapter.
6. The `service` contract: `InstanceId` and `Unit` are checked types, so an adapter
   cannot receive an unchecked one; `ServiceStatus` is one enum, so a status cannot
   combine states that do not go together; a foreign registration is `Foreign` for
   every method; `register` never starts; each mode's behaviour for every method is
   stated and held by `service::testing::check_registered` and `check_foreground`.
7. Foreground mode is built now, the same code on macOS and Linux, because it calls
   nothing in the system. Registered mode refuses on every target until A2 (launchd)
   and X5 (systemd).
8. `ServiceManager` is `Send + Sync`, so a composition can hold one across threads.

**Still open.** The A2 lead reviews the `service` contract when A2 starts; that is
the unmet part of the plan's "Done when". Nothing in Track X depends on it before X5.

**Checks and review.** Recorded here when run.
