# ADR-0016 · No application-level encryption of organization data

Status: **accepted by the owner 2026-11-17** during the architecture review (conflict
plan entry E-01). Implementation pending. The owner chose the key storage of decision 3 on 2026-11-17
(option 2 in the options table below). Supersedes the SQLCipher requirement of
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
   live in plaintext in a database or in Git content. Vendor sign-in state keeps its
   selected owner-only directory, `durable/local/offices/<office-id>/auth/`
   (document 11). The web TLS key, the pending recovery code, provider credentials and
   the GitHub App key stay in the host's custody as now.
   **Private keys held by an office are wrapped by a host-held key (owner decision
   2026-11-17).** This covers the organization CA private key now, and each office's
   TLS identity key and the invitation key when satellite enrollment is built.
   - The host keeps one **office secrets key** per office in its credential provider
     and hands it to the worker at launch. This is the existing per-office key and
     handoff, kept for a smaller job; it no longer opens any database.
   - The worker stores each private key in its own store as a record encrypted and
     authenticated with that key. The record is bound to the organization, the office
     and the key's purpose, and its format is versioned.
   - The host never receives the wrapped keys, so the rules of ADR-0007 and ADR-0011
     stand. The HQ-creation commit stays one record in one transaction; only how the
     private key bytes are protected inside that record changes.
   - If the office secrets key is missing, the office still opens and serves its data.
     Only functions that need a private key are unavailable, and each key is
     recoverable: the CA from the escrow package and the recovery code, a TLS identity
     by re-enrollment, an invitation by reissue.
   The recovery escrow envelope of ADR-0007 is unaffected: it remains encrypted under
   the recovery code.
4. **Stored values that grant access are digests.** Session records hold digests
   (ADR-0013) and password records hold verifiers, never the values themselves.
5. **What is removed, and what stays.** Removed: SQLCipher and every database key
   use (the `PRAGMA key` path for the office databases and the private-notes
   database), the host registry database's own key (its `registry` secret and the
   `NoCipher` check), and the rule that a missing key prevents an office from opening
   its data. Staying, with a narrower purpose: generation and custody of one key per
   office, now the office secrets key of decision 3, and its delivery in the launch
   handoff, which also still carries the grant key and the recovery code. Staying
   unchanged: the host's secrets for TLS, writer custody and recovery (`host_secrets`),
   and writer custody itself, which is continuity evidence and not a key.
   **Ordering.** FC-162 does not wait for FC-161. The owner accepts the reduced
   protection of `same-user` mode for data; private keys stay wrapped in both modes.
6. **Backups.** A copy of the data folder is readable. Backup guidance must say to
   encrypt backups at their destination.
7. **Private notes.** Author-only access is enforced by the service, as before. A
   person with file access to the machine can read the file; this was also true of
   anyone able to obtain the database key.
8. **Linux.** A Linux host needs a credential provider for one small key per office
   and for the host's own secrets. It no longer needs one to open data.

## Options considered for decision 3 (owner chose option 2)

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

Decided 2026-11-17: option 2. It is the only one that covers all three keys in both
isolation modes without re-deciding an existing rule or changing the reviewed
HQ-creation flow.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| ENC-1 | No database is encrypted. An office opens and serves its data when its office secrets key is absent; only functions that need a private key report unavailable. |
| ENC-2 | No private key or credential is present in any database or in Git content; credential files exist only in the designated owner-only directories; a scan of a provisioned instance proves it. |
| ENC-3 | Setup reports the state of disk encryption and the file permissions it set. |
| ENC-4 | Where `dedicated-user` mode is in use, ISO-1 of ADR-0015 holds, so removing encryption exposes no source data to agents. In `same-user` mode the product states the reduced protection. |
| ENC-5 | The CA private key is stored only as a record encrypted with the office secrets key; the host never receives it; the HQ creation and recovery flows of ADR-0007 and ADR-0011 pass. A copy of the office database alone does not yield a usable private key. |
| ENC-6 | A record wrapped for one office, organization or purpose fails to open for another. |

## Consequences

- SQLCipher and the host registry key go away, a missing key no longer makes data
  unreadable, and Linux is no longer blocked on a key store to open data. The
  per-office key and its handoff remain, protecting only private keys, so less
  custody code is removed than a full removal would allow.
- A second line of defence is lost in `same-user` mode, and a copied data folder is
  readable. Both are accepted.
- Because nothing is deployed, there is no data to convert. The change is made in
  code in one increment.
