# Canonical Secrets Map

This document is the canonical inventory of every cryptographic key, secret, credential, token, verifier, and sensitive local store in **Agentic Org**. Every path, filename, key domain, cipher, holder, reader, sandbox mask, and redaction boundary below is grounded directly in the live Rust implementation across `crates/org-pki`, `crates/org-host`, `crates/org-office`, `crates/org-office-sessions`, `crates/org-office-server`, `crates/org-agent-cli`, and `apps/agentic-org-web`.

> **Core invariants:**
> 1. **Plaintext secret separation:** No wrapped Org CA key, mTLS private key, provider credential, session token, or browser TLS private key enters Org Git (`durable/state/`), the acceptance journal (`durable/recovery/`), `office.db`, snapshots, or state exports.
> 2. **Per-office master key custody:** The host (`agentic-org serve` / `org-host`) stores each office's 32-byte `OfficeSecretsKey` in OS-native custody and delivers it only over an authenticated Unix launch rendezvous to the verified worker process; workers hold it in RAM only and never persist it to disk.
> 3. **Domain separation:** Office PKI wrapping keys and provider-vault wrapping keys are cryptographically separated via HKDF-SHA256 / distinct AEAD domain strings and AAD bindings.
> 4. **Defense in depth:** Every secret path is masked from agent sandboxes (`OfficeMasks`, `bwrap` pre-spawn verification, `agent-os` file guard), and live secret values are scrubbed before logs, transcripts, wake queues, and SSE streams.

---

## 1. Per-Office Master Key (`OfficeSecretsKey`) & Launch Handoff

| Attribute | Implementation |
| --- | --- |
| **Material** | 32-byte random symmetric key (`org_proto::OfficeSecretsKey`, zeroized on drop). |
| **Owner / Custody Adapter** | `crates/org-host/src/keys/mod.rs` (`OfficeSecretsKeys` trait) and `crates/org-host/src/keys/custody.rs`. |
| **macOS (release)** | Data Protection Keychain (`service = org.agentic.office-key.instance.<instance-id>` or `org.agentic.office-key`, `account = <office-id>`, raw 32 bytes, access group `ORG_HOST_KEYCHAIN_GROUP`; requires hardened runtime and rejects injectable entitlements). |
| **Linux (release, or debug when setup records a credential dir)** | `<credential-dir>/office-keys/<office-id>.key` (`0600` inside `0700` dir, raw 32 bytes, exclusive lock, verified mount point on ext4/XFS/Btrfs/F2FS with `.instance` binding marker; `crates/org-host/src/credential_store_root.rs`). |
| **Development (debug without credential dir)** | `<ORG_OFFICE_DEV_KEY_DIR>/<instance-id>/<office-id>.key` (64 lowercase hex chars, `0600`, strictly outside office `<home>`). |
| **Unsupported OS (release)** | Fails closed (`UnsupportedKeys` -> `KeyStoreError::Unsupported`). |
| **Delivery to Worker** | Never passed via argv, env, or disk (`PASSED_ENV` in `crates/org-host/src/supervisor/child.rs` excludes dev key dirs). Sent solely over the per-launch Unix rendezvous socket `<run>/r/<16-hex>` (`0600` in `0700` dir; `crates/org-host/src/supervisor/channels.rs`) **after** peer identity verification (`SO_PEERCRED` on Linux / audit token on macOS), `ready_for_secrets`, and image re-verification (`Admission::recheck`). Host zeroes the serialized handoff buffer immediately after write and drops the key. |
| **Worker Holder** | Held in RAM only in `Office.secrets_key` (`crates/org-office/src/office/mod.rs`) and `CredentialVault` (`crates/org-office-sessions/src/credentials/vault.rs`). |
| **Deletion Rule** | Deleted by the supervisor **only** after a `--discard` worker reports `ServerEvent::Discarded`, or by orphan sweep after the 7-day grace window when permitted. |

---

## 2. Org CA Private Key, Escrow Package & Office mTLS Certificates

### 2.1 Wrapping Primitives (`crates/org-pki/src/wrap.rs`)

| Wrapping Class | Derivation | Cipher & Wire Format | AAD Domain & Binding |
| --- | --- | --- | --- |
| **Office private keys** (`WrapPurpose::OrganizationCa` = `"organization-ca"`, `WrapPurpose::OfficeTlsKey` = `"office-tls-key"`, `WrapPurpose::InvitationKey` = `"invitation-key"`) | Direct 32-byte `office_secrets_key` (`org_pki::wrap::wrap` / `unwrap`; `KeyClass::OfficePrivateKey` also defines HKDF info `b"agentic-org/wrap-key/office-private-key/v1"`). | AES-256-GCM: `version(1 byte = 0x01) \|\| nonce(12) \|\| ciphertext \|\| tag(16)`. | `b"agentic-org/office-private-key/v1"` length-prefixed with `(organization_id, office_id, purpose)`. |
| **Provider credentials** (`KeyClass::ProviderCredential`) | HKDF-SHA256 subkey (`derive_wrapping_key`): salt `b"agentic-org/hkdf-salt/v1"`, info `b"agentic-org/wrap-key/provider-credential/v1"`. | AES-256-GCM: `version(1 byte = 0x01) \|\| nonce(12) \|\| ciphertext \|\| tag(16)`. | `b"agentic-org/provider-credential/v1"` length-prefixed with `(organization_id, office_id, connection_id, "provider-credential")`. |

### 2.2 PKI Files & Holders

| Secret / Record | Canonical Path | Permissions & Scope | Protection & Binding | Writer / Reader |
| --- | --- | --- | --- | --- |
| **Org CA private key** (HQ only) | `<durable>/local/offices/<hq-office-id>/security/ca-key.v1.json` | `0600` in `0700` dirs; office-local, excluded from `durable/recovery/`, Org Git, projections, and exports. | P-256 ECDSA PKCS#8 DER (`org_pki::SigningKey`) wrapped via `org_pki::wrap::wrap` with `WrapPurpose::OrganizationCa` bound to `(org_id, hq_office_id)`. | Written by `org-office` (`office_local/mod.rs`); wrapped in `org-office-server/src/hq_creation.rs`; unwrapped in RAM by `org-office/src/office/invitations/tls.rs`. |
| **Transient CA creation staging** (HQ only) | `<durable>/local/offices/<hq-office-id>/security/ca-key.v1.staging` | `0600`; created before Entry 1 commits, promoted to `ca-key.v1.json` and removed after Entry 1 syncs. | Same wrapped `CaKey` JSON record; keeps key ciphertext out of acceptance journal segments (`TASK-10` / H-06). | `org-office/src/office/creation.rs`, `office_local/mod.rs`. |
| **Org CA public escrow package** (Git-safe) | `<durable>/state/security/ca-escrow.v1.bin` | Canonical Entry 1 file in Org Git (`durable/state/`). | Binary envelope (`crates/org-pki/src/envelope.rs`): magic `AGORGCA1`, Argon2id (`m=65536 KiB`, `t=3`, `p=4`, 32-byte key) over recovery code + 16-byte salt, AES-256-GCM (12-byte nonce), AAD `b"Agentic Org CA escrow v1\0" \|\| header`. | Sealed in `org-office-server/src/hq_creation.rs`; committed in Entry 1; read by `org-office` and `agentic-org recovery`. |
| **HQ mTLS leaf identity** (HQ only) | `<durable>/local/offices/<hq-office-id>/tls/hq-key.v1.json` (plus public `org-ca.v1.der`, `hq-cert.v1.der`, `revocations.v1.json`) | `0600` in `0700` dir; office-local. | Atomic JSON containing `epoch`, `wrapped_key_b64`, `cert_der_b64`, `san_hosts`; leaf P-256 key wrapped with `WrapPurpose::OfficeTlsKey`. | `org-office/src/office/invitations/tls.rs`; served by `org-office-server` federation listener. |
| **Satellite mTLS leaf identity** (Satellite only) | `<durable>/local/offices/<office-id>/tls/federation-identity.v1.json` (atomic commit record) + `office-key.v1.json`, `org-ca.v1.der`, `office-cert.v1.der`, `federation.v1.json` | `0600` in `0700` dir; office-local. | Leaf P-256 key wrapped with `WrapPurpose::OfficeTlsKey` bound to `(org_id, satellite_office_id)`. | `org-office/src/office/satellite/identity.rs`; loaded by `org-office-server` satellite sync client. |
| **Federation invitation state & one-time token** | HQ persists public state + invitation public key (never the secret token) at `<durable>/local/offices/<hq-office-id>/invitations/state.v1.json`. | `0600`; secret token is RAM/handoff-only. | Secret token delivered to joining satellite worker over launch rendezvous (`HandoffSecrets::Join`) and bound via RFC 5705 TLS exporter proof (`crates/org-office-server/src/federation/enrollment.rs`). | `org-office/src/office/invitations.rs`, `org-host/src/supervisor/child.rs`, `org-office-server/src/federation/enrollment.rs`. |

---

## 3. Host Database (`host-registry.db`) & Host Secrets Store

### 3.1 Host SQLite Registry (`<home>/host-registry.db`)

- **Path:** `<home>/host-registry.db` (plus `-wal` / `-shm`, mode `0600`; constant `DB = "host-registry.db"` in `crates/org-host/src/hostdb/sqlite/mod.rs`).
- **Protection:** Plain SQLite database protected by OS account boundaries and `0600`/`0700` permissions (ADR-0016; no DB encryption key). Never mounted inside agent sandboxes.
- **Security-relevant tables (`crates/org-host/src/hostdb/sqlite/{schema,sessions,links}.rs`):**
  - `accounts.password_phc` & `op_verifiers.verifier`: Pinned Argon2id PHC strings (`m=65536 KiB`, `t=3`, `p=1`, 16-byte salt, 32-byte tag; `crates/org-host/src/accounts/verifier.rs`), or the sentinel `!local-authless` (`crates/org-host/src/hostdb.rs`) in authless local mode (ADR-0034).
  - `browser_sessions`: Stores **only** 32-byte `cookie_digest` and `proof_digest` (plus `prev_cookie_digest` / `prev_proof_digest` for rotation), scoped to `(scheme, host, port, instance_id)`, 30-day idle TTL, cap 4096 rows (`crates/org-host/src/sessions.rs`). Raw 32-byte cookies (base64url) and proofs (hex) are minted and hashed inside `agentic-org-web` (`apps/agentic-org-web/src/session/cookie.rs`) using `SHA-256(b"agentic-org/browser-session/cookie/v1\0" || cookie)` and `SHA-256(b"agentic-org/browser-session/proof/v1\0" || proof)`; raw values never leave `agentic-org-web` or reach `org-host`.
  - `one_time_links`: Stores **only** 32-byte `digest = SHA-256(len(D) || D || len(I) || I || len(K) || K || token)` where `D = b"agentic-org/link/v1"`, `I = instance_id`, `K ∈ {"registration", "set_password"}`, and `token` is 32 random bytes delivered once in the URL fragment `#<base64url>` with 24-hour TTL (`crates/org-host/src/links.rs`).
  - `tls_pair`: Stores **only** the public PEM certificate chain (`cert_pem`), `source` (`made` | `supplied`), and 32-byte `pair_fp` (never the private key).

### 3.2 Host Secrets Store (`crates/org-host/src/host_secrets.rs`)

- **Backing store (max 16 KiB per item):**
  - **macOS (release):** Data Protection Keychain (`service = org.agentic.host-secret.instance.<instance-id>`, `ORG_HOST_KEYCHAIN_GROUP`).
  - **Linux (release, or debug when setup records a credential dir):** `<credential-dir>/host-secrets/<name>` (`0600` inside `0700` dir, exclusive lock, mount-verified credential store root).
  - **Development (debug without credential dir):** `<dev-base>/host-secrets/<instance-id>/<name>` (`0600`, strictly outside office `<home>`).
- **Allowed item names (`Name::parse` in `crates/org-host/src/host_secrets.rs`):**

| Item Name | Mutability | Contents & Purpose | Holder / Reader |
| --- | --- | --- | --- |
| `tls.<64-hex-pair-fp>` | Add-only | Unencrypted PEM private key for the instance's opt-in browser HTTPS certificate (ADR-0033 / `TASK-62`). Fetched by `agentic-org-web` over the authenticated `host.sock` control channel via `tls.fetch` (`crates/org-host/src/web_tls/store.rs`). | `org-host` -> `agentic-org-web` (RAM only). |
| `writer-device` | Add-only | Stable host machine identity used to bind writer custody records (`crates/org-host/src/writer.rs`). | `org-host`. |
| `writer.<64-hex>` | Read/write | Per-office schema-3 writer custody record (incarnation, epoch, confirmed journal head, organization/owner binding; `crates/org-host/src/writer.rs`). | `org-host`. |
| `recovery.<operation-uuidv7>` | Transient | Staged HQ creation/import recovery code held only until the operation settles (`crates/org-host/src/provisioning/admission.rs`). | `org-host` (sent once to worker over `HandoffSecrets::Create`). |
| `registry` | Reserved / legacy | Cleared on fresh `HostDb::create` (`crates/org-host/src/host_secrets.rs`). | `org-host`. |

---

## 4. Office Protected Databases & Office-Local Sensitive Files

### 4.1 Durable Private & Office-Local Paths (`crates/org-office/src/store/layout.rs`, `companion_paths.rs`)

| Path | Scope | Protection & Invariants | Reader / Writer |
| --- | --- | --- | --- |
| `<durable>/private/task-notes.db` (+ `-wal`/`-shm`) | **HQ only** (`ARCH-02`; never created on satellites). | `0600` in `0700` `durable/private/`; excluded from `OfficeStore` journal, Org Git, and satellite replication. Per-author private task notes (`SqlitePrivateNotesStore`). | `org-office` (`store/sqlite/private_notes.rs`). |
| `<durable>/private/user-space.db` (+ `-wal`/`-shm`) | **HQ only** (`ARCH-02`). | `0600` in `0700` `durable/private/`; human-private file space metadata (`SqliteUserSpaceStore`). | `org-office` (`store/sqlite/user_space.rs`). |
| `<durable>/private/blobs/sha256/<aa>/<bb>/<sha256>` | **HQ only** (`ARCH-02`). | `0600` content-addressed human-private attachment payloads (`FsBlobStore`). | `org-office` (`attachments/blobs.rs`). |
| `<durable>/local/offices/<office-id>/hq-owner.v1.json` | **Satellite only** (`TASK-141`). | `0600`; HQ owner member ID learned over verified mTLS `Hello`, checked on satellite open. | `org-office` (`office/satellite/`). |
| `<durable>/local/offices/<office-id>/attachment-staging/` | Office-local (`TASK-98.1`). | `0700`/`0600`; unattached uploads in flight; excluded from backups and agent sandboxes. | `org-office` (`attachments/staging.rs`). |
| `<durable>/local/offices/<office-id>/blob-outbox/` | Office-local (`TASK-98.6`). | `0700`/`0600`; pending attachment replication outbox; excluded from agent sandboxes. | `org-office` / `org-office-server`. |

### 4.2 Runtime Protected Databases (`<runtime>/<office-id>/protected/`)

All rebuildable SQLite projections and caches under `<runtime>/<office-id>/` are `0700`/`0600` and masked from every agent sandbox (`crates/org-office/src/store/layout.rs`, `crates/org-office-sessions/src/manager/office_masks.rs`):

- `<runtime>/<office-id>/office.db` (+ `-wal`/`-shm`), `git-state.json`, `git-home/`
- `<runtime>/<office-id>/protected/messaging.db` (isolated companion projection for messaging)
- `<runtime>/<office-id>/protected/accounting.db` (`L2` usage & cost projection)
- `<runtime>/<office-id>/protected/budgets.db` (`L3` budget policies & holds projection)
- `<runtime>/<office-id>/protected/providers.db` (`L4` provider bindings, discovery & permit projection)
- `<runtime>/<office-id>/protected/replication.db` (federation replication projection)
- `<runtime>/<office-id>/protected/private/task-notes-search.db` (HQ-only disposable FTS5 index for `task-notes.db`)
- `<runtime>/<office-id>/protected/blob-cache/sha256/<aa>/<bb>/<sha256>` (disposable attachment cache)

---

## 5. Provider Vault Credentials, Host OAuth Fallback & Git Remote Credentials

### 5.1 Provider Credential Vault (`crates/org-office-sessions/src/credentials/vault.rs`)

| Credential | Canonical Path | Wrapping & Binding | Runtime Injection |
| --- | --- | --- | --- |
| **Provider auth credential** (OAuth tokens or API keys) | `<durable>/local/offices/<office-id>/auth/<connection-id>/credential.v1.json` (`0600` in `0700` dir) | AES-256-GCM using HKDF-SHA256 subkey `derive_wrapping_key(office_secrets_key, KeyClass::ProviderCredential)`; AAD domain `b"agentic-org/provider-credential/v1"`, purpose `"provider-credential"`, bound to `(org_id, office_id, connection_id)`. | Unwrapped in RAM (`crates/org-office-sessions/src/router/mod.rs`) and injected into the child agent process environment (`ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, etc.) — never written to agent workspaces or transcripts. |
| **Subscription usage-API credential** | `<durable>/local/offices/<office-id>/auth/<connection-id>/usage-credential.v1.json` (`0600`) | Same `KeyClass::ProviderCredential` HKDF subkey and AES-256-GCM wrap, with `connection_id` AAD bound to `"{connection_id}:usage:{bound_origin}"`. | Unwrapped in RAM only during provider usage polling; never passed to agents. |
| **Host-local Claude OAuth fallback** | macOS Keychain service `"Claude Code-credentials"` (`crates/org-office-sessions/src/host_auth/keychain.rs`, bounded 3 s lookup, launch-scoped RAM cache) or host credentials file. | OS keychain / host account file; never copied into durable storage. | Injected into Claude child runs via `CLAUDE_CODE_OAUTH_TOKEN` alongside `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` so child subprocesses cannot inherit the token. |

### 5.2 Office Git Remote Credentials (`crates/org-office-server/src/git_calls/credentials.rs`, `TASK-168`)

| Path | Permissions & Verification | Notes |
| --- | --- | --- |
| `<office.folder()>/git-credentials/hosts/<host>/https-helper` | `0700` executable regular file (`O_NOFOLLOW`, owner-only ancestor chain verified up to `<office.folder()>`). | Origin-scoped HTTPS credential helper invoked strictly by the office server during `repo push` / Git remote ops; never accessible to agents. |
| `<office.folder()>/git-credentials/hosts/<host>/ssh-key` | `0600` regular file (`O_NOFOLLOW`, owner-only ancestor chain). | Origin-scoped SSH private key for office-mediated Git operations. |
| `<office.folder()>/git-credentials/hosts/<host>/known-hosts` | `0600` regular file (`O_NOFOLLOW`, owner-only ancestor chain). | Pinned SSH host keys (`StrictHostKeyChecking=yes`). |
| *Legacy path (never loaded)* | `<durable>/local/offices/<office-id>/security/git-{https-helper,ssh-key,known-hosts}` | Explicitly never loaded by `git_calls/credentials.rs`, and still masked from all agent sandboxes. |

---

## 6. Per-Run `AGENT_OS_SESSION_TOKEN` Bindings & External (BYOA) Auth

### 6.1 Managed Agent Run Bindings (`crates/org-office-sessions/src/binding.rs`, `manager/launch.rs`, `manager/environment.rs`)

| Artifact / State | Location | Lifetime & Verification |
| --- | --- | --- |
| **Per-run session token** | Generated as 32 random bytes (64 lowercase hex chars, `Zeroizing<String>`). Injected into child env as `AGENT_OS_SESSION_TOKEN` + `AGENT_OS_BINDING_FILE`. | Held in RAM in `RunBindingRegistry.inner` as `BindingMode::Managed { root_pid }` (plus RAM-only `acting_for_caller_id` for delegated Chief of Staff turns). Verified on every `agent-os` RPC with `subtle::ConstantTimeEq` **plus** process-lineage check `org_platform::process::is_descendant_of(peer_pid, root_pid)`. |
| **Per-run binding file** | `<durable>/local/offices/<office-id>/agents/<employee-id>/runtime/session-binding-<run-id>.v1` (`0600`). | Written for the run's own agent only; automatically deleted on teardown by `BindingGuard`. Sibling agents' `agents/<other>/` directories are masked by the sandbox. |

### 6.2 External (BYOA) Workspace Authentication (`TASK-137`)

| Side | Location / Storage | Contents & Security Controls |
| --- | --- | --- |
| **CLI client (`crates/org-agent-cli/src/features/auth/store.rs`)** | `<application-base>/agent-os/workspaces/<sha256-hex(canonical-workspace-root)>/` (`0700`, `O_NOFOLLOW`) | `key.ed25519.pkcs8` (`0600`, Ed25519 private key), `binding.v1.json` (`0600`, non-secret workspace binding metadata), `token.v1` (`0600`, cached fast token, deleted on logout). |
| **Office RAM state (`crates/org-office-sessions/src/external/mod.rs`, `binding/external.rs`)** | `PendingLogins` (RAM only), `NonceBook` (RAM only), `RunBindingRegistry.external` (RAM only) | `PendingLogins`: 10-minute TTL, 3-attempt lockout. `NonceBook`: 60-second single-use Ed25519 login challenges. Fast tokens stored as `BindingMode::External { expires_at_ms }`, verified with constant-time equality + same-UID Unix socket check. Built-in HR and Chief of Staff are unconditionally refused external sign-in. |
| **Office durable lease** | `<durable>/local/offices/<office-id>/sessions/<employee-id>/external-lease.v1.json` (`0600`) + `external-history.jsonl` | Stores the bound **public** Ed25519 key and lease metadata only (never private keys or fast tokens). `external-history.jsonl` is passed through the redactor before write. |

---

## 7. Sandbox Deny Coverage & Redaction Choke-Points

### 7.1 Filesystem & Socket Deny Boundaries

1. **Managed agent sandboxes (`crates/org-office-sessions/src/manager/office_masks.rs`, `sandbox_paths.rs`, `sandbox_rules.rs`, `bwrap_isolation.rs`):**
   - **Host & office metadata:** `<home>/offices/**`, `<office.folder()>/git-credentials/**`, privileged sockets (`host.sock`, `<run>/r/`, `<run>/t/`, D-Bus, Docker, Podman, systemd).
   - **All 10 durable families:** `<durable>/{state,journal,authority,private,messaging,budgets,accounting,recovery,blobs,checkpoints}/**` plus sibling `restore-backups/**`.
   - **Office-local secrets:** Every entry under `<durable>/local/offices/<office-id>/*` **except** `agents/` — explicitly blocking `auth/`, `tls/`, `security/`, `invitations/`, `sessions/`, `replication/`, `hq-owner.v1.json`, `attachment-staging/`, and `blob-outbox/`.
   - **Cross-entity isolation:** Sibling organizations, sibling offices, sibling agents (`agents/<other-employee-id>/`), sibling temp dirs (`<run>/t/<other>`), untargeted repository caches, and other worktree `.git` admin dirs.
   - **Runtime projections & scratch:** `<runtime>/<office-id>/{office.db*,git-state.json,protected/**,git-home/**,snapshot-import/**,snapshot-export/**}`.
   - **Host & vendor credentials:** `$CARGO_HOME/{credentials,credentials.toml}`, vendor CLI credential files, and all `agent-os` BYOA workspace state directories.
   - **Linux `bwrap` hardening:** Masked target directories are pre-created when required so bind-mounts always attach, and `verify_bwrap_masked_before_spawn` re-checks sibling/path races immediately before spawn.
2. **`agent-os` CLI file guard (`crates/org-agent-cli/src/runtime/file_guard.rs`):**
   - Independently refuses reads/writes targeting `private/task-notes.db*`, `private/user-space.db*`, `attachment-staging/`, and `blob-outbox/`.

### 7.2 Redaction Choke-Points

| Layer | Implementation | Scope & Rules |
| --- | --- | --- |
| **Session & SSE redactor** | `crates/org-office-sessions/src/redact/` (`redact_text`, `redact_json`) | Scrubs **live** secrets dynamically (`RunBindingRegistry::active_tokens()` plus active provider OAuth/API secrets) alongside static detectors: fail-closed PEM private key blocks, 25 known vendor secret prefixes, AWS `AKIA`/`ASIA` access keys, `Bearer`/`Basic` authorization values, JWTs, URL userinfo credentials, and sensitive HTTP headers, quoted JSON/YAML fields, env assignments, and query parameters. Applied before `SessionStreamHub` (SSE + live presence + `org-tess` Ask Tess SSE), transcripts, wake queues, startup/external histories, and usage diagnostics. |
| **Git subprocess redactor** | `crates/org-office-server/src/git_calls/redaction.rs` (`scrub_git_log_text`, `redact_git_error_for_log`) | Scrubs URL userinfo, auth headers, tokens, and credential paths from git stderr/error messages before logging or returning diagnostics. |
| **Host log sanitizer** | `crates/org-host/src/host_log.rs` (`log`, `untrusted`) | Emits fixed static phase/error categories only; never logs secret values, wire bytes, or raw paths; `untrusted()` caps server-supplied messages at 2 KiB and escapes control, line/paragraph separator, and bidirectional override characters. |

---

## 8. Export, Replication & Backup Exclusions

| Surface | Excluded Secrets / Stores |
| --- | --- |
| **Org Git (`durable/state/`) & state exports** | Contains only public canonical records and the passphrase-sealed `state/security/ca-escrow.v1.bin`. Never contains `ca-key.v1.json`, `tls/`, `auth/`, `git-credentials/`, `private/`, `host-registry.db`, or `OfficeSecretsKey`. |
| **Acceptance journal (`durable/recovery/`)** | Contains public state mutations and receipts only (`TASK-10` / H-06). Wrapped CA key ciphertext is staged in `local/offices/<id>/security/ca-key.v1.staging` and promoted to `ca-key.v1.json`, never written to journal segments or hashed into receipt fingerprints. |
| **Satellite replication** | Never replicates `durable/private/` (`task-notes.db`, `user-space.db`, `private/blobs/`), `durable/local/`, or HQ CA/auth material (`ARCH-02`). |
| **Local backups (`agentic-org backup`)** | Excludes host-local `OfficeSecretsKey`, `host-registry.db`, `host-secrets/`, `<office.folder()>/git-credentials/`, disposable `runtime/`, `enroll-tmp/`, `attachment-staging/`, and agent `runtime/`/`tmp/` (including per-run `session-binding-*.v1`). Optional `--encrypt` seals the archive with Argon2id (64 MiB, `t=3`, `p=4`) + AES-256-GCM. |
