-- Frozen legacy org.db DDL from v1 44d895657c323c0be79a3f72908bf083ef0434c6 before 2a.6a.
CREATE TABLE org_replica (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             org_id TEXT NOT NULL, local_office TEXT NOT NULL, hq_office TEXT NOT NULL,
             ca BLOB NOT NULL CHECK (length(ca) = 32),
             epoch INTEGER NOT NULL CHECK (epoch > 0), seq INTEGER NOT NULL CHECK (seq >= 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32),
             highest_epoch INTEGER NOT NULL CHECK (highest_epoch >= epoch),
             restore_pending INTEGER NOT NULL CHECK (restore_pending IN (0, 1)),
             pending_base BLOB CHECK (length(pending_base) = 32),
             protocol_level INTEGER NOT NULL CHECK (protocol_level > 0),
             awaiting_snapshot INTEGER NOT NULL CHECK (awaiting_snapshot IN (0, 1)),
             native_seq INTEGER NOT NULL CHECK (native_seq >= 0),
             CHECK (awaiting_snapshot = 0 OR local_office <> hq_office),
             CHECK ((epoch < highest_epoch) = (pending_base IS NOT NULL)),
             CHECK (epoch = highest_epoch OR restore_pending = 1)
         ) STRICT;
         CREATE TABLE org_replica_events (
             epoch INTEGER NOT NULL CHECK (epoch > 0), seq INTEGER NOT NULL CHECK (seq > 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32),
             previous BLOB NOT NULL CHECK (length(previous) = 32),
             codec TEXT NOT NULL, codec_version INTEGER NOT NULL, payload BLOB NOT NULL,
             PRIMARY KEY (epoch, seq)
         ) STRICT;
         CREATE TABLE org_replica_base (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             epoch INTEGER NOT NULL CHECK (epoch > 0), seq INTEGER NOT NULL CHECK (seq >= 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32)
         ) STRICT;
CREATE TABLE org_snapshot_stage (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             stage_id TEXT NOT NULL, epoch INTEGER NOT NULL CHECK (epoch > 0),
             seq INTEGER NOT NULL CHECK (seq >= 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32),
             body_digest BLOB NOT NULL CHECK (length(body_digest) = 32), body BLOB NOT NULL
         ) STRICT;
         CREATE TABLE org_snapshot_install (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             stage_id TEXT NOT NULL, epoch INTEGER NOT NULL CHECK (epoch > 0),
             seq INTEGER NOT NULL CHECK (seq >= 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32),
             body_digest BLOB NOT NULL CHECK (length(body_digest) = 32)
         ) STRICT;
