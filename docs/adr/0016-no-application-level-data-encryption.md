# ADR-0016 · No application-level encryption of organization data

Status: **accepted by the owner 2026-11-17** during the architecture review (conflict
plan entry E-01). Implementation pending; decision 3 names relocations that must be
designed first. Supersedes the SQLCipher requirement of
[ADR-0002](0002-storage-sqlite-sqlcipher-one-writer.md) decision 1 and the
database-key custody of its decision 2. ADR-0002's choice of SQLite, its single writer
and WAL stand, and its durability rule stands as amended by
[ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md).

## Context

- ADR-0002 encrypts every office database with SQLCipher and makes the host the
  keeper of each database key.
- Three protections stop different readers. Disk encryption protects a machine that is
  stolen or switched off. File permissions stop other OS users. Application-level
  encryption stops a program that can open the file but does not hold the key.
- The third exists here mainly because agents run as the same OS user as the data.
  [ADR-0015](0015-agent-isolation-modes.md) addresses that directly.
- Its cost is high: generated keys per office, a signed host with a key-store
  entitlement, key passing in the launch protocol, a `key_missing` failure with no
  recovery, an undecided Linux key store, and about 46 source files that handle the
  key types.
- The selected storage contract moves most data out of the database into files under
  `durable/`. An encrypted database would then protect an index while the sources sat
  in the clear, unless every file were encrypted too.

## Decision

1. **Organization data is not encrypted by the application.** This covers the office
   databases, the host registry database, the private-notes database and every file
   under `durable/` and `runtime/`.
2. **Data at rest is protected by three things.** Operating-system disk encryption,
   which setup checks and warns about when it is off; private file permissions, owned
   by the service account; and agent isolation under ADR-0015. In `same-user` mode the
   vendor tool sandbox and the office's deny rules are the only barrier between an
   agent and the data, and the product says so.
3. **Secrets are not data and stay protected.** Private keys and credentials never
   live in a plaintext database or in Git content. Vendor sign-in state keeps its
   selected owner-only directory, `durable/local/offices/<office-id>/auth/`
   (document 11). **Three private keys live inside the encrypted office database today
   and need a new protected location before SQLCipher is removed:** the organization
   CA private key (ADR-0007), each office's TLS identity key, and the invitation key
   (offices design). Where they go is an open design question. ADR-0007, ADR-0011 and
   the offices design say the host never receives the CA or office TLS private keys,
   so either that rule is re-decided and the host's credential provider holds them, or
   the worker gets a protected key store of its own. The design must also say what
   protects those keys in `same-user` mode, where file permissions do not stop an
   agent. The web TLS key, the pending recovery code, provider credentials and the
   GitHub App key stay in the host's custody as now. The recovery escrow envelope of
   ADR-0007 is unaffected: it remains encrypted under the recovery code.
4. **Stored values that grant access are digests.** Session records hold digests
   (ADR-0013) and password records hold verifiers, never the values themselves.
5. **What is removed, and what stays.** Removed: database key generation and custody
   for the office databases and for the host registry database (its `registry` secret
   and the `NoCipher` check), database-key passing in the launch protocol, the
   `key_missing` failure, orphan database-key handling, and the debug database-key
   store (`crates/org-host/src/keys/dev.rs`). Staying: the launch handoff itself, which
   still carries the grant key and the recovery code; the host's secrets for TLS,
   writer custody and recovery (`host_secrets`); and writer custody, which is
   continuity evidence and not a key.
   **Ordering.** FC-162 does not wait for FC-161. The owner accepts the reduced
   protection of `same-user` mode. It does wait for the relocation design of
   decision 3.
6. **Backups.** A copy of the data folder is readable. Backup guidance must say to
   encrypt backups at their destination.
7. **Private notes.** Author-only access is enforced by the service, as before. A
   person with file access to the machine can read the file; this was also true of
   anyone able to obtain the database key.
8. **Linux.** The open question narrows from where database keys live to where a few
   private keys live.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| ENC-1 | No database key is generated, stored, passed at launch or required to start an office. |
| ENC-2 | No private key or credential is present in any database or in Git content; credential files exist only in the designated owner-only directories; a scan of a provisioned instance proves it. |
| ENC-3 | Setup reports the state of disk encryption and the file permissions it set. |
| ENC-4 | Where `dedicated-user` mode is in use, ISO-1 of ADR-0015 holds, so removing encryption exposes no source data to agents. In `same-user` mode the product states the reduced protection. |
| ENC-5 | The CA private key, each office TLS identity key and the invitation key have a documented protected location, and the HQ creation, enrollment and recovery flows of ADR-0007, ADR-0011 and the offices design pass with it. |

## Consequences

- A large amount of custody machinery goes away, an unrecoverable failure mode with
  it, and Linux support is no longer blocked on a database key store.
- A second line of defence is lost in `same-user` mode, and a copied data folder is
  readable. Both are accepted.
- Because nothing is deployed, there is no data to convert. The change is made in
  code in one increment, after decision 3's relocation is designed.
