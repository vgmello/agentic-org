use super::*;
use crate::key::Key;
use crate::store::{Opening, Store};
use org_sync::error::{Refusal, SyncError};
use org_sync::organization::{self as sync_org, OrgOutput, PeerIdentity, PeerRole, VerifiedPeer};
use org_sync_proto::{Digest, Epoch, OfficeId, OrgId};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

const LEGACY_V1: Schema = Schema {
    version: 1,
    create: |tx| tx.execute_batch(include_str!("fixtures/schema-v1.sql")),
    upgrade: |_, _| panic!("the frozen v1 reader cannot upgrade"),
};

const LEGACY_V2: Schema = Schema {
    version: 2,
    create: |tx| {
        tx.execute_batch(include_str!("fixtures/schema-v1.sql"))?;
        tx.execute_batch(include_str!("fixtures/schema-v2-replication.sql"))
    },
    upgrade: |_, _| panic!("the frozen v2 reader cannot upgrade"),
};

type LegacyRows = Vec<(String, i64, i64, String, Option<String>)>;

fn key() -> Key {
    Key::from_bytes([23; 32])
}

fn open(path: &Path, opening: Opening, schema: &Schema) -> Store {
    Store::open(path, &key(), opening, schema).unwrap()
}

fn version(store: &Store) -> i64 {
    store
        .read(|db| {
            db.pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(OfficeError::storage)
        })
        .unwrap()
}

fn columns(store: &Store) -> Vec<(String, String, i64, Option<String>)> {
    store
        .read(|db| {
            let mut query = db
                .prepare("SELECT name, type, \"notnull\", dflt_value FROM pragma_table_info('commands') ORDER BY cid")
                .map_err(OfficeError::storage)?;
            let rows = query
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .map_err(OfficeError::storage)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(OfficeError::storage)
        })
        .unwrap()
}

fn assert_principal_columns(store: &Store) {
    let columns = columns(store);
    assert_eq!(columns.len(), 8);
    for name in ["principal_account", "principal_member", "actor_kind"] {
        let column = columns.iter().find(|column| column.0 == name).unwrap();
        assert_eq!(column.1, "TEXT", "{name}");
        assert_eq!(column.2, 0, "{name} must allow legacy NULL");
    }
}

fn legacy_rows(store: &Store) -> LegacyRows {
    store
        .read(|db| {
            let mut query = db
                .prepare("SELECT command_id, epoch, seq, expires_at, result FROM commands ORDER BY command_id")
                .map_err(OfficeError::storage)?;
            let rows = query
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
                })
                .map_err(OfficeError::storage)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(OfficeError::storage)
        })
        .unwrap()
}

fn assert_unbound(store: &Store, expected: i64) {
    let (total, bound): (i64, i64) = store
        .read(|db| {
            db.query_row(
                "SELECT count(*), coalesce(sum(principal_account IS NOT NULL OR principal_member IS NOT NULL OR actor_kind IS NOT NULL), 0) FROM commands",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(OfficeError::storage)
        })
        .unwrap();
    assert_eq!(total, expected);
    assert_eq!(bound, 0);
}

fn fingerprint_vectors() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../../org-proto/test-vectors/v2/proto-fingerprints.json"
    ))
    .unwrap()
}

fn seed_legacy(store: &Store, case: &serde_json::Value) {
    let id = case["params"]["command_id"].as_str().unwrap().to_owned();
    let hash = case["fingerprint"].as_str().unwrap().to_owned();
    let expires = time::format_epoch_ms(command_expiry_ms(ulid::time_ms(&id).unwrap()));
    let answer = serde_json::json!({
        "params_hash": hash,
        "refusal": {"code": "invalid_request", "message": "legacy remembered refusal"}
    })
    .to_string();
    store
        .write(move |db| {
            seed_genesis(db)?;
            db.execute(
                "INSERT INTO commands (command_id, epoch, seq, expires_at, result) VALUES (?1, 0, 0, ?2, ?3)",
                (&id, &expires, &answer),
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
}

fn seed_genesis(db: &Connection) -> Result<(), OfficeError> {
    // Seed the pre-replication layout directly: today's genesis log
    // writer consults org_replica, which genuinely did not exist in v1.
    for (key, value) in [
        ("organization_id", "ORG1"),
        ("organization_name", "Legacy organization"),
        ("created_at", "2026-11-08T12:00:00.000Z"),
    ] {
        db.execute("INSERT INTO meta VALUES (?1, ?2)", (key, value))
            .map_err(OfficeError::storage)?;
    }
    let changes = rules::owner_bootstrap(
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "Owner",
        "2026-11-08T12:00:00.000Z",
    )
    .unwrap();
    apply_changes(db, &changes)?;
    db.execute(
        "INSERT INTO events VALUES (1, 1, 'org.changed', 'genesis', '2026-11-08T12:00:00.000Z', ?1)",
        [json(&changes)],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

#[test]
fn fresh_v3_has_nullable_principal_columns() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir.path().join("org.db"), Opening::New, &SCHEMA);
    assert_eq!(version(&store), 3);
    assert_principal_columns(&store);
    assert_replication_tables(&store);
    seed_legacy(&store, &fingerprint_vectors()["org"][0]);
    assert_unbound(&store, 1);
}

#[test]
fn encrypted_v1_and_v2_upgrade_preserves_rows_and_reopens() {
    for legacy in [&LEGACY_V1, &LEGACY_V2] {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("org.db");
        let store = open(&path, Opening::New, legacy);
        seed_legacy(&store, &fingerprint_vectors()["org"][0]);
        let before = legacy_rows(&store);
        assert_eq!(columns(&store).len(), 5);
        drop(store);
        assert_ne!(&std::fs::read(&path).unwrap()[..16], b"SQLite format 3\0");

        let store = open(&path, Opening::Existing, &SCHEMA);
        assert_eq!(version(&store), 3, "upgrade from v{}", legacy.version);
        assert_principal_columns(&store);
        assert_replication_tables(&store);
        assert_eq!(legacy_rows(&store), before);
        assert_unbound(&store, 1);
        drop(store);

        let reopened = open(&path, Opening::Existing, &SCHEMA);
        assert_eq!(version(&reopened), 3);
        assert_eq!(legacy_rows(&reopened), before);
        assert_unbound(&reopened, 1);
    }
}

#[test]
fn failed_legacy_upgrade_rolls_back_partial_ddl() {
    for legacy in [&LEGACY_V1, &LEGACY_V2] {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("org.db");
        let store = open(&path, Opening::New, legacy);
        seed_legacy(&store, &fingerprint_vectors()["org"][0]);
        // Deliberate collision on the second ALTER, after the first one succeeded.
        store.run_sql_for_test("ALTER TABLE commands ADD COLUMN principal_member TEXT");
        let before = legacy_rows(&store);
        let before_columns = columns(&store);
        let before_schema = schema_image(&store);
        drop(store);

        match Store::open(&path, &key(), Opening::Existing, &SCHEMA) {
            Err(OfficeError::Storage(cause)) => {
                assert!(cause
                    .to_string()
                    .contains("duplicate column name: principal_member"));
            }
            _ => panic!("a failed ALTER must refuse the upgrade"),
        }
        let store = open(&path, Opening::Existing, legacy);
        assert_eq!(version(&store), legacy.version);
        assert_eq!(
            columns(&store),
            before_columns,
            "partial ALTER escaped rollback"
        );
        assert_eq!(
            schema_image(&store),
            before_schema,
            "new replication tables escaped rollback"
        );
        assert_eq!(legacy_rows(&store), before);
        store.run_sql_for_test("ALTER TABLE commands DROP COLUMN principal_member");
        drop(store);

        let reopened = open(&path, Opening::Existing, &SCHEMA);
        assert_eq!(version(&reopened), 3);
        assert_principal_columns(&reopened);
        assert_eq!(legacy_rows(&reopened), before);
        assert_unbound(&reopened, 1);
    }
}

#[test]
fn older_v2_reader_refuses_v3() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("org.db");
    let store = open(&path, Opening::New, &SCHEMA);
    seed_legacy(&store, &fingerprint_vectors()["org"][0]);
    let before = legacy_rows(&store);
    drop(store);
    match Store::open(&path, &key(), Opening::Existing, &LEGACY_V2) {
        Err(OfficeError::Storage(cause)) => {
            assert!(cause.to_string().contains("database schema 3 is newer"));
        }
        _ => panic!("the v2 reader must not open a v3 database"),
    }
    let reopened = open(&path, Opening::Existing, &SCHEMA);
    assert_eq!(legacy_rows(&reopened), before);
    assert_unbound(&reopened, 1);
}

#[test]
fn snapshot_projection_does_not_depend_on_local_schema_version() {
    let source = include_str!("snapshot.rs");
    assert!(!source.contains("SCHEMA.version"));
}

fn assert_replication_tables(store: &Store) {
    let count: i64 = store
        .read(|db| {
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('org_replica', 'org_replica_events', 'org_replica_base', 'org_snapshot_stage', 'org_snapshot_install')",
                [],
                |row| row.get(0),
            )
            .map_err(OfficeError::storage)
        })
        .unwrap();
    assert_eq!(count, 5, "the v1 upgrade must also install replication");
}

// The same typed input shape used by org-proto's frozen v1/v2 fingerprint vectors.
#[derive(serde::Deserialize, serde::Serialize)]
struct AgentInputParams {
    command_id: String,
    member_id: String,
    name: String,
    office_id: String,
    profile: org_proto::org::AgentProfileInput,
}

#[test]
fn stored_v1_agent_fingerprint_matches_both_profile_forms() {
    use sha2::{Digest as _, Sha256};
    let vectors = fingerprint_vectors();
    let cases = vectors["org"].as_array().unwrap();
    assert!(
        !cases.is_empty(),
        "stored v1 fingerprint vectors are required"
    );
    for case in cases {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("org.db");
        let legacy = open(&path, Opening::New, &LEGACY_V1);
        seed_legacy(&legacy, case);
        let before = legacy_rows(&legacy);
        drop(legacy);
        let store = open(&path, Opening::Existing, &SCHEMA);
        assert_eq!(version(&store), 3);
        let rows = legacy_rows(&store);
        assert_eq!(rows, before);
        assert_eq!(rows.len(), 1);
        let stored: Answer = serde_json::from_str(rows[0].4.as_ref().unwrap()).unwrap();
        let command = OrgCommand::parse(&org_proto::Request {
            id: 1,
            method: case["method"].as_str().unwrap().into(),
            principal: None,
            params: case["params"].clone(),
        })
        .unwrap()
        .unwrap();
        assert_eq!(fingerprint(&command), stored.params_hash);
        let input: AgentInputParams = serde_json::from_value(case["params"].clone()).unwrap();
        let serialized = serde_json::to_string(&input).unwrap();
        assert_eq!(serialized, case["serialized"].as_str().unwrap());
        let mut hash = Sha256::new();
        hash.update(case["method"].as_str().unwrap().as_bytes());
        hash.update([0]);
        hash.update(serialized.as_bytes());
        let actual: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(actual, stored.params_hash);
        assert_eq!(actual, case["fingerprint"].as_str().unwrap());
        let replay = store
            .write(move |db| submit(db, 1_800_000_000_000, &command).map(|result| (result, None)))
            .unwrap()
            .unwrap_err();
        assert_eq!(replay.message, "legacy remembered refusal");
        assert_eq!(legacy_rows(&store), before);
        assert_unbound(&store, 1);
    }
}

const CLOCK: u64 = 1_800_000_000_000;
const SNAPSHOT_LIMIT: u64 = 1024 * 1024;

fn team(store: &Store, id: &str, name: &str) -> Result<CommandAccepted, ApiError> {
    let command = OrgCommand::TeamCreate(org_proto::org::TeamCreateParams {
        command_id: id.into(),
        team_id: "TEAM1".into(),
        name: name.into(),
        owner_team_id: None,
    });
    store
        .write(move |db| submit(db, CLOCK, &command).map(|result| (result, None)))
        .unwrap()
}

#[test]
fn legacy_native_write_replay_conflict_and_rollback_keep_null_principals() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("org.db");
    let store = open(&path, Opening::New, &SCHEMA);
    seed_legacy(&store, &fingerprint_vectors()["org"][0]);
    let id = ulid::from_ms(CLOCK, 91);
    let accepted = team(&store, &id, "Engineering").unwrap();
    let before = legacy_rows(&store);
    assert_unbound(&store, 2);
    assert_eq!(team(&store, &id, "Engineering").unwrap(), accepted);
    assert_eq!(
        team(&store, &id, "Different").unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(legacy_rows(&store), before);

    let refused = ulid::from_ms(CLOCK, 92);
    assert!(team(&store, &refused, " ").is_err());
    assert_unbound(&store, 3);
    // Existing legacy-column updates and deletes must not invent a principal.
    let refusal_id = refused.clone();
    store.write(move |db| {
        assert_eq!(db.execute(
            "UPDATE commands SET result = json_set(result, '$.refusal.message', 'updated legacy refusal') WHERE command_id = ?1",
            [&refusal_id],
        ).map_err(OfficeError::storage)?, 1);
        Ok(((), None))
    }).unwrap();
    assert_unbound(&store, 3);
    store
        .write(move |db| {
            assert_eq!(
                db.execute("DELETE FROM commands WHERE command_id = ?1", [&refused])
                    .map_err(OfficeError::storage)?,
                1
            );
            Ok(((), None))
        })
        .unwrap();
    let committed = legacy_rows(&store);
    let rollback = ulid::from_ms(CLOCK, 93);
    let result: Result<(), OfficeError> = store.write(move |db| {
        db.execute(
            "INSERT INTO commands (command_id, epoch, seq, expires_at, result) SELECT ?1, epoch, seq, expires_at, result FROM commands LIMIT 1",
            [&rollback],
        ).map_err(OfficeError::storage)?;
        Err(OfficeError::Invalid("injected writer rollback"))
    });
    assert!(matches!(
        result,
        Err(OfficeError::Invalid("injected writer rollback"))
    ));
    assert_eq!(legacy_rows(&store), committed);
    assert_unbound(&store, 2);
    drop(store);
    let reopened = open(&path, Opening::Existing, &SCHEMA);
    assert_eq!(legacy_rows(&reopened), committed);
    assert_unbound(&reopened, 2);
}

fn bind(store: &Store, local: &str) {
    bind_replication(
        store,
        OrgId::new("ORG1").unwrap(),
        OfficeId::new(local).unwrap(),
        OfficeId::new("HQ").unwrap(),
        Digest::from_bytes([6; 32]),
        if local == "HQ" {
            ReplicationBootstrap::Hq {
                max_snapshot_bytes: SNAPSHOT_LIMIT,
            }
        } else {
            ReplicationBootstrap::Satellite {
                authority: Epoch::new(1).unwrap(),
                epoch_base: Digest::ZERO,
            }
        },
    )
    .unwrap();
}

fn exported_body(dir: &TempDir, exported: &OrganizationSnapshotExport) -> Vec<u8> {
    assert_eq!(
        exported.manifest.schema, 2,
        "projection version is independent of the local DB"
    );
    assert_eq!(exported.manifest.protocol_level, 1);
    let artifact = dir.path().join("artifact.db");
    std::fs::write(&artifact, &exported.ciphertext).unwrap();
    let db = crate::store::connect(&artifact, &Key::from_bytes([24; 32]), false, false).unwrap();
    let body: Vec<u8> = db
        .query_row("SELECT body FROM snapshot", [], |row| row.get(0))
        .unwrap();
    let projection: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(projection["header"]["schema"], 2);
    assert_eq!(projection["header"]["protocol"], 1);
    body
}

struct Adapter<'a>(&'a Store);
impl sync_org::OrgStore for Adapter<'_> {
    fn transact(&self, attempt: Arc<sync_org::OrgAttempt>) -> Result<OrgOutput, SyncError> {
        transact_replication(self.0, attempt)
    }
}

struct Hq;
impl VerifiedPeer for Hq {
    fn identity(&self) -> PeerIdentity {
        PeerIdentity {
            org_id: OrgId::new("ORG1").unwrap(),
            office_id: OfficeId::new("HQ").unwrap(),
            role: PeerRole::Hq,
            authority: Epoch::new(1).unwrap(),
        }
    }
}

#[test]
fn snapshot_roundtrip_keeps_projection_two_and_null_columns() {
    let legacy_dir = TempDir::new().unwrap();
    let legacy = open(&legacy_dir.path().join("org.db"), Opening::New, &LEGACY_V2);
    seed_legacy(&legacy, &fingerprint_vectors()["org"][0]);
    bind(&legacy, "HQ");
    let current_dir = TempDir::new().unwrap();
    let current = open(&current_dir.path().join("org.db"), Opening::New, &SCHEMA);
    assert_eq!(version(&current), 3);
    seed_legacy(&current, &fingerprint_vectors()["org"][0]);
    bind(&current, "HQ");
    let transport = Key::from_bytes([24; 32]);
    let old_export = export_replication_snapshot(&legacy, &transport, SNAPSHOT_LIMIT).unwrap();
    let new_export = export_replication_snapshot(&current, &transport, SNAPSHOT_LIMIT).unwrap();
    let golden = include_bytes!("fixtures/snapshot-projection-v2.json")
        .strip_suffix(b"\n")
        .unwrap();
    assert_eq!(exported_body(&legacy_dir, &old_export), golden);
    assert_eq!(exported_body(&current_dir, &new_export), golden);
    let old_base = replication_checkpoint(&legacy).unwrap();
    let new_base = replication_checkpoint(&current).unwrap();
    assert_eq!(old_base, new_base);

    let replica_dir = TempDir::new().unwrap();
    let replica_path = replica_dir.path().join("org.db");
    let replica = open(&replica_path, Opening::New, &SCHEMA);
    replica
        .write(|db| {
            seed_genesis(db)?;
            Ok(((), None))
        })
        .unwrap();
    bind(&replica, "SAT");
    let before = replication::staging_checkpoint(&replica).unwrap();
    assert!(before.1, "fresh replica must require its initial snapshot");
    for schema in [1, 3] {
        let mut wrong = old_export.manifest.clone();
        wrong.schema = schema;
        assert!(matches!(
            stage_replication_snapshot(
                &replica,
                Hq.identity(),
                &wrong,
                &transport,
                UnreadSnapshot,
                SNAPSHOT_LIMIT,
                SnapshotStageMode::Normal,
            ),
            Err(SyncError::Refused(Refusal::UnsupportedVersion))
        ));
        assert_eq!(replication::staging_checkpoint(&replica).unwrap(), before);
    }
    let mut wrong_protocol = old_export.manifest.clone();
    wrong_protocol.protocol_level = 2;
    assert!(matches!(
        stage_replication_snapshot(
            &replica,
            Hq.identity(),
            &wrong_protocol,
            &transport,
            UnreadSnapshot,
            SNAPSHOT_LIMIT,
            SnapshotStageMode::Normal,
        ),
        Err(SyncError::Refused(Refusal::UnsupportedVersion))
    ));
    assert_eq!(replication::staging_checkpoint(&replica).unwrap(), before);
    let staged = stage_replication_snapshot(
        &replica,
        Hq.identity(),
        &old_export.manifest,
        &transport,
        old_export.ciphertext.as_slice(),
        SNAPSHOT_LIMIT,
        SnapshotStageMode::Normal,
    )
    .unwrap();
    sync_org::install_snapshot(&Adapter(&replica), staged).unwrap();
    assert_eq!(legacy_rows(&replica), legacy_rows(&legacy));
    assert_unbound(&replica, 1);

    let command = ulid::from_ms(CLOCK, 91);
    team(&legacy, &command, "Engineering").unwrap();
    team(&current, &command, "Engineering").unwrap();
    let old_events = replication_events_after(&legacy, old_base.position, old_base.digest, 10)
        .unwrap()
        .unwrap();
    let new_events = replication_events_after(&current, new_base.position, new_base.digest, 10)
        .unwrap()
        .unwrap();
    assert_eq!(old_events.len(), 1);
    assert_eq!(old_events, new_events);
    let event = old_events[0].clone();
    assert_eq!(
        event.codec,
        org_sync_proto::Codec::new("org.native.changes", 1).unwrap()
    );
    let payload: serde_json::Value = serde_json::from_slice(&event.payload).unwrap();
    assert!(payload.get("principal").is_none());
    assert!(matches!(
        sync_org::apply(&Adapter(&replica), &Hq, event.clone()).unwrap(),
        OrgOutput::Ack {
            duplicate: false,
            ..
        }
    ));
    assert!(matches!(
        sync_org::apply(&Adapter(&replica), &Hq, event).unwrap(),
        OrgOutput::Ack {
            duplicate: true,
            ..
        }
    ));
    assert_eq!(legacy_rows(&replica), legacy_rows(&current));
    assert_unbound(&current, 2);
    assert_unbound(&replica, 2);
    let after = replication_checkpoint(&replica).unwrap();
    drop(replica);
    let reopened = open(&replica_path, Opening::Existing, &SCHEMA);
    assert_eq!(replication_checkpoint(&reopened).unwrap(), after);
    assert_eq!(legacy_rows(&reopened), legacy_rows(&current));
    assert_unbound(&reopened, 2);
}

fn schema_image(store: &Store) -> Vec<(String, String, Option<String>)> {
    store
        .read(|db| {
            let mut query = db
                .prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
                .map_err(OfficeError::storage)?;
            let rows = query
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .map_err(OfficeError::storage)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(OfficeError::storage)
        })
        .unwrap()
}

struct UnreadSnapshot;
impl std::io::Read for UnreadSnapshot {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        panic!("unsupported projection must be refused before payload I/O");
    }
}
