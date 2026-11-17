# Linux host design

How Agentic Org runs on Linux, including as the main process of a container, under
[ADR-0017](../adr/0017-linux-and-container-hosting.md). It names the platform
contracts, the one crate that implements them for every operating system, the Linux
adapter of each, what each proves, and the order of work. The macOS adapters are the
code that exists today; their requirements stay in
[ADR-0008](../adr/0008-gateway-macos-sandbox.md),
[ADR-0011](../adr/0011-protected-recovery-delivery.md) and the
[host boundary design](office-host-boundary.md).

## 1. The shared platform crate

`crates/org-platform` holds every piece of code that differs by operating system, for
all four executables. Nothing else in the workspace contains `target_os`,
`target_vendor` or `target_family`, in source or in tests.

- **Contents.** One module per contract (`admission`, `credentials`, `confinement`,
  `service`, `isolation`) and per supporting capability (`fs`, `dirs`, `process`,
  `socket`, `presence`; section 4). Each module has `macos.rs`, `linux.rs` and
  `refused.rs`, and exports one non-conditional constructor that returns the build
  target's implementation or the refusing one. The refusing adapter returns
  `Refused::Unsupported { capability, os }` and never falls back to another system's
  behaviour. A non-Unix target is a `compile_error!`; every other Unix target gets the
  refusing adapters.
- **Contracts live in `org-platform`.** A contract here is an operating-system
  capability (start this verified image, keep these bytes secret, confine this
  process), and four executables and five libraries consume it, so no single consumer
  can own it. Policy stays with the consumer: which image is trusted, which secret
  names exist, when a check is skipped in a development build. The crate takes that
  policy as plain data (paths, digests, signing identifiers) and decides nothing about
  the domain.
- **Dependencies.** `libc` and `sha2`; on macOS also `security-framework`,
  `security-framework-sys`, `core-foundation` and the `cc` build of the recovery
  presence bridge, all moved from `org-host`. It depends on no workspace crate.
  `org-socket`, `org-supervise`, `org-client`, `org-office`, `org-host` and the
  applications depend on it; there is no cycle.
- **What moves in, and what stays.** `org-socket` keeps its portable socket code (the
  ticked reader, descriptor-passing framing); its `confinement`, `image` and the
  platform parts of `peer` and `rights` move into `org-platform`. `org-supervise`
  keeps process ownership policy; `src/platform.rs`, `src/recovery/native/` and the
  conditionals in `spawn.rs` and `child.rs` move into `org_platform::process`.
  `org-host` keeps the admission and custody policy; its Security-framework,
  Keychain, audit-token and SDK code moves.
- **Tests.** A test that needs a platform feature is an adapter test inside
  `org-platform`. Consumer tests are platform-neutral and run on both systems against
  the real adapter, or against a test adapter built from the contract.
- **The check (LNX-5).** CI runs one script that fails when any `.rs` file outside
  `crates/org-platform/` contains `target_os`, `target_vendor` or `target_family`. Its
  allowlist, `.github/platform-conditionals.allow`, names only test files gated today
  because Linux cannot yet start a worker; the script also fails when a listed file no
  longer has a conditional, so the list only shrinks, and it is empty when X2 is done.

Rejected: renaming `org-socket` to `org-platform`, because the rename ripples through
files the macOS track is editing (`apps/agentic-org-office/src/serve.rs`, the host
supervisor); and one contract per consumer crate, because the same five capabilities
would be declared four times.

## 2. What each executable needs from the platform

| Executable | Platform concern | Where it is today | Behind |
| --- | --- | --- | --- |
| `agentic-org` (`serve`) | Start an office worker and admit it | `org-host/src/spawn.rs`, `server_check.rs`, `code_signing.rs`, `peer.rs`, `supervisor/child.rs`, `supervisor/channels.rs` (`reader_is_child`) | `admission` |
| `agentic-org` (`serve`) | Start the web process and verify its confinement | Not wired (A3); host side is `org-socket/src/confinement.rs` (`verify_gateway`) | `admission`, `confinement` |
| `agentic-org` (`serve`) | Admit native clients on `host.sock` | `org-host/src/app_auth.rs`, `bootstrap.rs`, `human.rs` (`verify_host`), `peer.rs` | `admission` (peer) |
| `agentic-org` (`serve`) | Office secrets keys and host secrets | `org-host/src/keys/` (`mod.rs`, `keychain.rs`, `dev.rs`), `host_secrets.rs`, `host_secrets/` (`keychain.rs`, `dev.rs`) | `credentials` |
| `agentic-org` (`serve`, `setup`) | Register, start and stop the service; stop on a signal | Not built (A2); `org-host/src/signals.rs` is portable | `service` |
| `agentic-org` (`serve`, `setup`) | Release images: static check and the host's own image | `org-host/src/release/platform.rs`, `release/runtime.rs` (`check_self`), `org-socket/src/image.rs` | `admission` |
| `agentic-org` (`setup`, `serve`) | Instance, temp and socket locations; filesystem checks | `org-host/src/home.rs`, `instance/directory.rs`, `instance/mod.rs`, `instance/temp.rs`, `instance/setup.rs`, `human.rs` (`paths`, `user_temp`) | `dirs`, `fs` |
| `agentic-org` (`serve`) | Durable flush and exclusive rename | `org-host/src/writer.rs`, `hostdb.rs`, `release/files.rs`, `release/files/output.rs`, `human/recovery.rs`, `host_secrets/dev.rs` | `fs` |
| `agentic-org` (`recovery`) | Local presence and confirmation for the recovery code | `org-host/src/recovery/presence.rs`, the bridge in `org-host/build.rs` | `presence` |
| `agentic-org-office` | Its end of the rendezvous: receive descriptors | `org-socket/src/rights.rs`, `rights/send.rs`, `rights/stream.rs` | `socket` |
| `agentic-org-office` | Admit same-user peers on its local socket | `org-socket/src/peer.rs` (`peer_allowed`), called from `apps/agentic-org-office/src/serve.rs` | `socket` |
| `agentic-org-office` | Start, watch, stop and recover agent processes | `org-supervise/src/platform.rs`, `spawn.rs`, `child.rs`, `recovery/native/` | `process`, `isolation` |
| `agentic-org-office` | Durable flush of accepted work | `org-office/src/store/acceptance/durable.rs` | `fs` |
| `agentic-org-web` | Confine itself before input | `org-socket/src/confinement.rs`, `confinement/macos.rs` | `confinement` |
| `agentic-org-web` | Check the inherited listener and received descriptors | `org-socket/src/rights/stream.rs` (`listening`), `rights.rs` | `socket` |
| `agent-os` | Connect to the office and check its peer | `org-client` through `org-socket/src/peer.rs` | `socket` |
| `agent-os` | Durable queue writes and safe reads | `org-client/src/agent.rs` (`rename_new`, `full_sync`), `org-spool/src/safe_read.rs` (deleted with B6) | `fs` |

Portable already, and staying where they are: core-dump limits in each `main.rs`,
signal handlers in `apps/agentic-org-office/src/signals.rs`, `apps/agent-os/src/main.rs`
and `org-host/src/signals.rs`, and the descriptor swap in
`apps/agentic-org-office/src/fdswap.rs`. No executable's own source has a platform
conditional outside its tests; the tests that have one are listed in the allowlist of
section 1 or move into `org-platform`.

## 3. The five contracts

### 3.1 Process admission

### 3.2 Credential provider

### 3.3 Confinement of the web process

### 3.4 Service lifecycle

### 3.5 Agent isolation

## 4. Supporting capabilities

## 5. What a local attacker can still do on Linux

## 6. What this design was checked against

## 7. Working in parallel with the macOS track
