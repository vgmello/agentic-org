pub(super) const DDL: &str = r#"
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE accounts (
 account_id TEXT PRIMARY KEY CHECK (length(account_id) = 26),
 login_key TEXT NOT NULL UNIQUE, login TEXT NOT NULL,
 password_phc TEXT NOT NULL, admin INTEGER NOT NULL CHECK (admin IN (0, 1)),
 created_at_ms INTEGER NOT NULL, password_changed_at_ms INTEGER NOT NULL,
 revoked_at_ms INTEGER
) STRICT, WITHOUT ROWID;
CREATE TABLE account_links (
 account_id TEXT NOT NULL REFERENCES accounts(account_id),
 org_id TEXT NOT NULL, member_id TEXT NOT NULL, PRIMARY KEY (account_id, org_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX account_links_org ON account_links(org_id);
CREATE TABLE login_lockout (
 socket TEXT NOT NULL CHECK (socket IN ('control', 'local')),
 login_key TEXT NOT NULL, failures INTEGER NOT NULL, locked_until_ms INTEGER NOT NULL,
 last_failure_ms INTEGER NOT NULL, PRIMARY KEY (socket, login_key)
) STRICT, WITHOUT ROWID;
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
