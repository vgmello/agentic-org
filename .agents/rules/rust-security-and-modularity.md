---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - ".github/large-files.allow"
---

# Rust security and modularity rules

These rules apply to every Rust change in this repository. They exist because god-object
files (6,000-line `serve.rs`, 5,600-line `manager.rs`, 4,700-line `satellite.rs`,
4,200-line `federation.rs`) and the same POSIX, UTF-8, redaction and concurrency mistakes
got past authoring and review again and again (TASK-178). Tests enforce the file-size and
platform rules (sections 1 and 3.5). For everything else, the author runs the self-check in
section 3 before every commit, and the reviewer checks the same six points.

`backlog/docs/doc-8 - Architecture-Rules.md` (parent `agentic-org-project`) still governs
architecture: crate boundaries, thin binaries, adapters for planned variation. This file adds
the rules that apply inside a crate.

## 1. File size: aim for 800 lines or fewer; 1,000 is the hard cap

| Rule | Value |
| --- | --- |
| Target for a production `.rs` file | **800 lines or fewer** |
| Hard cap for a production `.rs` file | **1,000 lines** |
| Files over the cap when the rule was written | listed in `.github/large-files.allow`; **a listed file may only shrink** |
| Enforced by | `crates/org-platform/tests/file_size_ratchet.rs` (CI: `rust` job and `large-files-list` job) |

**What counts as a line.** Every line of the file, as `wc -l` counts it (plus a final
line with no newline). Nothing is parsed or stripped. Inline `#[cfg(test)] mod tests { .. }`
blocks, doc comments and blank lines all count. Counting this way is deterministic and
easy to check. It also pushes large inline test modules out of production files. For
`foo.rs`, move them to `foo/tests.rs` (`#[cfg(test)] mod tests;`) or to `foo_tests.rs`
(`#[cfg(test)] #[path = "foo_tests.rs"] mod tests;`). Lines can't be hidden by reformatting:
never use `#![rustfmt::skip]` or very long lines to get under the cap.

**Production versus test code** is decided by path alone. A `.rs` file is **dedicated test
code**, which the cap does not count, when one of its directories, or the file name without
`.rs`, is:

| Name | Examples in this repository |
| --- | --- |
| `tests` | Cargo `tests/` targets, `src/**/tests.rs`, `src/**/tests/*.rs` |
| `*_tests`, `*-tests` | `wake_tests.rs`, `web_process_tests/`, `apps/agentic-org/setup-tests/` |
| `testing`, `testkit`, `test_support` | `#[cfg(any(test, feature = ..))]` test adapters and conformance kits |

Every other `.rs` file anywhere in the repository counts as production code. That includes
`build.rs`, `examples/`, `benches/`, and `#[cfg(test)]` modules with other names (for example
`test_teardown.rs`). The names in the table are reserved for code compiled only for tests
(`#[cfg(test)]`, a test-only feature, or a Cargo test target). Never put production code under
them, or load it through `#[path]`, to dodge the cap.

**The list only shrinks.** `.github/large-files.allow` holds lines of the form `path lines`,
sorted by path, after a `#` comment header. Each count is the file's **exact** current line
count, so the list always says what is left. The ratchet test fails when:

- a production file that isn't listed has more than 1,000 lines. Split it; never add it to the list.
- a listed file has more lines than its entry. A listed file never grows.
- a listed file has fewer lines than its entry. Lower the entry in the same change so the lines you removed can't come back.
- a listed file is at 1,000 lines or fewer, or no longer exists. Delete its line.
- the list is unsorted, repeats a path, lacks its comment header, lists test code, or is malformed.

The `large-files-list` CI job compares the list with the commit where the branch left `v1`.
It fails when a count rises, or when a path is added that isn't a rename. A renamed or moved
file may show up under its new path at no more than its old count. It pairs one-to-one with a
removed entry whose file is gone. Lines removed from a file that still exists never pay for a
new entry, so a piece split out of a listed file is never listed: each piece is within the
cap. The check can't tell a rename from a split that deletes the old path and moves most of it
to one new path, so review enforces that. Swapping two listed paths takes two changes.

**When your change touches a listed file:**

1. Keep the file's net line change at zero or below. If the fix needs new lines, first
   extract a cohesive responsibility into a new module of 800 lines or fewer (a separate
   preparatory commit is best), then make the fix.
2. Run the ratchet and copy the counts it reports into `.github/large-files.allow`.
3. Never raise a count or add a path (a rename keeps its entry under the new path, at no
   more than the old count). If you think a file has to grow, raise it with the increment's
   lead rather than editing the list.

**How to split.** Split by responsibility, following the vertical-slice layout of `doc-68`
(`backlog/docs/design/doc-68 - Design-v1-Project-Structure.md`). Each slice gets a module
with one reason to change. The parent module becomes a thin facade of `mod` and `pub use`
lines. Mechanical chunking (`part2.rs`, `helpers2.rs`, `misc.rs`) and moving code into a
`*_tests`/`testkit` name it doesn't belong to are not splits.

```sh
cargo test --locked -p org-platform --test file_size_ratchet
# The current counts over the cap, in the list's format:
FILE_SIZE_RATCHET_PRINT=1 cargo test -p org-platform --test file_size_ratchet -- --nocapture
# What CI's shrink-only job runs:
FILE_SIZE_RATCHET_BASE=$(git merge-base HEAD origin/v1) \
  cargo test --locked -p org-platform --test file_size_ratchet -- --include-ignored
```

## 2. One choke-point per security-critical concern

A security check written at every call site eventually gets missed at one of them. Each
concern below has **one** implementation, and every call site goes through it:

- **Logging and redaction.** Each crate has one redacting choke-point. All text that comes
  from outside the process goes through it before it is logged, persisted, returned in an
  error or sent over the wire. That covers subprocess stdout/stderr, provider and git
  output, error chains, transcripts and headers. Example: `org-office-sessions`'
  `transcript/redact.rs` (`redact_text`, `redact_json`). Extend the choke-point; never add
  a local `replace`/regex scrub beside it.
- **Live-handle and process teardown.** One function reaps a live session or process handle
  and releases its slots. Everything that can end a handle calls that one function: stop,
  crash, timeout, abort before the pump starts, startup failure and shutdown.
- **Path and sandbox validation.** One validator decides whether a path may be read, written,
  mounted or exposed to a sandbox. Callers pass it raw input and use only what it returns.

When you find a duplicated check, fold it into the choke-point in the same change. If that
can't be done in the same change, record a follow-up backlog task that names both sites.
Choke-points fail closed, and their adversarial tests (point 6) target the choke-point itself.

## 3. Six-point pre-commit security and systems self-check (mandatory)

Before **every** commit that touches Rust, go through all six points. Mark each one as
applying or not applying to the change. Fix anything that applies before you commit.

### 3.1 Paths and POSIX

- Strip trailing `/` and `/.` before `lstat`/`symlink_metadata`. `lstat("link/")` resolves
  the link, so the symlink check passes on a path it should reject.
- Reject `..` components, NUL bytes and any other traversal **before** joining or opening.
  Compare `Path` components, never string prefixes (`/etc2` is not inside `/etc`).
- Check forbidden roots **symmetrically**. A path is refused when it lies inside a forbidden
  root (`path.starts_with(root)`) **or** contains one (`root.starts_with(path)`): mounting
  `/home` exposes `/home/<user>/.ssh`.
- Open sensitive host files with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, through the
  `org-platform` contract where one exists. `std::fs::File` already sets `O_CLOEXEC`; keep it
  when calling `libc` directly. `O_NONBLOCK` stops a FIFO from blocking the open.
- `O_NOFOLLOW` refuses a symlink only in the **last** path component. Parent directories
  that are symlinks are still followed. Where a parent can be attacker-controlled, resolve
  without symlinks: `openat2` with `RESOLVE_NO_SYMLINKS`/`RESOLVE_BENEATH` on Linux,
  `O_NOFOLLOW_ANY` on macOS, or `openat` one component at a time, all behind
  `org-platform`.
- Check `is_file()` on the **opened** handle's metadata, not a separate `stat` (time of
  check to time of use). Bound every read: read with `take(limit + 1)`, and fail if more
  than `limit` bytes come back.

### 3.2 Strings and UTF-8

- Never call `String::truncate(n)`, slice `&s[..n]`, or `split_at(n)` with a byte count
  unless you have checked `s.is_char_boundary(n)`. Use the floor helper:
  `s.truncate(s.floor_char_boundary(n))`.
- Budgets for logs, previews and wire fields are in bytes. With a suffix such as
  `… [truncated]`, cut at `floor_char_boundary(budget - suffix.len())` and then append the
  suffix, so the result stays within the budget. Handle a budget smaller than the suffix.

### 3.3 Redaction and logging

- Route every external, subprocess and error string through the crate's single redacting
  choke-point (section 2) before it reaches a log, an error, storage or the wire.
- The scrubber must handle quoted values (`"token": "…"`, `key='…'`) and work line by line,
  so one secret can't hide the rest of the line. Header names are case-insensitive
  (`Authorization`, `authorization`, `X-Api-Key`). It must remove PEM blocks
  (`-----BEGIN … KEY-----` through `END`), URL userinfo (`https://user:pass@host`), secret
  query parameters (`?token=`, `access_token=`, `sig=`), and bearer/token patterns.
- Always add adversarial tests: mixed case, quotes, several secrets on one line, secrets
  split across lines, and multibyte text next to a secret.

### 3.4 Concurrency and lifecycles

- List **every** reader, writer and remover of shared state before changing it, for example
  `reg.live`, `reg.starting`, `active_permit` and `RecoveryRequired`. A fix at one site
  that leaves the others alone is a race.
- Hold slot guards through teardown. Release them only after teardown completes or fails
  closed. A panicking teardown must still release them, so use RAII guards and not manual
  release at the end of a function.
- Persist durable recovery state (for example `RecoveryRequired`) with an **atomic upsert**
  **before** releasing the guard that protects the slot, so a crash in between can't lose it.
- Never hold a coarse lock (registry, store, map mutex) across a network RPC, a subprocess
  wait or a disk walk. Snapshot what you need, drop the lock, do the slow work, then
  re-acquire the lock and re-validate.

### 3.5 Platform gate (ADR-0017 LNX-5)

- Zero `#[cfg(unix)]`, `#[cfg(windows)]`, `#[cfg(target_os = ..)]` (also `cfg!`,
  `cfg_attr`, `cfg_select!`, and `target_vendor`/`target_env`/`target_family`) outside
  `crates/org-platform/`, plus `crates/org-platform-host/` for what only the host links.
  Put the difference behind an `org-platform` contract.
- Always run `cargo test --locked -p org-platform --test platform_conditionals`.

### 3.6 Negative and adversarial tests

Every security, isolation or concurrency fix includes tests of the failure mode it closes,
not just the happy path. Cover whichever of these apply:

- symlinks, including dangling, looping and trailing-slash paths;
- teardown that panics;
- missing or vanished files;
- concurrent contention for the same slot or record;
- UTF-8 multibyte truncation at the boundary;
- oversized input that hits a read bound;
- a FIFO or device where a file was expected.

## 4. Before every Rust commit

```sh
cargo fmt --all
CARGO_BUILD_JOBS=2 cargo clippy --locked -p <crate> --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --locked -p org-platform --test file_size_ratchet
CARGO_BUILD_JOBS=2 cargo test --locked -p org-platform --test platform_conditionals
CARGO_BUILD_JOBS=2 cargo test --locked -p <crate>
# When .github/large-files.allow changed (CI's large-files-list job):
FILE_SIZE_RATCHET_BASE=$(git merge-base HEAD origin/v1) CARGO_BUILD_JOBS=2 \
  cargo test --locked -p org-platform --test file_size_ratchet -- --include-ignored
```

Before merging, run the full list of required checks in `DEVELOPMENT.md#required-local-checks`.
