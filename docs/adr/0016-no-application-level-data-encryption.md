# ADR-0016 · No application-level encryption of organization data

Status: **accepted by the owner 2026-11-17** during the architecture review (conflict
plan entry E-01). Implementation pending; decision 3 names a relocation that must be
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
   live in a plaintext database or under `durable/`. This applies to the organization
   CA private key, the TLS private key, provider credentials, the GitHub App key and
   the pending recovery code. They belong in the host's credential provider or an
   equivalent protected store. **ADR-0007 currently keeps the usable CA key inside
   SQLCipher storage; that location must be redesigned before SQLCipher is removed.**
   The recovery escrow envelope of ADR-0007 is unaffected: it remains encrypted under
   the recovery code.
4. **Stored values that grant access are digests.** Session records hold digests
   (ADR-0013) and password records hold verifiers, never the values themselves.
5. **What is removed.** Database key generation, database-key custody, key passing in
   the launch protocol, the `key_missing` failure, orphan-key handling and the debug
   key store. Writer custody, which is continuity evidence and not a key, stays.
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
| ENC-2 | No private key or credential is present in any database or file under the organization data root; a scan of a provisioned instance proves it. |
| ENC-3 | Setup reports the state of disk encryption and the file permissions it set. |
| ENC-4 | In `dedicated-user` mode ISO-1 of ADR-0015 holds, so removing encryption exposes no data to agents. |
| ENC-5 | The CA private key has a documented protected location and the HQ creation and recovery flows of ADR-0007 and ADR-0011 pass with it. |

## Consequences

- A large amount of custody machinery goes away, an unrecoverable failure mode with
  it, and Linux support is no longer blocked on a database key store.
- A second line of defence is lost in `same-user` mode, and a copied data folder is
  readable. Both are accepted.
- Because nothing is deployed, there is no data to convert. The change is made in
  code in one increment, after decision 3's relocation is designed.
