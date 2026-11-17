# ADR-0007: HQ creation escrow format (REC-ESC-1), revision 1

Status: ACTIVE for implementation architecture, revision 1; not implementation acceptance. Owner: local Codex lead.
Canonical path: docs/adr/0007-hq-creation-escrow.md.

**Amended by [ADR-0016](0016-no-application-level-data-encryption.md) (2026-11-17).**
SQLCipher is removed. The usable CA private key stays in the worker's local-only PKI
record and is still committed in the one initial writer transaction, but its bytes are
stored encrypted with the host-held office secrets key instead of relying on
whole-database encryption. The host still never receives the CA key. Where the steps
below say "SQLCipher storage" or "SQLCipher writer transaction", read "the office
database, with the private key wrapped". The escrow envelope format and recovery code
are unaffected.

## Why this decision is needed

The original fullstack/architecture author (session 01a1dfb4-1d26-7100-9ea4-72b53756ef23) found a planned missing prerequisite while implementing ADR-0006 P6/P9 and plan section 4: authorities mandate Argon2id/encrypted CA escrow but no concrete envelope/KDF/code format exists. This is not evidence that task acceptance or its reviewed writer/journal architecture is invalid. Preserve ADR-0006 revision 4 and its other verified premises. Pause dependent escrow/setup completion only; authenticated selected transport and UI can continue. The original author remains implementation owner; this lead proposal answers its format question and requires independent architecture review before activation.

## Premises and authority (2026-11-15)

E1 REQUIRED: new HQ creation provisions encrypted CA escrow plus a server-generated code of at least 128 random bits, Argon2id, protected one-time delivery and idempotent retries. Sources: docs/design/standalone-server.md, CA escrow and explicit recovery gate; docs/design/offices-hq-and-satellites.md, Losing HQ and Where code lives. Verified requirement, not implemented fact.
E2 REQUIRED: pending delivery secret belongs to host credential provider; only opaque handle enters setup journal. No usable CA key/code in replicated organization data, gateway/browser/status/logs. ACK erases pending code; no later redisplay or implicit rotation. Waiting/Shown need not prevent HQ readiness. Same authorities plus docs/specs/server-installation-and-access-ux.md recovery collection. Verified requirement.
E3 REQUIRED: existing device-bound office database key custody and distinct principals remain unchanged. Setup/host never receive an office TLS identity private key. org-pki owns PKI adapter behavior; org-office domain remains certificate/network independent. REC-1 manual HQ-loss replacement and promotion remain separate release work. Same sources and ADR-0005. Verified requirement.
E4 OBSERVED: crates/org-pki is absent; RecoveryCode is only a nonempty redacted secret wrapper, launch-v3 carries a code but worker has not created escrow. No current envelope format in inspected authorities. Targeted source evidence: crates/org-proto/src/recovery.rs, crates/org-office-launch/src/v3.rs, apps/agentic-org-office/src/launch.rs; docs/design/project-structure.md. Recheck before integration.
E5 OBSERVED: maintained ring 0.17.14, argon2 0.5.3 and zeroize 1.9.1 are cached/pinned in the current workspace lock. Argon2's zeroize feature plus caller-owned Zeroizing<Vec<Block>> is necessary to wipe its large working allocation; alloc alone does not establish that. No custom cipher, KDF, elliptic-curve implementation or certificate encoder is authorized.
E6 FEASIBILITY: ignored disposable probe at escrow-design/probe uses those exact versions. It generates/imports a P-256 PKCS#8 key, derives the fixed profile, encrypts/decrypts it, restores matching public identity and verifies a signature; wrong code, ciphertext corruption and wrong AAD fail. Probe log/receipt records actual completion. It does not prove production persistence, complete envelope parsing, X.509 issuance, protected delivery, platform custody or disaster recovery. ring's opaque internal private-key/document allocations are not claimed to be fully zeroized.

## Chosen format, exact version 1

Use one new cohesive org-pki adapter crate at the already planned boundary, called by the HQ worker. It exposes generation/sealing and validation/recovery of an organization CA signing key, not enrollment/promotion orchestration. Host handles the code and existing launch contract, never receives CA PKCS#8 or office TLS private keys. Public wire crates expose bounded opaque escrow bytes/metadata only, not crypto implementations.

CA signing-key profile 1 is ECDSA P-256 with SHA-256, generated/imported using ring. PKCS#8 DER is the encrypted payload; public identity is exactly ring's 65-byte uncompressed SEC1 P-256 key (first byte 0x04). Certificates/enrollment may later bind this same key through a maintained X.509 library; do not claim that this decision ships a certificate authority service, TLS enrollment or promotion.

Recovery code: generate exactly 20 bytes with the OS CSPRNG, giving 160 random bits. Display their uppercase hexadecimal encoding as ten groups of four characters, joined with ASCII hyphens. The final group is the existing TTY challenge. Parsing accepts only ASCII hex, ASCII '-' and ASCII whitespace, with an input cap of 128 bytes; remove separators, fold ASCII case, require exactly 40 hex digits and decode exactly 20 bytes. Reject all other inputs without echoing them. KDF password bytes are the decoded 20 random bytes, never the formatted string. No fast verifier or raw code goes into persisted public metadata. This makes spelling/formatting changes independent of derived keys.

KDF profile 1: Argon2id version 0x13; m=65536 KiB, t=3 passes, p=4 lanes, 16 random salt bytes, output 32 bytes; no optional secret or associated-data input to Argon2. This is RFC 9106 section 7.4's second recommended profile, selected for bounded setup memory. It is fixed by the profile ID, not attacker-chosen numeric parameters. Use caller-owned zeroizing blocks and key/code/plaintext buffers on all application-controlled exits. No claim about library-owned opaque key allocations or resistance to an already compromised HQ process.

AEAD profile 1: ring AES-256-GCM, 12 random nonce bytes, 16-byte tag appended by the maintained API. A fresh code/salt/nonce/key is used for each new organization's initial generation. Persist and reuse the exact committed envelope on retries; never re-seal different plaintext under a retained nonce/key. Rotation is a separately reviewed operation, not an ordinary retry.

Envelope, in this exact order, with unsigned big-endian lengths:
- 8 bytes ASCII magic/version `AGORGCA1`.
- u16 organization-ID byte length, then that many original UTF-8 bytes (1..255, no normalization; must equal the expected organization identity supplied by authorized context).
- u8 suite/profile ID, exactly 1 (defines CA, KDF and AEAD profiles above).
- 16 salt bytes.
- 12 nonce bytes.
- 65 public SEC1 key bytes.
- u32 ciphertext length, 17..4112 inclusive (up to 4096 PKCS#8 bytes plus 16 tag bytes).
- ciphertext and appended tag, exact declared length; EOF immediately after.

AEAD associated data is the ASCII byte string `Agentic Org CA escrow v1` followed by one NUL byte followed by the entire serialized envelope header up to and including ciphertext length. Thus org identity, version, suite, salt, nonce, key identity and length are authenticated. Reject unknown magic/profile, bad UTF-8/identity/length, non-0x04 public key, trailing or truncated input and total package >4475 bytes BEFORE allocating KDF memory or deriving a key. After successful decrypt, import PKCS#8 using the expected P-256 signing algorithm and verify exact public-key equality before returning a secret holder. Wrong code/tamper/invalid private key yields one sanitized recovery failure. Public header fields alone are not trusted certificate or epoch authority.

## Provisioning, ownership, partial failure and retry

1. Host's protected create operation freezes organization identity and idempotency context, generates/persists the pending code in the existing credential provider first, and records only its opaque handle. Resume recovers that same code; missing/wrong code or mismatched request fails without resetting committed organization identity.
2. On HQ create only, worker receives the code via the existing protected launch-v3 handoff. org-pki generates the CA key and seals it in worker memory. Usable CA PKCS#8 is stored only in office-local encrypted SQLCipher storage, in a dedicated local-only PKI record explicitly excluded from replicated organization rows/events/export, together with the versioned public escrow record. No plaintext key files or new host key namespaces are introduced. A reviewed storage migration must mark that table local-only before replication/export consumers use it.
3. The worker commits CA key, public envelope, org identity and provisioning-complete/version atomically in its initial SQLCipher writer transaction before create success, selected capability readiness or launch-ready ACK. The public envelope subsequently projects into the organization's copied data through its owning versioned record; the usable key never does. No callback regenerates material during transaction retry: freeze generated material once per uncommitted create attempt. Crash before commit may discard that uncommitted CA; retained host code is reused. Crash after commit/reply loss must recover the existing exact CA/envelope and completion version; no rotation/re-encryption.
4. Existing committed provisioning state is verified against frozen org/request identity and the protected code, and returns its original public result. A missing side of the atomic record, unsupported version, mismatched public identity or decryption failure stops setup for forward repair. No deleting data, replacing missing custody, or accepting an escrow-free organization as complete. Never reset an existing CA to make a retry pass.
5. Setup failures before escrow commit are resumable 'organization preparation incomplete'; do not publish HQ-ready or completed organization-create success. Failure after commit resumes that same record and code. This resolves UX A89 by separating incomplete preparation from complete creation rather than inventing a valid escrow-free HQ.
6. The original worker code secret is dropped after provisioning; it must not be retained throughout serve. Pending owner delivery stays at host under its opaque handle. ACK removes it as existing authority requires. Already acknowledged organizations can restart without any recovery code, using their local CA record; absence of a pending code is not a reason to rotate or refuse a healthy existing HQ.
7. Protected recovery release offers the existing collect/defer flow and an explicit encrypted-package export to a caller-selected new file (create-new, bounded bytes; no code in that file). It verifies package identity from the worker's committed public record. The TTY tells the owner to copy both package and code off HQ before the final-group + typed-yes ACK; it does not infer external retention from a local path. Noninteractive/macOS SSH still receives no code. Status may show version/public fingerprint/export availability but never code/key/secret handle. This resolves A90 without promising backup of office database keys or work records.

The host credential provider, native TTY confirmation, one-time release/ACK state machine and worker-to-host public metadata remain existing owning responsibilities. Any missing concrete operation/migration must be implemented and checked as a prerequisite; this proposal is not evidence they already exist. Export is a local protected-owner operation, not a new browser recovery surface.

## Dependencies and alternatives

Direct org-pki dependencies: ring =0.17.14, argon2 =0.5.3 (zeroize/alloc), zeroize =1.9.1. Worker composition adds org-pki. Host/domain/protocol do not depend on org-pki. Package graph has a leaf crypto adapter with Ce=3 external library units, Ca=1 worker; no source cycle. Initial public key fixture is not a complete X.509 certificate implementation. Reassess actual manifests/lock/catalog before implementation dispatch; these are prospective edges.

Rejected: Docker/provider services (unnecessary); placeholder RecoveryCode/nonempty random string without escrow (violates E1); custom crypto or homegrown X.509 (maintenance/security); arbitrary mutable KDF parameters (resource risk); storing code alongside package/raw CA in copied rows (violates E2); changing to password-derived owner-selected code (violates E1); waiting for full multi-office recovery implementation (unnecessary for escrow creation, REC-1 remains open); assuming pending-code absence authorizes re-provisioning (violates retry/ACK).

## Required acceptance/falsification and rollback

- Exact code format, parser normalization/rejection/size cap and deterministic KDF/envelope fixtures, profile/version rejection before KDF, strict length/EOF and public-key binding.
- Round trip identical CA public identity plus signature verification; wrong code/org, changed header/key/salt/nonce/ciphertext/tag, malformed PKCS#8, truncation/oversize fail without leaking secret input.
- Real worker initial SQLCipher create/reopen and injected before/after-commit/reply-loss paths prove one CA/package, retained host code and no ordinary retry rotation. Existing acknowledged HQ restarts without code. Dedicated local-only key never appears in copied state, history, Git, status, gateway, logs or export.
- Production protected collect/defer/export/ACK/restart tests with temporary credential roots; ACK removes pending code and later release refuses redisplay, while Waiting/Shown do not block otherwise ready HQ.
- Compile/workspace checks, actual setup/adoption/task/browser acceptance and complete local code/original-author/final review remain mandatory. Passing this probe/review proves none of those.
- Before any committed creation, disable unfinished provisioning capability to roll back. After committed CA/package, preserve key/envelope/version and code handle, pause incompatible setup and forward-repair; no old executable allowed to rotate/discard them. REC-1 remains open.

## Review and challenge history

r1: proposed by lead in response to original author's concrete missing-format report. E1-E3 remain required/verified authorities. E4 is missing implementation, not an invalid requirement. E5/E6 are bounded API feasibility only. Independent ARCHITECTURE verdict pending. No claim of READY or ACTIVE.

Primary external rationale: https://www.rfc-editor.org/rfc/rfc9106.html section 7.4. Pinned library source/API in local Cargo registry was inspected; docs.rs retrieval failed, not interpreted as proof of absence. Probe uses maintained primitive APIs.

Independent ARCHITECTURE review returned **READY** on revision 1. Fresh read-only reviewer session `01a1e0c7-e87c-7f21-957e-ef51ab9c838d`; required assignment Sol/Xhigh, model/effort not independently attested by reviewer environment. Actual ordered read ACK and current preflight verified. Review findings preserve the listed production persistence, public-record/export exclusions and protected acknowledgement/deletion reconciliation obligations; none is waived. Lead activated this decision on 2026-11-15. Exact reviewed proposal SHA-256 `82add59e71e1b5b34ac06c68fbfc04e453ff4bdbbc634dfc1a0506444290e82e`; `.agent-kit/logs/v1-assignment-integration/escrow-design/proposal-r1.md` preserves its bytes. Probe/receipt and fresh result are in the same ignored evidence directory. The mechanism above is unchanged by this status/history update. REC-1, all full assignment acceptance and production tests remain open.
