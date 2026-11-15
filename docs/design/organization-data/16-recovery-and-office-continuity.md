# Recovery and office continuity data

Data specification, not an implementation of federation, automatic HQ promotion, backup automation or restore commands. This document owns the metadata required to restart/rebuild safely and links existing numbering (06), accounting (10), execution (11), and messaging (15) authorities rather than replacing them.

## What recovery means

Recovery is answering, after a crash or lost cache: “Did this command already finish? Which changes have we accepted? Is this office stale? Is the old agent process still alive?” A restored task list alone cannot answer these questions. Repeating unknown side effects can create two tasks, send two messages, deploy twice or start a second agent.

| Record | Why it exists | Existing v1 source |
| --- | --- | --- |
| `mutation_receipts` | Stable command identity, request fingerprint, bound actor, state and protected result reference; retries reuse the recorded outcome | Organization `commands`, office `work_change_results`; host operation journal remains separately protected |
| `office_origin_streams` | Per-office/incarnation high-water identity and journal head; never reuse acknowledged source sequence IDs after restore | `work_origin_counter/log`, message origin sequences and incarnation metadata |
| `office_recovery_checkpoints` | Last applied org epoch/sequence, highest known authority epoch, base digest, pending restore state | `org_replica`, `org_replica_base` |
| `recovery_snapshot_transfers` | Verified snapshot content identity and staging/install state; interruption does not masquerade as a complete install | `org_snapshot_stage/install`, snapshot artifact |
| `process_cleanup_records` | Ownership mark/generation plus machine/boot/start identity and process inventory, linked optionally to a new execution process | `owned_processes`; document 11's process instances and placement fencing remain authoritative |

## Ownership, storage and rebuilding

Organization domain records live in organization Git; these operational records do not. They can contain actor/command relationships and machine-local details. Keep source records, append journals and protected result blobs in the configured durable operational root outside disposable office databases. Owner-verified backup/restore must include these sources and the separately managed credentials. The consolidated SQL is their indexed projection, not a substitute for durable journal evidence.

```text
<organization-operational-root>/recovery/          # protected, never org Git
├── commands/<command-id>.json                    # receipt, immutable fingerprint/actor
├── results/<content-id>                          # protected command outcome
├── offices/<office-id>/
│   ├── streams/<kind>/<incarnation>/journal.jsonl # immutable accepted source changes
│   ├── streams/<kind>/<incarnation>/head.json     # watermark and digest
│   ├── checkpoint.json                          # current verified authority checkpoint
│   ├── snapshots/<snapshot-id>.json              # transfer metadata and verified content ref
│   └── cleanup/<cleanup-id>.json                 # process ownership evidence
└── snapshots/<content-sha256>                    # immutable protected snapshot content
```

Every JSON envelope has `schema_version: 1`, positive `revision`, same-named scalar fields and organization/office identity. Projection JSON columns map from objects (`inventory` → `inventory_json`); never store escaped JSON strings inside JSON source files. Journal entries retain source sequence, command/change identity, payload, prior/head digests and v1 codec/version. Keep source payload and codec where no normalized lossless mapping is defined; the projection is not permission to erase it. Use the full-envelope and platform flush guarantees already specified by document 11/ADR-0002. Do not acknowledge a mutation until its domain change, required source journal evidence and receipt/outcome have a single recoverable commit boundary. This document specifies the data obligation; an implementation must demonstrate that boundary rather than treating independent file and SQL writes as atomic.

Receipts cover shared domain commands only. Credentials, passwords, login/reset outcomes, private notes and their existence never enter these shared records: retain those in the host/private stores. `result_ref` resolves through the original command's actor/scope checks, never an unrestricted file path or public URL. `actor_id` is an employee ID only; owner/system principals are bound by the existing authentication layer, not guessed from a display name. One command ID with another fingerprint or actor conflicts. Completed/rejected outcomes remain stable. An `outcome_unknown` command is reconciled before any side effect is repeated.

The office message substrate currently shares an incarnation/sequence allocator between content and state operations; importing it must preserve that original allocator and global high-water across the two stream categories, not create independent overlapping counters. Work uses its independent source stream. New incarnation after restore never resets/reuses an old incarnation's identity. Archive old high-water records even when payload retention expires. Persist command retention/expiry deliberately; do not delete a receipt while retries may still be valid. SQL blocks deletion/replacement of retained receipts and watermarks; an eventual reviewed expiry/archive procedure must preserve equivalent retry/tombstone evidence.

A checkpoint cannot establish authority by itself. Verify organization identity, trusted HQ/CA pins, codec compatibility, contiguous event sequence and digest chain before applying/rebuilding. Host credentials and CA pins stay in existing protected custody; they are not synthesized from Git or these tables. Higher observed epochs fence stale authority; a restored old office never promotes itself. Snapshot bytes must hash-match their transfer metadata before installation; rejected/partial artifacts confer no authority. Import keeps v1 native/base/pending fields and opaque snapshot install payload where needed; SQL is not a lossy replacement for the existing binary/state-machine journal.

Process cleanup is machine-local. Never send a signal based solely on a restored PID: verify the ownership mark, generation, boot and process-start identity through the existing process-control library. A copied record on another machine is unverified. Cleanup and existing fencing must establish the previous process has stopped before the new one runs. These records complement document 11; they do not allocate a second placement generation or authorize launch.

## Loss boundaries

| What is lost | Expected result |
| --- | --- |
| Office SQLite projections/unread indexes | Rebuild from Git plus protected operational sources; rebind credentials; reconcile process ownership before execution |
| Office running process | Restart from retained receipts, stream/checkpoint and cleanup evidence; uncertain effects remain unknown until reconciled |
| Runtime cache/temp/typing/status | Recreate; stale presence expires; no mutation is replayed merely because its UI status vanished |
| Operational sources but a valid backup exists | Restore with verified identity/epoch/digests and disclose changes not covered by backup; no automatic claim of zero data loss |
| Operational sources and backups | Git restores portable definitions/open work already published, not all acknowledged messages, receipts, authority or usage; stop affected work rather than fabricate continuity |

This explicitly limits “delete office state”: the disposable directory cannot contain the only copy of acknowledged recovery evidence. Protect live source journals, host/private databases, keys, unresolved communication and process ownership until safe shutdown/archive. Recovery data is now selected; areas, Undo, automatic scheduling, memory and process-engine behavior remain unchanged.

## SQL and field relationships

All definitions below are included in `schema.sql`. All references are organization scoped. SQL enforces core identity/watermark invariants. Canonical paths, actor authorization, replay coverage, payload hashes, process verification, snapshot state transitions and filesystem/DB commit boundaries remain command/import/platform responsibilities. `generation` is the existing cleanup record generation; it is not the employee placement generation. Preserve existing IDs and generations without issuing new ones during projection rebuild.

```sql
CREATE TABLE mutation_receipts (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  command_id TEXT NOT NULL,
  origin_office_id TEXT NOT NULL,
  actor_kind TEXT NOT NULL CHECK (actor_kind IN ('employee','owner','system')),
  actor_id TEXT,
  command_kind TEXT NOT NULL,
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256)=64 AND request_sha256 NOT GLOB '*[^0-9a-f]*'),
  state TEXT NOT NULL CHECK (state IN ('reserved','linked','completed','rejected','abandoned','outcome_unknown')),
  result_ref TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  expires_at TEXT,
  PRIMARY KEY (organization_id,command_id),
  CHECK ((actor_kind='employee')=(actor_id IS NOT NULL)),
  FOREIGN KEY (organization_id,actor_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,origin_office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE office_origin_streams (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  stream_kind TEXT NOT NULL CHECK (stream_kind IN ('organization','work','message_content','message_state')),
  incarnation TEXT NOT NULL,
  high_water_sequence INTEGER NOT NULL CHECK (high_water_sequence>=0),
  journal_ref TEXT NOT NULL,
  head_sha256 TEXT CHECK (head_sha256 IS NULL OR (length(head_sha256)=64 AND head_sha256 NOT GLOB '*[^0-9a-f]*')),
  state TEXT NOT NULL CHECK (state IN ('open','sealed')),
  PRIMARY KEY (organization_id,office_id,stream_kind,incarnation),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE office_recovery_checkpoints (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  hq_office_id TEXT NOT NULL,
  authority_epoch INTEGER NOT NULL CHECK (authority_epoch>0),
  highest_seen_epoch INTEGER NOT NULL CHECK (highest_seen_epoch>=authority_epoch),
  applied_sequence INTEGER NOT NULL CHECK (applied_sequence>=0),
  digest_sha256 TEXT NOT NULL CHECK (length(digest_sha256)=64 AND digest_sha256 NOT GLOB '*[^0-9a-f]*'),
  base_sequence INTEGER NOT NULL CHECK (base_sequence>=0 AND base_sequence<=applied_sequence),
  base_digest_sha256 TEXT NOT NULL CHECK (length(base_digest_sha256)=64 AND base_digest_sha256 NOT GLOB '*[^0-9a-f]*'),
  protocol_level INTEGER NOT NULL CHECK (protocol_level>0),
  restore_pending INTEGER NOT NULL CHECK (restore_pending IN (0,1)),
  awaiting_snapshot INTEGER NOT NULL CHECK (awaiting_snapshot IN (0,1)),
  native_sequence INTEGER NOT NULL CHECK (native_sequence>=0),
  pending_base_sha256 TEXT CHECK (pending_base_sha256 IS NULL OR (length(pending_base_sha256)=64 AND pending_base_sha256 NOT GLOB '*[^0-9a-f]*')),
  journal_ref TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,office_id),
  CHECK ((authority_epoch<highest_seen_epoch)=(pending_base_sha256 IS NOT NULL)),
  CHECK (authority_epoch=highest_seen_epoch OR restore_pending=1),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,hq_office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE recovery_snapshot_transfers (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  authority_epoch INTEGER NOT NULL CHECK (authority_epoch>0),
  base_sequence INTEGER NOT NULL CHECK (base_sequence>=0),
  content_ref TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256)=64 AND content_sha256 NOT GLOB '*[^0-9a-f]*'),
  state TEXT NOT NULL CHECK (state IN ('staging','verified','installing','installed','rejected')),
  byte_count INTEGER NOT NULL CHECK (byte_count>=0),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,id),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE process_cleanup_records (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  execution_process_id TEXT,
  generation INTEGER NOT NULL CHECK (generation>0),
  ownership_mark TEXT NOT NULL CHECK (length(ownership_mark)>0),
  machine_id TEXT NOT NULL,
  boot_id TEXT,
  leader_pid INTEGER,
  leader_pgid INTEGER,
  leader_started TEXT,
  inventory_json TEXT NOT NULL CHECK (json_valid(inventory_json) AND json_type(inventory_json)='object'),
  state TEXT NOT NULL CHECK (state IN ('owned','cleanup_pending','cleaned','unverified')),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,office_id,ownership_mark),
  CHECK ((leader_pid IS NULL)=(leader_pgid IS NULL) AND (leader_pid IS NULL)=(leader_started IS NULL) AND (leader_pid IS NULL)=(boot_id IS NULL)),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,execution_process_id) REFERENCES agent_process_instances(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TRIGGER receipt_identity_immutable BEFORE UPDATE ON mutation_receipts
WHEN NEW.organization_id<>OLD.organization_id OR NEW.command_id<>OLD.command_id OR NEW.origin_office_id<>OLD.origin_office_id
 OR NEW.actor_kind<>OLD.actor_kind OR NEW.actor_id IS NOT OLD.actor_id OR NEW.command_kind<>OLD.command_kind OR NEW.request_sha256<>OLD.request_sha256
 OR (OLD.state IN ('completed','rejected','abandoned') AND (NEW.state<>OLD.state OR NEW.result_ref IS NOT OLD.result_ref))
BEGIN SELECT RAISE(ABORT,'receipt identity or terminal outcome changed'); END;
CREATE TRIGGER receipt_no_replace BEFORE INSERT ON mutation_receipts
WHEN EXISTS (SELECT 1 FROM mutation_receipts WHERE organization_id=NEW.organization_id AND command_id=NEW.command_id)
BEGIN SELECT RAISE(ABORT,'receipt command already registered'); END;
CREATE TRIGGER origin_stream_no_regression BEFORE UPDATE ON office_origin_streams
WHEN NEW.organization_id<>OLD.organization_id OR NEW.office_id<>OLD.office_id OR NEW.stream_kind<>OLD.stream_kind OR NEW.incarnation<>OLD.incarnation
 OR NEW.high_water_sequence<OLD.high_water_sequence OR (NEW.high_water_sequence=OLD.high_water_sequence AND NEW.head_sha256 IS NOT OLD.head_sha256)
 OR (OLD.state='sealed' AND (NEW.state<>OLD.state OR NEW.high_water_sequence<>OLD.high_water_sequence))
BEGIN SELECT RAISE(ABORT,'origin stream regressed or forked'); END;
CREATE TRIGGER origin_stream_no_replace BEFORE INSERT ON office_origin_streams
WHEN EXISTS (SELECT 1 FROM office_origin_streams WHERE organization_id=NEW.organization_id AND office_id=NEW.office_id AND stream_kind=NEW.stream_kind AND incarnation=NEW.incarnation)
BEGIN SELECT RAISE(ABORT,'origin identity already registered'); END;
CREATE TRIGGER checkpoint_no_regression BEFORE UPDATE ON office_recovery_checkpoints
WHEN NEW.organization_id<>OLD.organization_id OR NEW.office_id<>OLD.office_id OR NEW.highest_seen_epoch<OLD.highest_seen_epoch
 OR NEW.authority_epoch<OLD.authority_epoch OR (NEW.authority_epoch=OLD.authority_epoch AND NEW.applied_sequence<OLD.applied_sequence)
 OR (NEW.authority_epoch=OLD.authority_epoch AND NEW.applied_sequence=OLD.applied_sequence AND NEW.digest_sha256<>OLD.digest_sha256)
BEGIN SELECT RAISE(ABORT,'recovery checkpoint regressed or forked'); END;
CREATE TRIGGER checkpoint_no_replace BEFORE INSERT ON office_recovery_checkpoints
WHEN EXISTS (SELECT 1 FROM office_recovery_checkpoints WHERE organization_id=NEW.organization_id AND office_id=NEW.office_id)
BEGIN SELECT RAISE(ABORT,'checkpoint identity already registered'); END;
CREATE TRIGGER receipt_no_delete BEFORE DELETE ON mutation_receipts
BEGIN SELECT RAISE(ABORT,'receipt retention requires reviewed expiry/archive procedure'); END;
CREATE TRIGGER origin_stream_no_delete BEFORE DELETE ON office_origin_streams
BEGIN SELECT RAISE(ABORT,'origin watermark must be retained'); END;
CREATE TRIGGER checkpoint_no_delete BEFORE DELETE ON office_recovery_checkpoints
BEGIN SELECT RAISE(ABORT,'authority checkpoint must be retained'); END;
```

```mermaid
erDiagram
    organizations ||--o{ mutation_receipts : accepted_commands
    employees o|--o{ mutation_receipts : bound_actor
    offices ||--o{ mutation_receipts : origin
    offices ||--o{ office_origin_streams : owns_streams
    offices ||--o| office_recovery_checkpoints : local_replica
    offices ||--o{ office_recovery_checkpoints : hq_authority
    offices ||--o{ recovery_snapshot_transfers : receives
    offices ||--o{ process_cleanup_records : local_cleanup
    agent_process_instances o|--o{ process_cleanup_records : process_evidence
    mutation_receipts {
        TEXT organization_id PK, FK "required"
        TEXT command_id PK "required"
        TEXT origin_office_id FK "required"
        TEXT actor_kind "required"
        TEXT actor_id FK "optional"
        TEXT command_kind "required"
        TEXT request_sha256 "required"
        TEXT state "required"
        TEXT result_ref "optional"
        TEXT created_at "required"
        TEXT updated_at "required"
        TEXT expires_at "optional"
    }
    office_origin_streams {
        TEXT organization_id PK, FK "required"
        TEXT office_id PK, FK "required"
        TEXT stream_kind PK "required"
        TEXT incarnation PK "required"
        INTEGER high_water_sequence "required"
        TEXT journal_ref "required"
        TEXT head_sha256 "optional"
        TEXT state "required"
    }
    office_recovery_checkpoints {
        TEXT organization_id PK, FK "required"
        TEXT office_id PK, FK "required"
        TEXT hq_office_id FK "required"
        INTEGER authority_epoch "required"
        INTEGER highest_seen_epoch "required"
        INTEGER applied_sequence "required"
        TEXT digest_sha256 "required"
        INTEGER base_sequence "required"
        TEXT base_digest_sha256 "required"
        INTEGER protocol_level "required"
        INTEGER restore_pending "required"
        INTEGER awaiting_snapshot "required"
        INTEGER native_sequence "required"
        TEXT pending_base_sha256 "optional"
        TEXT journal_ref "required"
        TEXT updated_at "required"
    }
    recovery_snapshot_transfers {
        TEXT organization_id PK, FK "required"
        TEXT id PK "required"
        TEXT office_id FK "required"
        INTEGER authority_epoch "required"
        INTEGER base_sequence "required"
        TEXT content_ref "required"
        TEXT content_sha256 "required"
        TEXT state "required"
        INTEGER byte_count "required"
        TEXT updated_at "required"
    }
    process_cleanup_records {
        TEXT organization_id PK, FK "required"
        TEXT id PK "required"
        TEXT office_id FK "required"
        TEXT execution_process_id FK "optional"
        INTEGER generation "required"
        TEXT ownership_mark "required"
        TEXT machine_id "required"
        TEXT boot_id "optional"
        INTEGER leader_pid "optional"
        INTEGER leader_pgid "optional"
        TEXT leader_started "optional"
        TEXT inventory_json "required"
        TEXT state "required"
        TEXT updated_at "required"
    }
```

[Budgets and controls](17-budgets-and-controls.md) now selects the budget data model. Policy/extension/hold records must survive office cache loss; automatic pausing/evaluation still requires service implementation. Other deferred features remain unchanged.
