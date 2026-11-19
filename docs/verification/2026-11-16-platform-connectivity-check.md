# Platform availability and connectivity smoke check

Date: 2026-11-16. These checks were executed in the side conversation against the
available development machine and its existing Podman VM. They establish usable
test infrastructure, not application end-to-end acceptance. Follow the
[office validation procedure](../processes/office-integration-validation.md),
including its change-driven rerun rules, for subsequent feature checks.

## Observed environment

| Component | Actual observation |
| --- | --- |
| Native host | macOS 26.7.2, build 25G332, arm64 |
| Native SDK/compiler access | `xcrun --show-sdk-version` returned 27.2; native Rust reported 1.99.1 |
| Podman | Client/server 6.1.4; existing `podman-machine-default` running through libkrun, 4 CPUs and approximately 4 GB memory |
| Linux VM | Fedora Podman machine OS 44, aarch64, kernel `7.2.7-200.fc44.aarch64` |
| VM service manager | `ps -p 1 -o comm=` returned `systemd`; `systemctl is-system-running` returned `running` |
| Cached Linux build image | `rust:1.99.1-bookworm`, local image ID prefix `10513cd0b64b`; Rust/Cargo 1.99.1 and Debian GCC 12.2.0 executed |
| Cached minimal client image | `alpine:3.20`, local image ID prefix `ab3fe4defd29`; used for disposable HTTP clients |

Image IDs above identify the local images inspected, not portable full-digest
release pins. This run does not establish native x86-64, another Linux kernel,
another macOS version, or the advertised production support matrix.

## Executed checks and results

| Check | Method | Result |
| --- | --- | --- |
| Native macOS compilation/execution | Compiled a temporary C program with `xcrun clang -Wall -Wextra -Werror`, then executed it | Pass: `native-macos-execution-ok` |
| Linux access and toolchain | Read-only VM queries plus a disposable cached Rust container with no network, no capabilities and a writable temporary filesystem | Pass: kernel/architecture and Rust/Cargo/C compiler commands executed; temporary filesystem writable |
| macOS → Linux | Native Python HTTP client reached a Python HTTP server in a separate Linux container through a randomly assigned `127.0.0.1`-only published port | Pass: expected `office-container-connectivity-ok` response |
| Linux → Linux | A separate Alpine container used `wget` to reach the server container's private bridge IP | Pass: same expected response |
| Linux → macOS | An Alpine container used `host.containers.internal` to reach a temporary native Python HTTP server bound to `127.0.0.1` on an ephemeral port | Pass: expected `native-macos-loopback-ok` response |
| Cleanup | Removed the exact uniquely named server container, confirmed it absent, used automatically removed clients and closed the temporary native HTTP server | Pass: owned resources cleaned; existing VM/services not restarted or changed |

Containers used cached images with `--pull=never`, read-only root filesystems,
`--cap-drop=all`, `--security-opt=no-new-privileges`, bounded memory/process counts
and no host filesystem mounts. The HTTP checks used synthetic fixed response
bodies and no application identities, accounts, credentials or user data. The
three reachability directions used separate short-lived probes, not a concurrent
full-office topology or TLS authentication test.

## Retained failure and evidence limits

The first HTTP server attempt exited before inspection. A subsequent image probe
showed that the minimal Alpine image had no `httpd` command. The corrected probe
used the already available Python server in the cached Rust image and passed.
No application code, assertion, security gate or capability allowance was changed
to obtain that pass.

Commands and outputs were observed in the side conversation. This note records
those observations after execution; no standalone raw-log bundle, source-bound
application receipt or fresh independent review was produced for these probes.
They ran synthetic programs rather than a repository application build, so there
is no application commit to certify. Future real-office acceptance must retain
the exact artifact identities, raw diagnostics and required reviews described in
the procedure.

Not exercised: actual HQ/satellite setup or enrollment, office protocol/TLS,
authenticated task/control/synchronization, application persistence/recovery,
application-installed security policy, native application service lifecycle,
reboot, physical OS authentication, provider access or authentic release signing.
The presence of systemd does not prove those application service checks passed.

No extra machine or Podman installation was needed for these development probes.
Missing product adapters/harnesses remain implementation work. Signing inputs and
genuine signed-release acceptance remain the final phase; this infrastructure
check does not waive them or establish a production-ready release.
