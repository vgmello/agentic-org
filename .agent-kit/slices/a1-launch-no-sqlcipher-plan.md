# A1 increment plan (planner, reviewed by the lead)

## A1 increment plan: one launch contract, no database encryption

Read-only planning only. I checked that the local `v1-integration` equals `origin/v1-integration` at 6d9b737a. F4 was read from the local branch `worktree-f4-sessions` (10613a83), which is not merged.

### 1. Outcome and finish line
- **Outcome:** nothing a person can see. The host starts offices only through the verified rendezvous, all databases are plain SQLite, and the CA private key is stored wrapped by the office secrets key (docs/implementation-plan.md:76-101; ADR-0016:43-58).
- **Interim gap, accepted by the plan:** once A1 lands, nothing on the command line can bind as a person until A5 (implementation-plan.md:231, 239). Agents still bind as members. Browser requests still use grants.
- **Finish line, all of which must hold:**
  - The host creates, starts, restarts and discards an office using only the rendezvous form, versioned as launch v1 (office-host-boundary.md:111).
  - `cargo tree -i sqlcipher-src` finds no package, and `Cargo.lock` has no sqlcipher entry.
  - ENC-1, ENC-5 and ENC-6 pass (ADR-0016:110-115).
  - The grep finish check in section 4 returns nothing.
  - FC-017 and FC-162 are updated with evidence (FEATURES.md:77, 89).

### 2. Building blocks, in order (each is a small commit with tests)

**A1.1 Host database becomes plain SQLite. Merge this to v1-integration on its own, early (see section 6).**
- Change `crates/org-host/src/hostdb.rs`:
  - `connect()` at :441-475 loses the `PRAGMA key` and `cipher_version` steps and `require_cipher`.
  - `create()` at :246-249 stops generating and `put`ting the `registry` secret.
  - `open()` at :316-332 takes no `HostSecrets` for the key.
  - Remove `Error::NoCipher` and `Error::KeyLength`.
- Delete `hostdb/sqlcipher.rs` and `hostdb/sqlcipher/tests.rs`.
- Adjust `hostdb/provisioning.rs`, `hostdb/tests.rs`, `hostdb/recovery/tests/crash.rs`, `bootstrap/tests.rs` and `provisioning/tests.rs`.
- This works while the workspace still builds SQLCipher, because a SQLCipher database opened without a key is plain SQLite.
- Tests:
  - The first 16 bytes of `host-registry.db` are `SQLite format 3\0`.
  - The file opens with a plain `rusqlite::Connection` and no key.
  - After create, `HostSecrets::list()` holds no `registry` entry.

**A1.2 Key wrapping primitive (new contract).**
- New file `crates/org-pki/src/wrap.rs` with tests. It reuses the existing `ring` AES-256-GCM, as `envelope.rs:78` and `:150` already do.
- API:
  - `WrapPurpose::{OrganizationCa}`. TLS identity and invitation are added with enrollment.
  - `WrapBinding { organization_id, office_id, purpose }`
  - `wrap(key: &[u8;32], &WrapBinding, der) -> Vec<u8>`
  - `unwrap(..) -> Zeroizing<Vec<u8>>`
- Record format: `version(1) || nonce(12, SystemRandom) || ciphertext+tag`. The authenticated data (AAD) is a domain string `agentic-org/office-private-key/v1`, then the version, then the length-prefixed purpose, organization ID and office ID.
- org-pki stays free of host and transport types: it takes raw bytes.
- ENC-6 unit tests: unwrapping fails for a different organization, office, purpose or key, and for a truncated record or a changed version byte.

**A1.3 Office store becomes plain, and the CA key is stored wrapped.**
- Keys come from the existing `db_key` handoff field, which carries the same 32-byte per-office key; it is renamed in A1.6. This keeps the launch protocol untouched in this commit.
- `crates/org-office/src/store.rs`:
  - Delete `KeyPragma` (:733-760), `sqlcipher_ready` / `initialize_sqlcipher` (:799-860) and the cipher check (:922-940).
  - The open paths at :176-209 and :463 take no key.
  - Delete `store/tests/sqlcipher_init.rs`. Adjust the `opener_*` tests and `store/work/tests/keys.rs`.
- `crates/org-office/src/error.rs`: delete `KeyMissing`.
- `crates/org-office/src/key.rs` stops being a database key. It becomes `OfficeSecretsKey` handling, an `Option` held by `Office` only for wrapping and unwrapping.
- `crates/org-office/src/store/hq_creation.rs:17,25,104,262`: the column `private_pkcs8` becomes `wrapped_ca_key`. The `CREATE TABLE` is edited in place: nothing has shipped, so there is no migration. `crates/org-office/src/hq_creation.rs:42-85` `LocalRecord` holds the wrapped bytes.
- `apps/agentic-org-office/src/hq_creation.rs`:
  - `prepare` wraps the key at :41-48 and unwraps it on retry at :33.
  - `verify_existing` at :55-65 unwraps when a key is present.
  - With the key absent, or failing to unwrap, the CA is marked unavailable and the office keeps serving (ENC-1).
  - A retry of an HQ create without a key fails as recovery-required.
- `crates/org-office/src/store/org/snapshot.rs:254-380` and `office.rs:606-628`: the replication snapshot becomes a plain SQLite file verified by a SHA-256 content digest. This replaces the one-time transport key (ADR-0016:66-69).
- Tests:
  - ENC-5: after provisioning, read `office.db` and its `-wal` with plain rusqlite and raw bytes. The PKCS#8 DER is not a substring anywhere, and the column does not parse as PKCS#8. Unwrapping with the right key gives the recorded public key.
  - ENC-1 at library level: open the office with no key. The data reads succeed, and the CA status is `unavailable`.
  - The ADR-0007 and ADR-0011 tests still pass: `apps/agentic-org-office/src/hq_creation/tests.rs`, `owned_recovery/tests.rs`, `tests/owned_recovery.rs`.

**A1.4 Host always uses the rendezvous; the v2 form is still compiled but unreachable.**
- `crates/org-host/src/lib.rs:551` and `supervisor/mod.rs:238-246`: delete `with_v3`. The broker is always built, so `Spawner.broker` stops being an `Option` (`child.rs:65`).
- `child.rs:214-232`: every create, start and discard goes through the rendezvous. This removes the "registered HQ creation requires launch v3" branch and non-HQ creates over v2.
- Port `apps/agentic-org/testing/fake_office_server.rs` to the rendezvous. Move `apps/agentic-org/tests/{supervision,create,keys,contract,binary}.rs` onto it.
- Do this after F4 merges, which touches `lib.rs`, `service/mod.rs`, `spawn.rs` and `server_check.rs`.

**A1.5 Delete v2 and `app_token`.**
- `crates/org-office-launch/src/lib.rs:509-993`: delete `HostMessage`, `CreateOffice`, `StartOffice`, `DiscardOffice`, `FirstLine`, `read_first_host_message`, `read/write_host_message`, `HostLine`, `write_host_line`, the v2 `ServerEvent` reader and writer, and `LAUNCH_VERSION = 2`.
- `v3.rs`: delete `AnyFirstLine` and `read_first_line_any` (:401-456).
- `child.rs`:
  - `Mode` becomes `Create(&CreateLine) | Office(id, Option<&Key>) | Discard(id)`.
  - Delete `CreateLine::into_token` and `drop_key`'s token half (:102-169), `write_first_line` v2 (:389-410) and stdout event reading (stdout becomes drained, untrusted).
- Delete `crates/org-host/src/app_auth.rs`. Any peer primitive that bootstrap, recovery or listener still use moves into `peer.rs`; verify those 17 references first.
- `offices.rs:212-240,769`, `service/mod.rs:551-628` (`office.app_token`), `supervisor/office.rs:44`, `secrets.rs` `generate_app_token`.
- `org-proto`:
  - `token.rs`: delete `AppToken` and `APP_TOKEN_BYTES`.
  - `calls.rs`: delete `OfficeAppToken`.
  - `msg.rs`: remove `MsgBindParams.app_token`.
  - Also update `retired.rs`, `wire.rs`, `v2_tests.rs` and `gateway/*`.
- `crates/org-client/src/lib.rs:688-717`: delete `bind_app`. Also update `org-host-control/src/secret.rs`.
- Worker:
  - `apps/agentic-org-office/src/launch.rs:47-74`: drop the V2 arms and `CreationRecovery::Legacy`.
  - `startup.rs`: one reader.
  - `serve.rs:103-140,1194-1290`: `LaunchAuth` collapses to the grant key. Delete `Binding::App` and `bind_app`. `with_app_token` is renamed `via_grant`, because grants are now the only person path (`serve/host/authority.rs:14-33`).
- Delete `apps/agentic-org/tests/app_token.rs`. Adjust the `agent-os/tests/office.rs` and `serve/tests/*` bindings.

**A1.6 One contract, named version 1, with the office secrets key.**
- Promote `v3.rs` to the crate root as `LAUNCH_VERSION = 1`. Move the tests in `v3/tests.rs`. No module named `v3` remains.
- Handoff:
  - `Create { office_secrets_key, grant_key, recovery_code? }`, with the key required.
  - `Start { office_secrets_key: Option, grant_key }`.
  - `Discard {}` carries no secret.
- Rename `org_proto::DbKey` / `DB_KEY_BYTES` to `OfficeSecretsKey` and `secrets::generate_db_key` to `generate_office_secrets_key`. The latter is also called at `hostdb.rs:246`, which is gone after A1.1.
- Host credential-provider contract: `keys::KeyStore` becomes `OfficeSecretsKeys`, defined by the consumer in `crates/org-host/src/keys/mod.rs:48`. Adapters:
  - `keychain.rs` on macOS (service `org.agentic.office-key`, `keychain.rs:45`, unchanged).
  - `dev.rs` for debug builds, in a temp directory.
  - `memory.rs` for tests.
  - A new `unsupported.rs` for non-macOS release builds. It refuses with a reported reason; Linux is implemented in X1.
  - `#[cfg(target_os)]` stays inside the adapter modules, with one non-conditional constructor (architecture-rules.md:447-469).
- Process admission already fails closed off macOS (`spawn.rs:244-254`). Keep it that way.
- Delete `key_missing` and `KeyMissing` everywhere:
  - The host status reason in `supervisor/{mod,office,state}.rs`.
  - `org-proto/src/types.rs` and `calls.rs`.
  - The worker's `lib.rs`.
  - The office's `snapshot.rs:378`.
  - The status-reasons table in office-host-boundary.md:266-277 does not list it.
- A start whose key is absent proceeds with `null`.

**A1.7 The workspace drops SQLCipher; documents are updated.**
- `Cargo.toml:69`: `features = ["bundled"]`.
- `crates/org-office/Cargo.toml:33`: remove `sqlcipher-src`.
- `clippy.toml:1` and `crates/org-supervise/clippy.toml:5`: remove the SQLCipher initialization fence (BUG-100), its disallowed-method rules and its `#[allow]`s.
- Regenerate `Cargo.lock`.
- Documents:
  - office-host-boundary.md "What the code does today" rows :386-390, and §5 code layout.
  - ADR-0016 status ("implemented").
  - FEATURES FC-017 and FC-162.
  - `docs/design/executable-migration.md`.
  - `DEVELOPMENT.md` if it mentions SQLCipher.
  - The code comments in `child.rs:51-55` and `org-pki/src/lib.rs:75`.

### 3. Acceptance criteria and how each is tested
- **ENC-1, unit:** the host and office databases begin with the plain SQLite header and open with no key. An office opened with `None` serves reads, and the CA function reports unavailable.
- **ENC-1, real process:**
  - In a temp home with a dev key directory in a temp directory, the real host supervisor runs the real `agentic-org-office` (`CARGO_BIN_EXE`).
  - Create an HQ office, wait for `ready`, kill the worker, and confirm the restart reaches `ready` with the same form.
  - Delete the office's key file from the dev store, restart, and confirm the status is `active` and the data reads succeed.
  - Admission in tests uses the existing `#[cfg(test)]` / debug `ServerCheck` path (`server_check.rs:55-103`). This needs checking at implementation time.
- **ENC-5:** the raw-byte scan of `office.db` and its WAL for the DER, as in A1.3. The host never receives the key: assert no rendezvous event or handoff field carries wrapped or plain key bytes. The HQ creation and recovery suites stay green.
- **ENC-6:** the org-pki wrap unit tests in A1.2.
- **One launch form:**
  - org-office-launch unit tests: a `"v":2` or `"v":3` first line is refused as `unsupported_version`; a missing `channel_profile` is refused before any secret (ADR-0012:36-40, 136).
  - The real-process restart above.
  - Discard receives no secret.
- **ADR-0012:** the existing channel tests (7 web plus 1 local, refusals, EOF) keep passing on the single form.
- **Key-store fakes:**
  - Unit tests use `keys::memory` and the in-memory `host_secrets`.
  - Real-process tests use `keys::dev` and `host_secrets/dev.rs` under `tempfile` roots, never inside the office home (`keys/mod.rs:190-205` enforces this).
  - No test opens the real Keychain. Confirm that `host_secrets/keychain_tests.rs` is gated or ignored.
  - No test uses a real organization root (non-negotiable 17).

### 4. What to delete, and the finish check
Delete:
- The v2 launch form and its types.
- `with_v3` and the broker `Option`.
- `AppToken`, `office.app_token`, `bind_app`, `Binding::App` and `app_auth.rs`.
- `DbKey` (renamed), `db_key` and the database-key use of `Key`.
- `KeyPragma`, both SQLCipher initialization fences and their tests.
- The `registry` host secret.
- `KeyMissing` / `key_missing`.
- The snapshot `transport_key`.
- The `private_pkcs8` column.
- `sqlcipher-src` and the `bundled-sqlcipher` feature.
- `apps/agentic-org/tests/app_token.rs`.
- The `v3` module name.

Finish check (it must print nothing, and the `cargo tree` must fail):
```
rg -n -i 'sqlcipher|PRAGMA key|cipher_version|NoCipher|bundled-sqlcipher|\bdb_key\b|DbKey|DB_KEY|app_token|AppToken|APP_TOKEN|app_auth|bind_app|HostMessage|HostLine|read_first_line_any|AnyFirstLine|with_v3|key_missing|KeyMissing|transport_key|private_pkcs8|from_db_key|org_office_launch::v3' crates apps Cargo.toml Cargo.lock clippy.toml
cargo tree -i sqlcipher-src   # expect: package ID specification did not match
test ! -e crates/org-office-launch/src/v3.rs && test ! -e crates/org-host/src/hostdb/sqlcipher.rs
```
`IdProfile` / `id_profile` is excluded on purpose: it belongs to FC-142 (office-host-boundary.md:400-403).

### 5. Decisions where the documents are silent
1. **Version and module name.** The single form is launch version 1 at the root of `org-office-launch`; no `v3` name remains (office-host-boundary.md:111 already says version 1). The code changes; no document update is needed.
2. **Discard handoff.** `--discard` gets no secret. Without SQLCipher it opens nothing that needs a key. Update office-host-boundary.md:330-331 ("loads it before every start and discard") and :131-132.
3. **Missing or unusable key.**
   - An absent key, or one that fails to unwrap, means the start proceeds with `office_secrets_key: null`. The CA is unavailable and logged without key material. It is never `storage_error`.
   - An HQ create retry without a key is recovery-required.
   - The host status `key_missing` is deleted.
   - Update ADR-0016 decision 3 (:55-58) and office-host-boundary.md:242-246.
4. **Wrap format and placement.** The primitive is `org_pki::wrap` (AES-256-GCM, record version 1, the AAD in A1.2). org-office stores opaque bytes; the worker app composes the two. Record this in an ADR-0016 decision 3 bullet and in office-host-boundary "HQ creation context" (:167-174).
5. **Snapshot transport.** A plain SQLite file plus a SHA-256 digest checked at import. This settles ADR-0016:66-69 "to be confirmed". Record it in ADR-0016 decision 5 and the replication or federation design.
6. **Credential-provider contract.** The `OfficeSecretsKeys` trait with macOS, dev, memory and unsupported adapters. Record it in office-host-boundary.md §5 (:372-375).
7. **Grant-only person path.** `Binding::Member { via_grant }` replaces the token flag, and "app-only" calls require a verified grant. Code only; ADR-0006 and ADR-0012 are unchanged.
8. **SQLCipher fence.** The SQLCipher initialization fence and its clippy rules go. B1 may add its own ENG-1 lint.
9. **Folding the web handshake into the one contract is a follow-up, not part of A1.**
   - F4 is unmerged, and its handshake lives in `org-host-control/src/gateway.rs` (`GatewayFirstLine`, `GatewayReadyForSecrets`, `GatewayHandoff`, all `"v":1`) and in `org-host/src/web.rs:394-470`.
   - With one writer per file, A1 cannot own those files before they exist on v1-integration.
   - A1's finish line covers office workers only.
   - The web process receives no secret, so the gap carries no security risk. F4 already follows the same steps: suspended spawn, `check.admit`, rendezvous, `reader_is_child` peer check, `ready_for_secrets`, then a descriptor handoff.
   - Plan: it is the last block of A3 (B13), which owns the web process, after A1.6 and F4 have merged. Move the three types into an `org_office_launch::web` module and share one host-side rendezvous and admission helper between `child.rs` and `web.rs`.
   - Record it in office-host-boundary.md "What the code does today" as an A3 row.

### 6. Conflicts and commit order
- **F4 merges first.** A1.4 to A1.6 edit `org-host/src/{lib.rs,service/mod.rs,secrets.rs,spawn.rs,server_check.rs}`, which F4 also changes. A1.1 to A1.3 do not overlap F4 and can start at once.
- **A2 and the host database.**
  - A1 owns `hostdb.rs` `connect`/`open`, the registry-secret lines of `create`, `hostdb/sqlcipher*`, `Error` and the cipher tests, until A1.1 merges.
  - A1.1 merges alone, as the first thing, a target of about one day.
  - Hand-off then: A2 owns `hostdb.rs` `create()`'s account part, `first_account.rs`, `schema.rs` (registration-link digests table) and `instance/`.
  - A2 rebases onto A1.1. If A2 has already changed `create()`, A1 hands A2 the patch for that function instead of editing it.
  - Later A1 commits do not touch `hostdb/`.
- **B1 and A4.**
  - B1 writes its plan only; its code starts after A1.3 and A1.7 merge, because the office store, `store/hq_creation.rs` and `snapshot.rs` change.
  - A4 touches HQ provisioning. It comes after A2 and A3, so A1 will have merged first.
  - B1 then rebuilds on a store with no key parameter and keeps the `wrapped_ca_key` column semantics.
- **Order:** F4, then A1.1 (merge), then A2 can start on hostdb. A1.2 and A1.3 (merge, which unblocks B1 code), then A1.4, A1.5, A1.6 and A1.7 (merge), then A3's web-fold commit.

### 7. Risks and verification
**Risks**
- Removing `app_token` is the largest part: about 377 references across org-proto, org-client, agent-os tests and worker serve. Split it as A1.4 and A1.5 so each commit compiles.
- Persons cannot bind from a terminal until A5. The plan accepts this.
- Switching rusqlite to `bundled` may change SQLite build options or the version. Compare `PRAGMA compile_options` before and after.
- A replication conformance test that depends on encryption (`store/org/replication/conformance.rs`) must be rewritten to check the digest.
- If the `app_auth` peer-check code is shared with bootstrap or recovery, it must move to `peer.rs` rather than be deleted.
- The Keychain adapter and sweep stay macOS-only. Without the `unsupported` adapter, a Linux release would fail to compile instead of refusing at run time.
- Admission of the real worker in tests depends on the debug `ServerCheck` path. Verify it before writing the real-process ENC-1 test.

**Verification commands**
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p org-pki wrap
cargo test -p org-host hostdb
cargo test -p org-office
cargo test -p org-office-launch
cargo test -p agentic-org-office --test launch --test owned_recovery
cargo test -p agentic-org --test supervision --test create --test keys --test contract
cargo tree -i sqlcipher-src ; cargo tree -e features -i rusqlite | rg -i cipher
<finish-check rg from §4>
python3 .agent-kit/runtime/kit_hooks.py check
```

### Critical files for implementation
- /Users/vgmello/projects/agentic-org-claude/crates/org-host/src/supervisor/child.rs
- /Users/vgmello/projects/agentic-org-claude/crates/org-office-launch/src/v3.rs (and lib.rs)
- /Users/vgmello/projects/agentic-org-claude/apps/agentic-org-office/src/launch.rs (and serve.rs, hq_creation.rs)
- /Users/vgmello/projects/agentic-org-claude/crates/org-office/src/store.rs (and store/hq_creation.rs, store/org/snapshot.rs)
- /Users/vgmello/projects/agentic-org-claude/crates/org-host/src/hostdb.rs (and keys/mod.rs)
- New: /Users/vgmello/projects/agentic-org-claude/crates/org-pki/src/wrap.rs
**A1.8 Retire the app-token path (added 2026-11-18).** Grants are the only path for a person to act; the app token is
deleted. Remove office.app_token, MsgBind.app_token, org-client bind_app, the worker serve.rs binding, host secrets.rs
app-token custody, org-host service/mod.rs + listener.rs references, org-host-control secret.rs, UI lib wire.ts/proto.ts/
model.ts/mock.ts and offices.spec.ts, and their tests; keep refusals of the retired field (org-proto retired.rs, vectors).
host app_auth.rs AppPolicy (peer-process admission for admin/recovery) is not a token and stays. Finish check: §4 rg prints
only classified allowed refusals. Runs after A3 B6 merged (done) and A1.7 merges; serial with A4.2 on service/mod.rs.
