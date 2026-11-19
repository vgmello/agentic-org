# F1a: pure complete-release verifier

Owner-requested separate acceptance/merge unit under
[delivery revision17](v1-incremental-delivery.md). This is an **internal foundation**,
not usable onboarding, a production release or signing setup. Root owns integration.
Reuse the existing `org-release` implementation at preservation checkpoint b77cbd68
and retain its original author context; do not reimplement it or start another broad
assignment review. Active mechanism: ADR0009 revision2, unchanged by extraction.

Exact product scope: `crates/org-release/Cargo.toml`, `README.md`,
`src/{decode.rs,lib.rs,payload.rs,tests.rs}`; only `org-release` member/default-member
additions in root `Cargo.toml` and the actually necessary generated `Cargo.lock`
delta. Include this contract, scoped assessment and review-required canonical ADR0009
with truthful implementation scope as documentation. No `org-pki`, Argon2/raw-value
feature changes, host/web/office adapters, bootstrap, task service/UI, publisher trust
provisioning, policy/guard edits or unrelated lock upgrades in this unit.

Acceptance:

1. The extracted crate builds against current `origin/v1-integration` without
   importing other unaccepted feature source. All six retained files match their
   recorded provenance, except a confirmed finding repaired by its original author.
   Public API and verification order remain unchanged from the retained implementation.
2. Exact signed-manifest bytes, trusted publisher identity, strict bounded decoding,
   signature/entry/path/digest/size/duplicate rejection and streaming payload checks
   retain ADR0009 semantics. Whole release is at most 2GiB/16,384 entries; UI is
   at most 64MiB/4,096 loader entries including proper inferred directories. No
   caller-supplied bundle root becomes a production trusted publisher key.
3. Retain all existing 19 focused tests, including exact/one-over nested UI loader
   limits and invalid signed payload cases. Run focused crate tests/Clippy/formatting
   during extraction, then required fmt/workspace Clippy/workspace tests and explicit
   kit checks for the frozen complete unit. Existing unrelated app checks are run
   only as required by the configured kit; no assignment runtime journey is implied.
4. Structural commit hook passes without policy/threshold waiver. Fresh independent
   complete-unit review, exact original-author validation and separate fresh final
   exact-commit review under the installed Codex-only policy precede root's
   `--no-ff` merge/push into `v1-integration`. No release-v1/main action.
5. Report library-only capability honestly. Generated fixture signing is test
   evidence; genuine signing setup and authentic signed-release installation/service
   verification stay final. Production verifier enforcement is not weakened.

Preserve the source reservoir, release worker and all historical receipts. Use a
new clean delivery worktree rather than reset/clean either preserved dirty worker.
The narrow source map and compile/lock delta prove extraction; past green tests do
not establish current acceptance. Root serially owns Cargo/documentation glue; the
preserved release author validates source intent and handles any confirmed library
repair. No new library writer is active until the current bounded channel handoff.
