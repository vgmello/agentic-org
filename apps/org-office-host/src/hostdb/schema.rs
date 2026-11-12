pub(super) const DDL: &str = r#"
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE op_journal (
 operation_id TEXT PRIMARY KEY CHECK (length(operation_id) = 26),
 kind TEXT NOT NULL,
 fingerprint BLOB NOT NULL CHECK (length(fingerprint) = 32),
 account TEXT NOT NULL,
 state TEXT NOT NULL CHECK (state IN ('completed','rejected','reserved','linked','abandoned')),
 result TEXT,
 expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms >= 0),
 CHECK ((state = 'reserved') = (result IS NULL)),
 CHECK (state IN ('completed','rejected') OR kind = 'organization.create'),
 CHECK (kind <> 'organization.create' OR state <> 'completed')
) STRICT, WITHOUT ROWID;
CREATE INDEX op_journal_expiry ON op_journal(expires_at_ms);
CREATE TABLE op_verifiers (
 operation_id TEXT PRIMARY KEY REFERENCES op_journal(operation_id) ON DELETE CASCADE,
 created_account TEXT NOT NULL, verifier TEXT NOT NULL
) STRICT, WITHOUT ROWID;
CREATE INDEX op_verifiers_account ON op_verifiers(created_account);
CREATE TABLE org_create_staging (
 operation_id TEXT PRIMARY KEY REFERENCES op_journal(operation_id),
 name_key TEXT NOT NULL UNIQUE, org_id TEXT NOT NULL UNIQUE,
 office_id TEXT NOT NULL UNIQUE, owner_member_id TEXT NOT NULL
) STRICT, WITHOUT ROWID;
CREATE TABLE tls_pair (
 id INTEGER PRIMARY KEY CHECK (id = 1), cert_pem TEXT NOT NULL,
 pair_fp BLOB NOT NULL CHECK (length(pair_fp) = 32), set_at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE org_owners (
 org_id TEXT PRIMARY KEY, account TEXT NOT NULL, owner_member_id TEXT NOT NULL
) STRICT, WITHOUT ROWID;
"#;
