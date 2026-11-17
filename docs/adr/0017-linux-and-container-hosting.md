# ADR-0017 · v1 runs on macOS and Linux, including inside a container

Status: **accepted by the owner 2026-11-17** (conflict plan entry B-09): "we need both;
I need to be able to host the office inside a Docker container; we need to abstract what
we need properly." Implementation pending. Amends the platform statements of
[standalone-server.md](../design/standalone-server.md),
[office-host-boundary.md](../design/office-host-boundary.md) and
[ADR-0008](0008-gateway-macos-sandbox.md), which is macOS only.

## Context

- The host can start office workers only on macOS today. On any other platform the
  spawn returns "office servers run only on macOS"
  (`crates/org-host/src/spawn.rs`).
- Worker admission on macOS checks the child's code signature through its audit token.
  Linux has no equivalent in the code.
- The web process confines itself with a macOS Seatbelt profile. The Linux confinement
  is a contract with no adapter, so it fails closed.
- The host's secrets use the macOS keychain. The Linux provider was undecided.
- Service registration assumes launchd or a systemd user unit. A container has neither.

## Decision

1. **Both platforms are v1.** macOS and GNU/Linux are supported hosts, and a Linux
   instance may run as the main process of a container image.
2. **Platform differences sit behind five small contracts.** Each has a macOS and a
   Linux implementation, chosen at composition. Nothing outside these adapters branches
   on the operating system.
   | Contract | Question it answers | macOS | Linux and containers |
   | --- | --- | --- | --- |
   | Process admission | Is the child the program we meant to start? | Code signature through the audit token | The verified release image: the pinned path, its content digest from the release manifest, and the child's identity from the kernel |
   | Credential provider | Where do host secrets live? | Keychain | A host-only directory with owner-only permissions; in a container, a mounted secret volume |
   | Confinement | What may the web process touch? | Seatbelt (ADR-0008) | Landlock and seccomp; the adapter must exist, and a container's own restrictions do not replace it |
   | Service lifecycle | Who starts and restarts the host? | launchd | A systemd user unit, or the container runtime running `agentic-org serve` in the foreground |
   | Agent isolation | How is an agent kept from the data? | The modes of ADR-0015 | The same modes; `dedicated-user` uses an account created in the image or by setup |
3. **No platform fails open.** Where an adapter is missing or a kernel feature is
   unavailable, the affected function refuses to start and says why. A container
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
| LNX-3 | With the Linux confinement adapter removed or its kernel feature unavailable, the web process refuses to start and reports the reason. |
| LNX-4 | Host secrets on Linux are readable only by the service account; none is in the data volume. |
| LNX-5 | No source file outside the five adapters contains a platform conditional for these concerns. |
| LNX-6 | The CI matrix runs the workspace checks on macOS and Linux. |

## Consequences

- Linux stops being a later port. Its admission, credential and confinement adapters
  are v1 work, and each needs a short design before code.
- macOS code-signature admission has no direct Linux equivalent. The Linux check is
  weaker against a local attacker who can replace files as the service user, and the
  design must say so.
