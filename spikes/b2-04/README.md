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
host support. Production uses private inherited channels and a verified release
manifest, with sandbox and process lifecycle review still required.

The primitive API floor is Linux 3.19 for execveat (memfd is Linux 3.17).
The probe uses Python/glibc fexecve and requires procfs/ptrace for its observations.
Executable memfds must be permitted by the unit/kernel policy. These API floors
are not a supported-distribution claim.

## Linux credentials

`linux-credentials.py` runs as an unprivileged user with a systemd user manager.
It tests a host-key encrypted, user-scoped synthetic credential, separate service
starts, and wrong-name/different-UID/missing/corrupt/null-envelope refusal. It also
shows that the same UID can decrypt the credential: v1 accepts that threat limit.
It needs passwordless sudo for the different-UID and synthetic null-envelope
negative controls; production does not inherit this testing requirement.

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

`mac-keychain.c` uses only a unique synthetic service/access group. Ad hoc code
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
