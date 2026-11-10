# Platform probes for standalone v1

These disposable B2-04 probes run only on `sage/b2-04-probes`, from `v1` at
`4e15a89c3806b0612f9fa3b815b95597f3d42f2d`. Ada authorized the branch and Actions
run on 2026-11-10. Never merge this branch. It adds no production behavior.

The workflow runs on a native GitHub-hosted Ubuntu 26.04 x86-64 VM, with two Cargo
build jobs. Each worker/credential/database experiment uses disposable state.
The user's organization home and the live team spool are never touched.

## Linux worker admission

`linux-exec.py` checks digest rejection, memfd seals, exact executable identity at
a ptrace exec stop, and an attempted SIGCONT before admission. The expected digest
comes from the fixture itself; this does **not** validate a signed release manifest.
`linux-worker.py` drives the unmodified worker through the same primitives to
create/reopen a temporary encrypted office and stop/reap it when its private
control pipe closes. `linux-host-baseline.py` reproduces the current host's
macOS-only spawn refusal. These probes do not implement `org-supervise` or Linux
host support. The v2 fixture uses private inherited pipes; X-02 rev 8 proposes a
common post-admission rendezvous for launch v3, without inherited authority.
That handoff, release-manifest trust, sandboxing and lifecycle still need review/tests.

The marker executable is built from `exec-fixture.c`; system utilities may
intentionally reject invocation from a memfd (Ubuntu's coreutils did so).
The primitive API floor is Linux 3.19 for execveat (memfd is Linux 3.17).
The probe uses Python/glibc fexecve and requires procfs/ptrace for its observations.
Executable memfds must be permitted by the unit/kernel policy. These API floors
are not a supported-distribution claim.

## Linux credentials

`linux-credentials.py` runs as an unprivileged user with a systemd user manager.
It tests a host-key encrypted, user-scoped synthetic credential, separate service
starts, and wrong-name/different-UID/missing/corrupt/null-envelope refusal. Isolated
mount namespaces hide or replace the provider key without changing the machine's
real key; decryption must fail without changing the ciphertext, and recover with
the legitimate provider. It also
shows that the same UID can decrypt the credential: v1 accepts that threat limit.
It needs passwordless sudo for the different-UID, isolated provider and synthetic
null-envelope controls. Positive controls verify nobody's own decrypt path and
root's --uid path. Production does not inherit this testing requirement.
The damaged-service control also reads only its unique current-boot unit's
journal and requires corruption diagnostics as well as exit 243. A provider
outage with the same exit code must fail this control.

The credential format needs systemd 256 for `--user`; explicit `--refuse-null`
needs systemd 259. This exact probe therefore requires 259. The workflow initializes
the disposable runner's root-owned systemd host key and enables lingering only
for its disposable runner user. A production provider must report missing or
unavailable unlock prerequisites and must never fall back to plaintext storage.

## SQLCipher crypto

`build-crypto.sh` compares the existing `bundled-sqlcipher` feature with
`bundled-sqlcipher-vendored-openssl`, pinned to rusqlite 0.41.1. It checks dynamic
dependencies and encrypted-fixture interchange in both directions. Missing and
wrong keys fail without changing the fixture. The static comparison concerns
libcrypto, not a fully static libc binary. Expected SQLCipher authentication-error
lines arise from the wrong-key control and contain no credentials.

## macOS helper

`run-macos.sh` records compile/sign/run commands for `mac-keychain.c`, which uses
only a unique synthetic service/access group. Ad hoc code
without the entitlement gets `errSecMissingEntitlement`; code claiming an
unauthorized restricted entitlement can be terminated by macOS. No Developer ID
identity or provisioning profile was available for production acceptance.
Package the real host in a minimal app-like structure with its profile and its
own Keychain group, as Apple's daemon guidance requires; the gateway and CLI
must not receive that entitlement. Lock/unlock, upgrade and wrong-helper tests
remain a release gate.

## Evidence

The workflow uploads logs as `b2-04-linux-x86-64-evidence`, including environment,
builds, worker tests, each probe and the linkage comparison. Its token is read-only
and checkout does not persist credentials. Local ARM64 Fedora CoreOS results and
the final O-4 decision note live in Sage's agent knowledge/workspace folders.

The manual Linux entrypoint refuses to make service/provider changes unless
`B204_DISPOSABLE_VM=1`. Run Python probes with `python3 -I`; each also refuses
optimized execution so its assertions cannot silently disappear.
