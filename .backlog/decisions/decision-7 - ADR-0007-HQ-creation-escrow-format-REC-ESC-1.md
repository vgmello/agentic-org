---
id: decision-7
title: 'ADR-0007: HQ creation escrow format (REC-ESC-1)'
date: '2026-11-21T00:43:00Z'
status: deferred
---

> **Review note (2026-11-21):** **Deferred to v2** per Owner Decision D24 (2026-11-20; tracked in `TASK-72`). Cryptographic foundation code exists in `crates/org-pki`, while end-to-end recovery and promotion flows are deferred to v2. Note `TASK-10` also removes the wrapped Org CA key from acceptance journal Entry 1 while keeping `durable/local/offices/<office-id>/security/ca-key.v1.json` as the protected local store.

# ADR-0007 · HQ creation escrow format (REC-ESC-1)

Status: accepted, **deferred to v2** (owner decision D24, 2026-11-20; see
`docs/FEATURES.md`). Foundation code exists in `crates/org-pki`, while end-to-end
recovery/promotion flows are deferred to v2. Key protection follows
[ADR-0016](0016-no-application-level-data-encryption.md); recovery-code delivery
follows [ADR-0011](0011-protected-recovery-delivery.md).

## Requirement

Creating HQ provisions an encrypted escrow of the organization CA signing key and
a server-generated recovery code of at least 128 random bits, derived with
Argon2id, delivered once through a protected path, with idempotent retries
([standalone design](../design/standalone-server.md),
[offices design](../design/offices-hq-and-satellites.md)).

- The pending code belongs to the host's credential provider; only an opaque handle
  enters the creation journal. No usable CA key or code appears in replicated
  organization data, the web process, the browser, status or logs.
- Acknowledging delivery erases the pending code. There is no later redisplay and no
  implicit rotation. A pending or shown code does not prevent HQ from being ready.
- The host never receives the CA private key or an office TLS identity private key.
  `org-pki` owns the PKI adapter; the `org-office` domain stays independent of
  certificates and the network.
- Manual replacement of a lost HQ (REC-1) and promotion are separate work.

## Chosen format, version 1

One adapter crate, `org-pki`, called by the HQ worker, generates and seals the
organization CA signing key and validates and recovers it. It does not orchestrate
enrollment or promotion. The host handles the code through the launch contract and
never receives CA PKCS#8 or office TLS private keys. Public wire crates carry
bounded opaque escrow bytes and metadata only. No custom cipher, KDF, elliptic-curve
implementation or certificate encoder is written; the maintained `ring`, `argon2`
and `zeroize` crates are used at exact pinned versions.

**CA signing-key profile 1** is ECDSA P-256 with SHA-256, generated or imported with
`ring`. The encrypted payload is PKCS#8 DER. The public identity is exactly the
65-byte uncompressed SEC1 P-256 key (first byte `0x04`). Certificates and enrollment
may later bind this key through a maintained X.509 library; this decision does not
ship a certificate authority service, TLS enrollment or promotion.

**Recovery code.** Exactly 20 bytes from the OS CSPRNG (160 random bits), displayed
as uppercase hexadecimal in ten groups of four characters joined with ASCII hyphens.
The final group is the terminal challenge. Parsing accepts only ASCII hex, `-` and
ASCII whitespace, with an input cap of 128 bytes; it removes separators, folds case,
requires exactly 40 hex digits and decodes exactly 20 bytes. Every other input is
rejected without being echoed. The KDF password is the decoded 20 bytes, never the
formatted string. No fast verifier or raw code goes into public metadata.

**KDF profile 1.** Argon2id version `0x13`; m = 65536 KiB, t = 3 passes, p = 4 lanes,
16 random salt bytes, 32 output bytes; no optional secret or associated data. This
is the second recommended profile of RFC 9106 section 7.4, chosen for bounded setup
memory. The profile ID fixes the parameters; they are never taken from input.
Caller-owned zeroizing blocks and key, code and plaintext buffers are wiped on every
application-controlled exit.

**AEAD profile 1.** `ring` AES-256-GCM, 12 random nonce bytes, 16-byte tag appended.
A fresh code, salt, nonce and key are used for each new organization. A retry reuses
the exact committed envelope and never re-seals different plaintext under a kept
nonce and key. Rotation is a separate operation, not a retry.

**Envelope**, in this order, with unsigned big-endian lengths:

- 8 bytes ASCII magic and version `AGORGCA1`.
- u16 organization-ID byte length, then that many UTF-8 bytes (1 to 255, no
  normalization; must equal the organization identity supplied by authorized
  context).
- u8 suite/profile ID, exactly 1 (the CA, KDF and AEAD profiles above).
- 16 salt bytes.
- 12 nonce bytes.
- 65 public SEC1 key bytes.
- u32 ciphertext length, 17 to 4112 inclusive (up to 4096 PKCS#8 bytes plus the
  16-byte tag).
- Ciphertext and appended tag, exactly the declared length, then end of input.

The AEAD associated data is the ASCII string `Agentic Org CA escrow v1`, one NUL
byte, and the whole serialized header up to and including the ciphertext length. So
organization identity, version, suite, salt, nonce, key identity and length are all
authenticated. Unknown magic or profile, bad UTF-8, identity or length, a public key
not starting with `0x04`, trailing or truncated input and a package over 4475 bytes
are rejected before any KDF memory is allocated. After decryption the PKCS#8 is
imported as P-256 signing and its public key must equal the header's before a
secret holder is returned. A wrong code, tampering or an invalid private key gives
one sanitized recovery failure. Header fields alone are never trusted as
certificate or epoch authority.

## Provisioning, ownership, partial failure and retry

1. The host's protected create operation freezes organization identity and the
   idempotency context, generates and stores the pending code in its credential
   provider first, and records only the opaque handle. A resume recovers the same
   code; a missing or wrong code or a mismatched request fails without resetting
   committed organization identity.
2. On HQ creation only, the worker receives the code over the launch rendezvous.
   `org-pki` generates the CA key and seals it in worker memory. The usable CA
   PKCS#8 is stored only in the HQ's protected local record
   `durable/local/offices/<office-id>/security/ca-key.v1.json`, wrapped with the
   office secrets key (ADR-0016), and excluded from organization Git, replicated
   records, events and exports; the protected acceptance journal holds it, wrapped,
   until the creation's entry is released (data contract document 16, "The HQ's CA
   records"). The versioned public escrow package is the organization's
   `state/security/ca-escrow.v1.bin`. No plaintext key file and no new host key
   namespace exist.
3. The worker commits the wrapped CA key, public envelope, organization identity and
   provisioning-complete version in one initial transaction, before creation
   success, capability readiness or the launch-ready acknowledgement. The public
   envelope is then projected into the organization's data through its owning
   record; the usable key never is. Generated material is frozen once per
   uncommitted attempt and never regenerated inside a transaction retry. A crash
   before commit may discard that CA, and the host's code is reused. A crash after
   commit or a lost reply recovers the exact CA, envelope and version; nothing is
   rotated or re-encrypted.
4. Existing committed provisioning state is verified against the frozen organization
   and request identity and the protected code, and returns its original public
   result. A missing half of the record, an unsupported version, a mismatched public
   identity or a decryption failure stops creation for forward repair. Data is never
   deleted, missing custody is never replaced, an escrow-free organization is never
   accepted as complete, and an existing CA is never reset to make a retry pass.
5. A creation failure before the escrow commit is a resumable "organization preparation
   incomplete"; HQ-ready and organization-created are not reported. A failure after
   the commit resumes the same record and code.
6. The worker drops its copy of the code after provisioning. The pending delivery
   stays at the host under its handle until acknowledged. An acknowledged
   organization restarts without any code, using its local CA record; the absence of
   a pending code is not a reason to rotate or refuse a healthy HQ.
7. Organization creation, in the browser, shows no code: the code starts Waiting.
   Protected recovery release on the HQ machine offers collect, defer and an explicit export of the
   encrypted package to a new file the caller chooses (create-new, bounded size; no
   code in the file). It verifies package identity from the worker's committed
   public record. The terminal tells the owner to copy both package and code off HQ
   before the final-group and typed-yes acknowledgement. Non-interactive sessions
   and macOS SSH receive no code. Status may show version, public fingerprint and
   export availability, never the code, key or secret handle. Export is a local
   protected-owner operation, not a browser surface.

## Dependencies

`org-pki` depends directly on `ring`, `argon2` (with `zeroize` and `alloc`) and
`zeroize`, at exact versions. The office worker composes `org-pki`. Host, domain and
protocol crates do not depend on it. There is no source cycle.

## Rejected alternatives

A placeholder code without escrow; custom cryptography or a home-grown X.509
encoder; mutable KDF parameters; storing the code beside the package or the raw CA
in copied rows; an owner-chosen password instead of a generated code; treating the
absence of a pending code as permission to re-provision.

## Acceptance

- Exact code format, parser normalization, rejection and size cap; deterministic
  KDF and envelope fixtures; profile and version rejection before the KDF; strict
  length and end of input; public-key binding.
- A round trip restores the identical CA public identity and verifies a signature.
  A wrong code or organization, a changed header, key, salt, nonce, ciphertext or
  tag, malformed PKCS#8, truncation and oversize all fail without leaking the input.
- A real worker creates and reopens HQ, and injected failures before commit, after
  commit and on reply loss prove one CA and package, the host's code kept, and no
  rotation on an ordinary retry. An acknowledged HQ restarts without a code. The
  local-only key never appears in copied state, history, Git, status, web, logs or
  export. No office database holds the key, wrapped or not (the projection records
  only the key record's hash): the wrapped key is only in the HQ's protected local
  record `durable/local/offices/<office-id>/security/ca-key.v1.json` and, until the
  creation's entry is released, in the protected acceptance journal, so a copy of
  either without the office secrets key yields no usable key (ENC-5).
- Protected collect, defer, export, acknowledge and restart run with temporary
  credential roots. Acknowledgement removes the pending code and a later release
  refuses redisplay; waiting or shown states do not block an otherwise ready HQ.
- Before any committed creation, rollback disables provisioning. After a committed
  CA and package, the key, envelope, version and code handle are preserved,
  incompatible creation pauses, and repair moves forward; no executable may rotate or
  discard them.
