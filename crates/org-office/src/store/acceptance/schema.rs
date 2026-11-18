//! Executable additive office migration. Source journals remain authoritative.

use rusqlite::Transaction;

pub(crate) fn create(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE selected_directory (
            organization_id TEXT NOT NULL, generation INTEGER NOT NULL CHECK(generation>0),
            data TEXT NOT NULL, payload_sha256 TEXT NOT NULL,
            PRIMARY KEY(organization_id,generation)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_directory_head (
            organization_id TEXT PRIMARY KEY, generation INTEGER NOT NULL CHECK(generation>0),
            FOREIGN KEY(organization_id,generation) REFERENCES selected_directory(organization_id,generation)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_owner_authority (
            organization_id TEXT PRIMARY KEY, account_id TEXT NOT NULL,
            member_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
            active INTEGER NOT NULL CHECK(active IN (0,1))
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_tasks (
            organization_id TEXT NOT NULL, task_id TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK(revision>0),
            team_id TEXT NOT NULL, project_id TEXT,
            assigned_team_id TEXT, assignee_id TEXT,
            canonical_path TEXT NOT NULL, markdown BLOB NOT NULL, payload_sha256 TEXT NOT NULL,
            PRIMARY KEY(organization_id,task_id), UNIQUE(canonical_path)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_events (
            organization_id TEXT NOT NULL, task_id TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK(revision>0),
            schema_version INTEGER NOT NULL, resource_profile TEXT NOT NULL,
            payload_sha256 TEXT NOT NULL, event_bytes BLOB NOT NULL,
            PRIMARY KEY(organization_id,task_id,revision),
            FOREIGN KEY(organization_id,task_id) REFERENCES selected_tasks(organization_id,task_id)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_receipts (
            organization_id TEXT NOT NULL, operation_id TEXT NOT NULL,
            instance_id TEXT NOT NULL, account_id TEXT NOT NULL,
            actor_kind TEXT NOT NULL, actor_id TEXT,
            method TEXT NOT NULL, command_version INTEGER NOT NULL,
            fingerprint TEXT NOT NULL, result BLOB NOT NULL,
            expires_at_ms INTEGER NOT NULL CHECK(expires_at_ms>=0),
            PRIMARY KEY(organization_id,operation_id),
            CHECK ((actor_kind='employee')=(actor_id IS NOT NULL))
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_adoptions (
            organization_id TEXT NOT NULL, task_id TEXT NOT NULL,
            writer_epoch INTEGER NOT NULL CHECK(writer_epoch>0),
            origin_generation INTEGER NOT NULL CHECK(origin_generation>0),
            manifest_sha256 TEXT NOT NULL, source_metadata TEXT NOT NULL,
            PRIMARY KEY(organization_id,task_id),
            FOREIGN KEY(organization_id,task_id) REFERENCES selected_tasks(organization_id,task_id)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_artifacts (
            namespace TEXT NOT NULL CHECK(namespace IN ('public','protected')),
            path TEXT NOT NULL, content BLOB NOT NULL,
            immutable INTEGER NOT NULL CHECK(immutable IN (0,1)),
            PRIMARY KEY(namespace,path)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE selected_journal_cursor (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            identity TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK(ordinal>=0),
            digest TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
            origin_high_water INTEGER NOT NULL CHECK(origin_high_water>=0)
         ) STRICT;",
    )?;
    for table in [
        "selected_events",
        "selected_receipts",
        "selected_directory",
        "selected_adoptions",
    ] {
        let key=match table {
            "selected_events"=>"organization_id=NEW.organization_id AND task_id=NEW.task_id AND revision=NEW.revision",
            "selected_receipts"=>"organization_id=NEW.organization_id AND operation_id=NEW.operation_id",
            "selected_directory"=>"organization_id=NEW.organization_id AND generation=NEW.generation",
            _=>"organization_id=NEW.organization_id AND task_id=NEW.task_id",
        };
        tx.execute_batch(&format!(
            "CREATE TRIGGER {table}_immutable_insert BEFORE INSERT ON {table}
             WHEN EXISTS(SELECT 1 FROM {table} WHERE {key})
             BEGIN SELECT RAISE(ABORT,'selected retained identity already exists'); END;
             CREATE TRIGGER {table}_immutable_update BEFORE UPDATE ON {table}
             BEGIN SELECT RAISE(ABORT,'selected retained source is immutable'); END;
             CREATE TRIGGER {table}_immutable_delete BEFORE DELETE ON {table}
             BEGIN SELECT RAISE(ABORT,'selected retained source cannot be deleted'); END;"
        ))?;
    }
    tx.execute_batch("CREATE TRIGGER selected_gate_child_replace BEFORE INSERT ON work_ids
        WHEN EXISTS(SELECT 1 FROM work_ids i JOIN selected_adoptions a ON a.task_id=i.item_id WHERE i.id=NEW.id)
        BEGIN SELECT RAISE(ABORT,'selected task requires selected contract'); END;")?;
    tx.execute_batch(
        "CREATE TRIGGER selected_receipt_legacy_collision BEFORE INSERT ON selected_receipts
        WHEN EXISTS(SELECT 1 FROM work_change_results WHERE change_id=NEW.operation_id)
        BEGIN SELECT RAISE(ABORT,'work operation identity already exists'); END;
        CREATE TRIGGER legacy_receipt_selected_collision BEFORE INSERT ON work_change_results
        WHEN EXISTS(SELECT 1 FROM selected_receipts WHERE operation_id=NEW.change_id)
        BEGIN SELECT RAISE(ABORT,'work operation identity already exists'); END;",
    )?;
    // These are permanent format gates, including raw replication/Undo writes.
    // Selected state is separate; no selected update mutates legacy StoredItem.
    for (table, id) in [
        ("work_items", "id"),
        ("work_events", "target_id"),
        ("work_ids", "item_id"),
    ] {
        for (operation, row) in [("INSERT", "NEW"), ("UPDATE", "OLD"), ("DELETE", "OLD")] {
            let condition = if operation == "UPDATE" {
                format!("task_id=OLD.{id} OR task_id=NEW.{id}")
            } else {
                format!("task_id={row}.{id}")
            };
            tx.execute_batch(&format!(
                "CREATE TRIGGER selected_gate_{table}_{operation} BEFORE {operation} ON {table}
                 WHEN EXISTS(SELECT 1 FROM selected_adoptions WHERE {condition})
                 BEGIN SELECT RAISE(ABORT,'selected task requires selected contract'); END;"
            ))?;
        }
    }
    Ok(())
}
