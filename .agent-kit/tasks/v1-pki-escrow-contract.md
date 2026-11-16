# F3a — production CA escrow adapter

Stable delivery contract; architecture/integration owner: Codex lead. Separate internal acceptance/merge unit under plan17. Integration base `eacb505d2398fc473d43f45053522e39269434b5`; working prerequisite checkpoint `fc379304382fc17c251ff3d6572ffa74657b12c6` includes the owner's c5123221 delivery-policy amendment. Governing boundary: docs/architecture-rules.md#independent-delivery-and-dependency-boundaries and docs/independent-delivery.md.

## Outcome and dependency boundary

Generate/import an HQ-local P-256 signing key, create the exact REC-ESC-1 encrypted envelope, and recover the same key/public identity from that envelope and code. This is a production cryptographic library, an INTERNAL FOUNDATION; no HQ provisioning, recovery delivery, enrollment, promotion, usable onboarding or authentic signed release is claimed. Required mechanism and public API are unchanged from independently READY ADR0007r1. Reuse all five retained b77cbd6876d72f76cc3b7f1d5fa3c3d96f424c09 org-pki blobs. No assignment, selected-task, host, protocol, directory or F4/release consumer dependency. Full F3/U1/v1 production scope remains intact; signing setup/authentic signed-release verification last.

Dependencies: maintained exact ring0.17.14, argon2 0.5.3 with alloc/zeroize and existing password-hash, zeroize1.9.1 alloc. Review actual manifest/lock delta; no existing dependency version/checksum change. No custom crypto, KDF, certificate encoder, plaintext key files or host key access.

## Exact ownership

Preserved original fullstack author session01a1dfb4-1d26-7100-9ea4-72b53756ef23 owns ONLY these seven implementation paths in /Users/vgmello/projects/agentic-org-delivery-worker, branch codex/v1-hq-escrow:
- crates/org-pki/Cargo.toml
- crates/org-pki/src/lib.rs
- crates/org-pki/src/code.rs
- crates/org-pki/src/envelope.rs
- crates/org-pki/src/tests.rs
- Cargo.toml: only add org-pki members/default-members and existing Argon2 alloc/zeroize feature flags; no unrelated source-reservoir manifest hunks.
- Cargo.lock: only generated org-pki registration and required feature-induced dependencies, without existing package-version/checksum changes.

Lead serial metadata ownership after writer handoff: .agent-kit/skill-catalog.json; .agent-kit/tasks/v1-pki-escrow-{contract.md,assessment.json,reuse.json}; .agent-kit/tasks/v1-incremental-delivery.md and v1-state.md; docs/FEATURES.md and docs/FEATURE-AREAS.md. Preserve prior F3 umbrella contract/assessment as superseded discovery artifacts, not accepted implementation; all user-authored policy files remain unmodified and in the reviewed integration diff. Existing six source-guard files remain unchanged. No source writes outside the seven-path grant without concrete evidence and explicit lead reassignment. No writer overlap; root reads only while the author writes.

Public interface: preserve Code, SigningKey, Envelope, recover, MAX_PACKAGE_BYTES and their exact meaning. Host/domain/protocol never import org-pki. Worker composition is a later unit, not introduced here to manufacture a consumer. Dependency graph is the ADR0007 leaf adapter (three external dependencies), no source cycle.

## Early verification and acceptance

Before broadening implementation, run the actual production library boundary:
1. RUSTC_WRAPPER='' cargo check --offline --locked -p org-pki --lib
2. RUSTC_WRAPPER='' cargo test --offline --locked -p org-pki

Retain exact160-bit code format/normalization/rejection/redaction; fixed profile/envelope/vector, malformed framing/identity/profile/resource limits BEFORE KDF allocation; round-trip original DER/public identity/signature; wrong code/AAD/ciphertext and sanitized failures; application-owned secret zeroization limits. Real implementation APIs, no test-only substitute, no test or guard weakening. Independent final review inspects actual code/output against ADR0007, not merely green helpers. Tests use temporary roots and generated fixtures only; no real credentials or OS authentication prompts.

Required current fmt, workspace all-target Clippy -Dwarnings, full workspace tests, configured kit/hooks and normal staged structural commit gate. Fix confirmed findings in retained author/reviewer context. Preserve initial fresh independent review, original-author exact-tree intent validation and separate fresh final SolXhigh review. Only then merge --no-ff/push v1-integration. Never v1/main, no Ada/offline/PR wait.

Source extraction/early check success is not acceptance; primitive/library evidence is not persistence, protected delivery or physical authentication. Preserve all remaining canonical requirements, deferred owner/generation tests and original worker/browser/release contexts. Full F3 follows with reviewed narrow creation-owned PreparedV1 and live-owner/directory boundary; do not import task feature protocols into foundations.
