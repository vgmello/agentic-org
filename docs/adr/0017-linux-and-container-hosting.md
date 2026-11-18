# ADR-0017 · v1 runs on macOS and Linux, including inside a container

Status: **accepted by the owner 2026-11-17** (conflict plan entry B-09): "we need both;
I need to be able to host the office inside a Docker container; we need to abstract what
we need properly." Linux is built now, as its own track alongside milestone one
(owner decision 2026-11-17); the design is [Linux host](../design/linux-host.md) and the
increments are X1a to X6 of the implementation plan. Implementation pending. Amends the platform statements of
[standalone-server.md](../design/standalone-server.md),
[office-host-boundary.md](../design/office-host-boundary.md),
[ADR-0008](0008-gateway-macos-sandbox.md), which is macOS only, and
[ADR-0011](0011-protected-recovery-delivery.md), which deferred a Linux release.

## Context

- The host can start office workers only on macOS today. On any other platform the
  spawn returns "office servers run only on macOS"
  (`crates/org-host/src/spawn.rs` when this was written; since X1c the admission
  contract's Linux adapter, `crates/org-platform-host/src/admission/linux.rs`).
- Worker admission on macOS checks the child's code signature through its audit token.
  Linux has no equivalent in the code.
- The web process confines itself with a macOS Seatbelt profile. On every other system
  a refusing adapter reports that confinement is unavailable.
- The host's secrets use the macOS keychain; a Linux release build has no store.
- No service is registered on any system yet. A container has neither launchd nor a
  user's systemd.

## Decision

1. **Both platforms are v1.** macOS and GNU/Linux are supported hosts, and a Linux
   instance may run as the main process of a container image.
2. **Platform differences sit behind five small contracts.** Each has a macOS and a
   Linux implementation and a refusing adapter for every other system, chosen at
   composition. The five are not the only differences: the
   [design](../design/linux-host.md#4-the-other-platform-capabilities) names five
   further capabilities (`fs`, `dirs`, `process`, `socket`, `presence`), which follow
   the same rule, and the
   [adapter rule](../architecture-rules.md#planned-variation-goes-behind-an-adapter-from-day-one)
   governs any difference found later. Nothing outside these adapters branches on the
   operating system.
   | Contract | Question it answers | macOS | Linux and containers |
   | --- | --- | --- | --- |
   | Process admission | Is the child the program we meant to start? | Code signature through the audit token | The verified release image: the pinned path, its content digest from the release manifest, and the child's identity from the kernel |
   | Credential provider | Where do host secrets live? | Keychain | A host-only directory with owner-only permissions; in a container, a mounted secret volume |
   | Confinement | What may the web process touch? | Seatbelt (ADR-0008) | Landlock and seccomp; the adapter must exist, and a container's own restrictions do not replace it |
   | Service lifecycle | Who starts and restarts the host? | launchd | A systemd user unit, or the container runtime running `agentic-org serve` in the foreground |
   | Agent isolation | How is an agent kept from the data? | The modes of ADR-0015 | The same modes; `dedicated-user` uses an account created in the image or by setup |
3. **No platform fails open.** On a system with only a refusing adapter, or where a
   kernel feature is unavailable, the affected function refuses to start and says why. A container
   without the kernel features confinement needs is reported, not silently accepted.
4. **A container is a normal instance.** The container holds the whole instance: the host, the web process and the office. A satellite office in a container is the same image enrolled as a satellite. `agentic-org serve` runs in the foreground and
   stops cleanly on the runtime's stop signal. The organization data root is a mounted
   volume. Setup can run without a terminal from a file or environment configuration,
   offline, as the standalone design already requires. Nothing the instance needs to
   keep is written outside the mounted volumes.
5. **One build, not a fork.** The same workspace builds both platforms. Tests that need
   a platform feature are marked for it; the rest run on both in CI.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| LNX-1 | On Linux, the host starts an office worker, the worker serves the local socket, and `agent-os` completes a send and a read. |
| LNX-2 | The same in a container built from the repository's container file, with only mounted volumes for data and secrets, and again after the container is replaced. |
| LNX-3 | With the Linux kernel feature unavailable, the web process refuses to start and reports the reason. |
| LNX-4 | Host secrets on Linux are readable only by the service account; none is in the data volume. |
| LNX-5 | No `.rs` file outside `crates/org-platform` has a `cfg`, `cfg_attr` or `cfg!` predicate on `target_os`, `target_vendor`, `target_env` or `target_family`; a CI check that parses the source enforces it. |
| LNX-6 | The CI matrix runs the workspace checks on macOS and Linux. |

## Decisions on Linux (2026-11-17)

These settle what the decisions above left open. The detail is in the
[Linux host design](../design/linux-host.md).

6. **Platform differences live in two crates and nowhere else.** `crates/org-platform`
   owns the contracts and capabilities that the four executables share, with macOS,
   Linux and refusing adapters and one non-conditional constructor each. It depends on
   the C library only and on no workspace crate.
   Consumers keep their policy (which image, which secret names) and pass it as data.
   This is a deliberate exception to the rule that a consumer owns its contract
   ([architecture rules](../architecture-rules.md#planned-variation-goes-behind-an-adapter-from-day-one)):
   these capabilities are consumed by four executables and six libraries, so no one
   consumer can own them.
   `crates/org-platform-host` (amended 2026-11-18, during X1b) holds, under the same
   rules, the capabilities that only the host consumes and whose macOS adapter needs
   an Apple framework: admission's code-identity checks, credentials, presence and the
   comparison of file names. A crate's link directives reach every executable that
   links it, so one crate for everything would have made the web process, the office
   worker and `agent-os` load Security, CoreFoundation and LocalAuthentication on
   macOS, and the web process must load nothing but the C library before it confines
   itself (ADR-0008). Only `org-host` depends on `org-platform-host`; a test keeps
   `org-platform` libc-only and the other executables' crates free of it. Rejected: a
   Cargo feature on one crate, because features unify across a workspace build, so
   what the web process links would depend on how it was built.
7. **Linux process admission executes verified bytes.** The host copies the pinned
   executable into a sealed, execute-only memory file (with `MFD_EXEC` from Linux 6.3,
   without it before), compares its SHA-256 with the signed release manifest (in a
   development build, with the bytes read at host start), and starts the child from
   that file with `execveat`, holding a pidfd. Loader variables (`LD_*`, `DYLD_*`,
   `GLIBC_TUNABLES`) are refused in every child's environment. The rendezvous peer
   must match by `SO_PEERCRED`. There is no start-suspended step: the bytes are fixed
   before `exec`, and the execute-only image starts a non-root child non-dumpable. The
   executable is hashed; the loader and shared libraries are trusted because root owns
   them. A release build without a runtime release pin refuses. The macOS signing
   identifiers are the macOS adapter's data; Linux has no equivalent identifier.
8. **The Linux credential store is an owner-only directory outside the data root.**
   `${XDG_STATE_HOME:-~/.local/state}/agentic-org/credentials/<instance-id>/`, which
   setup creates, or a mounted secrets volume in a container, recorded at setup. Mode
   `0700`, items `0600`, owned by the service user; ancestors not writable by others
   or by any group but the user's private group; on ext4, XFS, Btrfs or F2FS only;
   disjoint from the data root by device and inode; a mount point in a container; and
   an empty store for an instance whose records say keys exist refuses. Anything else
   refuses. The same engine is every development build's store.
9. **The web process confines itself with Landlock and seccomp.** It connects to the
   rendezvous, sets no-new-privileges, applies Landlock (ABI 3 or higher, every
   filesystem right handled, no rule) and a seccomp allowlist that kills on any other
   call, then reports ready. The host checks seccomp and no-new-privileges in
   `/proc/<pid>/status`. Linux hosts need kernel 6.2 or later with Landlock enabled,
   and glibc 2.34 or later; other C libraries get the refusing adapters.
10. **Service.** A systemd user unit on a Linux machine; in a container, `agentic-org
    serve --setup <file>` in the foreground under `tini`, stopping within 8 seconds on
    `SIGTERM`. Setup's non-interactive configuration names the mode and the credential
    directory; nothing detects a container. `serve` refuses to run as root. The
    container runs as UID 61000 with the repository's seccomp and AppArmor profiles,
    listens with HTTPS on its network interface with an operator-supplied certificate,
    and its first registration link comes only from
    `docker exec -u 0 <container> agentic-org admin reset --user <service user>`.
11. **Recovery, reset and native clients on Linux.** A native client on `host.sock` is
    admitted by its kernel UID; Linux has no code identity for it, so the "verified
    signed `agentic-org` client" of ADR-0011 means, on Linux, a peer with the service
    user's UID, and authority comes only from the browser-approved session. The
    recovery code's confirmation is the administrator's browser approval of the run
    (ADR-0011); the local context is a controlling terminal in the foreground. Against
    a same-user process these are a gate against mistakes, not a security control,
    because that process can read the pending code from the store. `admin reset` is
    authorized by root (SIGN-33), the one method `host.sock` admits root for.
12. **Same-user agents.** Nothing new on Linux. An agent whose vendor sandbox is
    unavailable, as in a container that refuses user namespaces or on a host that
    restricts them through AppArmor, is not started.

## Consequences

- Linux stops being a later port. Its adapters are v1 work, built as Track X in
  parallel with milestone one.
- macOS code-signature admission has no direct Linux equivalent. The Linux check is
  weaker against a local attacker who runs as the service user: such a process can
  read the credential store and replace the host's own executable before its next
  start. The design's section 5 lists what such an attacker can still do.
