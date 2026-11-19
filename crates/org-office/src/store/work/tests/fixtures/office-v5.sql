-- Released office v5 schema (C1 c47a46a3); independent of current create helpers.
CREATE TABLE member_status (
             member_id TEXT PRIMARY KEY, status_text TEXT NOT NULL, current_task_id TEXT,
             updated_at TEXT NOT NULL, expires_at TEXT NOT NULL
         ) STRICT;
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
CREATE TABLE msg_bodies (
             id TEXT PRIMARY KEY REFERENCES msg_headers (id) ON DELETE CASCADE,
             compression INTEGER NOT NULL, body_blob BLOB NOT NULL, meta_blob BLOB
         ) STRICT;
CREATE TABLE msg_headers (
             id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, reply_to TEXT,
             origin_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
             origin_seq INTEGER NOT NULL, local_seq INTEGER NOT NULL UNIQUE,
             org_epoch INTEGER NOT NULL, org_seq INTEGER NOT NULL,
             from_member_id TEXT NOT NULL, from_display_name TEXT NOT NULL,
             to_member_id TEXT NOT NULL, to_display_name TEXT NOT NULL,
             kind TEXT NOT NULL, priority TEXT NOT NULL DEFAULT 'normal', subject TEXT,
             created_at TEXT NOT NULL, expires_at TEXT, ttl_ms INTEGER,
             body_bytes INTEGER NOT NULL, meta_bytes INTEGER NOT NULL DEFAULT 0,
             content_hash BLOB NOT NULL, flags INTEGER NOT NULL DEFAULT 0, dead_reason TEXT
         ) STRICT;
CREATE TABLE msg_state (
             id TEXT PRIMARY KEY REFERENCES msg_headers (id) ON DELETE CASCADE,
             state TEXT NOT NULL, triage_gen INTEGER NOT NULL DEFAULT 0,
             snoozed_until TEXT, snooze_after_task INTEGER NOT NULL DEFAULT 0,
             reason TEXT, read_at TEXT, updated_at TEXT NOT NULL,
             owner_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
             origin_seq INTEGER NOT NULL, org_epoch INTEGER NOT NULL, org_seq INTEGER NOT NULL
         ) STRICT;
CREATE TABLE msg_thread_participants (
             member_id TEXT NOT NULL,
             thread_id TEXT NOT NULL REFERENCES msg_threads (thread_id) ON DELETE CASCADE,
             PRIMARY KEY (member_id, thread_id)
         ) STRICT;
CREATE TABLE msg_threads (
             thread_id TEXT PRIMARY KEY, subject TEXT, created_at TEXT NOT NULL,
             last_msg_id TEXT NOT NULL, last_msg_at TEXT NOT NULL,
             message_count INTEGER NOT NULL, participants_json TEXT NOT NULL
         ) STRICT;
CREATE TABLE msg_unread (
             to_member_id TEXT NOT NULL, local_seq INTEGER NOT NULL,
             id TEXT NOT NULL UNIQUE REFERENCES msg_headers (id) ON DELETE CASCADE,
             from_member_id TEXT NOT NULL, kind TEXT NOT NULL,
             priority TEXT NOT NULL DEFAULT 'normal', urgent INTEGER NOT NULL DEFAULT 0,
             triage_gen INTEGER NOT NULL DEFAULT 0, reply_to TEXT, created_at TEXT NOT NULL,
             expires_at TEXT, snoozed_until TEXT,
             snooze_after_task INTEGER NOT NULL DEFAULT 0, snooze_note TEXT, triaged_at TEXT,
             PRIMARY KEY (to_member_id, local_seq)
         ) STRICT;
CREATE TABLE work_events (
             target_id TEXT NOT NULL REFERENCES work_items(id),
             version INTEGER NOT NULL CHECK (version >= 1),
             feed_seq INTEGER NOT NULL CHECK (feed_seq >= 1) REFERENCES work_feed(seq),
             data TEXT NOT NULL,
             CHECK (json_extract(data, '$.target_id') IS target_id
                 AND json_extract(data, '$.version') IS version),
             PRIMARY KEY (target_id, version)
         ) STRICT, WITHOUT ROWID;
CREATE TABLE work_feed (
             seq INTEGER PRIMARY KEY AUTOINCREMENT,
             change_id TEXT, targets TEXT NOT NULL
         ) STRICT;
CREATE TABLE work_ids (
             id TEXT PRIMARY KEY, item_id TEXT NOT NULL REFERENCES work_items(id),
             kind TEXT NOT NULL CHECK (kind IN ('item', 'step', 'link', 'comment'))
         ) STRICT, WITHOUT ROWID;
CREATE TABLE work_items (
             id TEXT PRIMARY KEY, generation INTEGER NOT NULL CHECK (generation >= 1),
             version INTEGER NOT NULL CHECK (version >= 1), data TEXT NOT NULL,
             CHECK (json_extract(data, '$.id') IS id
                 AND json_extract(data, '$.generation') IS generation
                 AND json_extract(data, '$.version') IS version)
         ) STRICT, WITHOUT ROWID;
CREATE TABLE work_origin_counter (
             origin_incarnation TEXT PRIMARY KEY,
             last_seq INTEGER NOT NULL CHECK (last_seq >= 0)
         ) STRICT, WITHOUT ROWID;
CREATE TABLE work_origin_log (
             origin_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
             origin_seq INTEGER NOT NULL CHECK (origin_seq >= 1),
             feed_seq INTEGER NOT NULL REFERENCES work_feed(seq),
             target_id TEXT NOT NULL, generation INTEGER NOT NULL CHECK (generation >= 1),
             version INTEGER NOT NULL CHECK (version >= 0),
             org_epoch INTEGER NOT NULL CHECK (org_epoch >= 0),
             org_seq INTEGER NOT NULL CHECK (org_seq >= 0),
             committed_at_ms INTEGER NOT NULL CHECK (committed_at_ms >= 0),
             UNIQUE (target_id, generation, version),
             PRIMARY KEY (origin_office_id, origin_incarnation, origin_seq)
         ) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX idx_msg_delivery_failed_once
             ON msg_headers (reply_to) WHERE kind = 'delivery_failed';
CREATE UNIQUE INDEX idx_msg_origin
             ON msg_headers (origin_office_id, origin_incarnation, origin_seq);
CREATE INDEX idx_msg_reply_to ON msg_headers (reply_to) WHERE reply_to IS NOT NULL;
CREATE INDEX idx_msg_thread_seq ON msg_headers (thread_id, local_seq DESC);
CREATE INDEX idx_msg_threads_last ON msg_threads (last_msg_at DESC, thread_id DESC);
CREATE INDEX idx_msg_to_seq ON msg_headers (to_member_id, local_seq DESC);
CREATE INDEX idx_msg_unread_after_task ON msg_unread (to_member_id)
             WHERE snooze_after_task = 1;
CREATE INDEX idx_msg_unread_due ON msg_unread (to_member_id, snoozed_until)
             WHERE snoozed_until IS NOT NULL;
CREATE INDEX idx_work_ids_item ON work_ids(item_id);
CREATE INDEX idx_work_origin_stream ON work_origin_log(origin_incarnation, origin_seq);
CREATE INDEX idx_work_origin_target_version ON work_origin_log(target_id, version);
CREATE TRIGGER work_events_no_delete BEFORE DELETE ON work_events
             BEGIN SELECT RAISE(ABORT, 'work events are immutable'); END;
CREATE TRIGGER work_events_no_replace BEFORE INSERT ON work_events
             WHEN EXISTS (SELECT 1 FROM work_events
                 WHERE target_id = NEW.target_id AND version = NEW.version)
             BEGIN SELECT RAISE(ABORT, 'work events are immutable'); END;
CREATE TRIGGER work_events_no_update BEFORE UPDATE ON work_events
             BEGIN SELECT RAISE(ABORT, 'work events are immutable'); END;

CREATE TABLE work_change_results (
             change_id TEXT PRIMARY KEY CHECK (length(change_id) BETWEEN 1 AND 128),
             params_hash BLOB NOT NULL CHECK (length(params_hash) = 32),
             answer TEXT NOT NULL,
             expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms >= 0),
             principal_account TEXT, principal_member TEXT,
             actor_kind TEXT CHECK (actor_kind IS NULL OR length(actor_kind) > 0)
         ) STRICT, WITHOUT ROWID;
CREATE INDEX idx_work_change_results_expiry ON work_change_results(expires_at_ms);
CREATE TABLE work_number_counters (
             scope TEXT PRIMARY KEY, last INTEGER NOT NULL CHECK (last >= 0)
         ) STRICT, WITHOUT ROWID;
CREATE TABLE work_clock (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             high_water_ms INTEGER NOT NULL CHECK (high_water_ms >= 0)
         ) STRICT;
INSERT INTO member_status VALUES ('fixture-agent','retained status',NULL,'2026-11-11T00:00:00.000Z','2026-11-12T00:00:00.000Z');
INSERT INTO member_status VALUES ('fixture-task','retained task','01ARZ3NDEKTSV4RRFFQ69G5FAZ','2026-11-11T00:00:00.000Z','2026-11-12T00:00:00.000Z');
