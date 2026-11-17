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

## Options for decision 3 (awaiting the owner)

What exists today. Only the organization CA private key exists in code. It is the
`private_pkcs8` column of the immutable `local_hq_creation` record in `office.db`,
beside the public key and the escrow package. Nothing uses it to sign yet: its first
use is issuing office certificates when satellite enrollment is built. The office TLS
identity key and the invitation key are part of that unbuilt enrollment design. A
satellite's TLS identity key must be usable unattended, every time the office starts.

| Option | How it works | What it keeps | What it costs |
| --- | --- | --- | --- |
| 1. Sealed only | Store no usable CA key. Keep only the escrow package, which is already encrypted under the recovery code. When a CA signature is needed, the owner supplies the recovery code and the worker unlocks the key in memory for that operation. | The least machinery. All database-key custody goes now. Nothing usable at rest. The host still never receives the key. | The recovery code becomes a working credential, needed for every enrollment and every certificate renewal, which ADR-0011 treats as delivered once and kept offline. Renewal cannot be automatic. It does not answer the unattended TLS identity key, so the question returns with satellites. Changes the reviewed HQ-creation record. |
| 2. Wrapped by a host-held key | The worker keeps the three keys in its own store, each encrypted with one key per office that the host holds in its credential provider and hands over at launch, the same handoff that carries the database key today. The databases themselves are plain. | Protection equal to today's in every isolation mode. The host never receives the keys, so ADR-0007 and ADR-0011 stand. The HQ-creation commit stays one record in one transaction. Unattended use and automatic renewal work. | A host-held key per office and its custody code remain, in a smaller role, so less is removed than hoped. If that key is lost, data is unaffected and only the three keys are, each recoverable: the CA from escrow and the code, TLS by re-enrollment, the invitation by reissue. Linux still needs a place for one key per office. |
| 3. Host holds the keys | The three keys go into the host's existing credential store, which already holds the web TLS key and the pending recovery code. The host hands them to the worker at launch. | One existing store. No wrapping code in the worker. Database-key custody is fully removed. | Re-decides the rule that the host never receives the CA or office TLS private keys. HQ creation must write to two stores atomically, a new two-step protocol in a flow that was just reviewed. The host handles more secrets. |
| 4. Owner-only files | Plain key files with owner-only permissions under `durable/local/offices/<office-id>/keys/`. | The simplest storage and no custody at all. | In `same-user` mode an agent that escapes its sandbox can read the CA key and issue office certificates. Copies and backups carry the key. ADR-0007 forbids plaintext key files. Only defensible with `dedicated-user`. |

Recommendation: option 2. It is the only one that covers all three keys in both
isolation modes without re-deciding an existing rule or changing the reviewed
HQ-creation flow. A staged variant is possible if the most removal now matters more:
option 1 for the CA key while only HQ exists, and option 2 when the first unattended
key appears with satellites.

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
