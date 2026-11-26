//! Adapter format, connection pragmas and DDL of the isolated `providers.db` projection.

pub(super) const PROVIDERS_ADAPTER_FORMAT: u32 = 1;
pub(super) const PROVIDERS_PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
";

pub(super) const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS coordinator_meta (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation >= 1)
) STRICT;

CREATE TABLE IF NOT EXISTS office_connection_bindings (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision >= 1),
  auth_context_ref TEXT NOT NULL CHECK (length(auth_context_ref) > 0),
  runtime_binary_path TEXT,
  account_identity_ref TEXT,
  updated_at TEXT NOT NULL,
  last_operation_id TEXT,
  last_operation_hash TEXT,
  PRIMARY KEY (organization_id, office_id, connection_id)
) STRICT;

CREATE TABLE IF NOT EXISTS office_connection_state (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('unknown','ready','auth_expired','unavailable')),
  binding_revision INTEGER NOT NULL CHECK (binding_revision >= 1),
  driver_version TEXT,
  observed_at TEXT,
  valid_until TEXT,
  issue_code TEXT,
  PRIMARY KEY (organization_id, office_id, connection_id),
  CHECK (status <> 'ready' OR (driver_version IS NOT NULL AND observed_at IS NOT NULL AND valid_until IS NOT NULL))
) STRICT;

CREATE TABLE IF NOT EXISTS office_connection_models (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  driver_model_id TEXT NOT NULL,
  entitlement TEXT NOT NULL CHECK (entitlement IN ('confirmed','denied','unknown')),
  capabilities_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(capabilities_json) AND json_type(capabilities_json) = 'object'),
  observed_at TEXT NOT NULL,
  valid_until TEXT NOT NULL,
  PRIMARY KEY (organization_id, office_id, connection_id, model_id)
) STRICT;

CREATE TABLE IF NOT EXISTS connection_quota_state (
  organization_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  model_id TEXT,
  status TEXT NOT NULL CHECK (status IN ('unknown','open','cooldown')),
  cooldown_until TEXT,
  observed_at TEXT NOT NULL,
  observed_by_office_id TEXT NOT NULL,
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation >= 1),
  PRIMARY KEY (organization_id, connection_id, scope_key),
  CHECK ((model_id IS NULL AND scope_key = 'connection') OR (model_id IS NOT NULL AND scope_key = 'model:' || model_id)),
  CHECK (status <> 'cooldown' OR cooldown_until IS NOT NULL)
) STRICT;

CREATE TABLE IF NOT EXISTS connection_turn_permits (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation >= 1),
  model_id TEXT NOT NULL,
  authority TEXT NOT NULL CHECK (authority IN ('hq_pool','office_local')),
  state TEXT NOT NULL CHECK (state IN ('active','release_pending','released')),
  issued_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation >= 1),
  PRIMARY KEY (organization_id, id)
) STRICT;
"#;
