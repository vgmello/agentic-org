# ADR-0009 · Authenticated complete release bundles

Status: accepted. Implementation pending. Setup uses this verifier before it
provisions anything ([standalone design](../design/standalone-server.md)). Process
admission per platform is owned by [ADR-0017](0017-linux-and-container-hosting.md);
web confinement on macOS by [ADR-0008](0008-gateway-macos-sandbox.md).

## Requirement

A complete release is authenticated before setup provisions anything. The four
executables and the static UI are bound together by one publisher signature. A bare
downloaded checksum is not enough, and running-image admission does not
authenticate the static assets. The installer's trust never comes from its own
input bundle: release owners supply public verification keys at build time and
keep private signing keys outside the application. A complete activated release
also fits the web process's bounded preloader.

## Format: profile `agentic-org-release-v1`

An exact-byte Ed25519 signature over a bounded UTF-8 `release.v1.json`. The sidecar
`release.v1.sig` contains exactly 64 raw bytes. The installer parses one strict
object, rejecting duplicate and unknown structural keys before any semantic use,
verifies the signature with a build-pinned key using `ring`, and never reserializes
the manifest to decide what was signed.

Manifest fields: `format` (= `agentic-org-release-v1`), `release_id`, positive
integer `sequence`, `platform` (`macos` or `linux`), `architecture` (`aarch64` or
`x86_64`), `installer_contract` (= 1), `key_id` and `files`. `key_id` is the
lowercase full SHA-256 of the 32-byte Ed25519 public key. A release ID has 1 to 96
ASCII letters, digits, dots, underscores or hyphens, starts with an alphanumeric,
and is not `.` or `..`. Each file entry has exactly `path`, `size`, `sha256`,
`role` and `macos_cdhash`. `macos_cdhash` is the 40-lowercase-hex Darwin
CodeDirectory unique hash of a macOS executable, produced after platform signing
from that exact payload, and null for Linux and data entries. `size` is a
non-negative integer, `sha256` is 64 lowercase hex, `role` is `executable` or
`data`. Floats, coercion and duplicate or case-colliding normalized paths are
refused.

The payload inventory is closed: exactly the four executables `agentic-org`,
`agentic-org-office`, `agentic-org-web` and `agent-os`, plus non-empty static
assets under `ui/` including `ui/index.html`. Manifest and signature are control
files outside the signed inventory. Data exists only under `ui/`; the executable
role is limited to the four names. Extra, missing, repeated or prefix-colliding
entries and unsupported roles are refused. Parent directories are inferred, never
payloads. Before activation every executable header matches the selected OS and
CPU; platform signing and identity checks stay mandatory. Verification never runs a
bundle binary.

Bounds: manifest ≤ 2 MiB, files ≤ 16384, relative path ≤ 1024 UTF-8 bytes, payload
≤ 2 GiB in total, each executable ≤ 512 MiB, each UI asset ≤ 64 MiB. Empty, `.` or
`..` components, leading or trailing or repeated slashes, backslashes, NUL and
control characters, absolute and drive paths and case-insensitive collisions are
refused.

The UI is bounded further: all `ui/` payload bytes total at most 64 MiB
(67,108,864 bytes), and its loader-entry count is at most 4096. That count is every
declared UI file plus each distinct directory prefix inferred under `ui/`, not
counting `ui/` itself; `ui/index.html` and `ui/_app/a.js` count as three entries.
Excess is refused before staging, publication, web allocation or preload. The pure
`org-release` verifier owns and exports these constants; host and web receive the
same validated contract, and the web loader also enforces actual byte and entry
counts itself. A larger supported package needs a new profile.

## Inputs: local directory and `tar.gz`

A complete local directory and `setup --bundle release.tar.gz --offline` go
through the same manifest, key and payload verification. Online acquisition is a
transport feeding the same verifier; offline setup makes no network, provider, Git
or model probe. Archive reading uses pinned `tar` and `flate2` (Rust backend) as
entry readers only; there is no hand-written archive parser and no generic unpack.

**`tar.gz`.** The bounded compressed input is copied to an owner-only temporary file
through no-follow verified descriptors and read from that held descriptor. A first
pass reads bounded metadata and the entry inventory without creating any path.
Links, devices, FIFOs, sparse files, duplicate members, aliases and unsupported
extension records are refused; only deterministic USTAR regular entries with exact
normalized UTF-8 names are accepted. Compressed bytes (2 GiB), decompressed bytes,
entry count and metadata are bounded. After verifying manifest, signature and the
complete inventory, the same held input is reparsed for the payload copy.
Truncated or corrupt gzip, extra gzip members, non-padding trailing data and hidden
entries after the tar end are refused; the reader accounts for the whole physical
container.

**Directory.** The walk uses no-follow directory handles and refuses symlinks,
non-regular payloads, hard links (`nlink != 1`), files that are unowned or group- or
world-writable, and unsafe parents. Each copied file's size and hash are verified,
and the opened descriptor's identity and metadata are compared before and after the
read. Input that changes is refused. No input path becomes a custody, instance or
organization path.

## Publication, pinning and crashes

`crates/org-release` holds the public manifest, signature and inventory policy. It
depends only on `ring`, `serde`, `serde_json`, `sha2` and the standard library,
with no internal outgoing edges; `org-host` and `agentic-org-web` consume it. Archive
staging, pinning and process admission stay in `org-host`; `tar` and `flate2` are
its private dependencies. Publisher trust is not in `org-pki`: the organization CA
is a different authority.

Staging is a new owner-only tree in the release store, written through held
no-follow directory handles with create-new, holding only manifest-authorized
regular files. Archive ownership, modes, extended attributes and timestamps are not
restored. Executables are read and execute, data is read-only, and the completed
tree is immutable. Written bytes are hashed and counted, file data and directory
entries are flushed (with `F_FULLFSYNC` on macOS and a parent-directory flush), and
platform image and architecture checks run on the staged bytes. Staging and the
final release are on the same filesystem. The tree is published by a no-replace
atomic directory rename, then a parent flush. A collision accepts only an identical,
fully re-verified release; the same `release_id` with different bytes is refused.
An installed release is never replaced in place.

Only after complete durable publication does the protected release-pin journal
select the exact `release_id`, manifest SHA-256 and sequence. A per-release-store
lock serializes publication and pin updates. A durable intent makes crashes before
or after the rename or pin recoverable: startup reconciles to the previous verified
pin or the exact new verified release. Missing or corrupt material refuses; there is
no mixed-version fallback and no activation of an orphan. Setup binds bootstrap and
resume to the pin and refuses an unexpected release change before touching
credentials or state. Garbage collection and automatic rollback are not part of this
decision.

## Runtime binding, including the static UI

Before setup or serve opens custody, the current release is verified against the
pinned manifest, and the loaded `agentic-org` image is bound to its signed entry.

- **macOS.** Every admitted worker and web process keeps the audit-token, Developer
  ID, team, identifier, hardening and entitlement checks, and its kernel-reported
  CodeDirectory hash must equal the signed `macos_cdhash` before any secret or
  right. The self-image hash comes from `csops(CS_OPS_CDHASH)`; a peer's from
  `csops_audittoken` with its authenticated audit token, refusing stale process
  incarnations. Disk-derived signing information never substitutes for the loaded
  identity. Another signed version from the same publisher is not interchangeable.
- **Linux.** The loaded image is bound through the kernel-held executable
  descriptor and its content digest from the manifest, never a mutable sibling path
  (ADR-0017).

The web process independently verifies the build-pinned manifest and pin and binds
its own loaded image before preloading. It preloads every allowed static asset into
memory against its signed size and SHA-256 and the exact UI inventory, computes the
content security policy from those bytes, and touches no filesystem after
confinement. A tampered asset, manifest or web image or a mixed release is refused
before secret handoff and listener readiness. The web process imports only the
`org-release` verifier. Debug fixture exemptions are named and debug-only; release
builds cannot opt out.

## Trusted keys and rotation

Release builds receive a strict, versioned public-key registry as a build input,
never from a runtime flag, the environment or the bundle. Version 1 entries bind
`key_id`, the 32-byte public key and an inclusive allowed sequence interval;
duplicate IDs or keys, malformed values and empty intervals fail the build. A
compiled minimum release sequence is the freshness floor. A development build
without a production registry compiles, but release activation refuses before
provisioning; it never generates a substitute key.

The publisher generates and keeps the signing key in release infrastructure and
gives the trusted build only the public key and its interval. Test keys are
generated in disposable roots and accepted only by test or debug verifier
configuration; release builds have no runtime trust override.

Rotation goes through a new trusted installer build: add the successor key and
interval before switching, then remove or restrict the retired key in a later
build. A manifest cannot add or remove keys or authorize its own signer. Emergency
revocation needs a new trusted installer; an old offline installer cannot learn of
later revocations. The highest activated sequence is stored per release store and a
lower new release is never activated. Retrying the exact same ID and digest is
idempotent. An offline installation enforces only its compiled floor and key
intervals.

## Rejected alternatives

Unauthenticated hashes; platform executable signatures alone, which omit static
assets; reusing the organization CA; an updater service, TUF or a transparency
service, which add scope without solving offline root distribution; a
directory-only verifier, which would leave the documented archive command
unsupported; generic archive unpack, which gives attacker-controlled paths and
metadata too much authority.

## Acceptance

- A valid exact bundle installs. A forged, unknown or self-provided key, an altered
  manifest or asset, duplicate, escaped-duplicate or unknown fields, a bad size,
  hash, role, CPU or platform, missing, extra or case-colliding files, hostile
  archive links, traversal, extensions, duplicates, trailing members and
  decompression bombs, and changing input or symlink races are each refused.
- UI limits at exactly the bound and one over, for bytes and for loader entries,
  including directory prefixes shared by several files, directories at several
  depths, and a 4096-file UI whose inferred directories exceed 4096. The host
  refuses before publication, pin or custody; the web process refuses before
  allocation or secrets. Whole-release and per-file limits fail as before.
- Faults before and after file flush, rename and pin publication leave either the
  previous complete pin or the exact recoverable new release, with no protected
  provisioning before verification.
- Kernel image-binding negative cases on each platform.
- The documented offline directory and archive setup, temporary service
  registration and the real built static UI in a browser.
- Generated-key fixtures prove verifier behaviour only, not real publisher signing.
