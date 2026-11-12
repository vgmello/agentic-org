-- Independent released office v2 fixture.
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
