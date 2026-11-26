//! Adapter format, connection pragmas and DDL of the isolated `accounting.db` projection.

pub(super) const ACCOUNTING_ADAPTER_FORMAT: u32 = 3;
pub(super) const ACCOUNTING_PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
";

pub(super) const ACCOUNTING_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS model_prices (
    organization_id TEXT NOT NULL,
    id TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    effective_from TEXT NOT NULL,
    json_payload TEXT NOT NULL,
    PRIMARY KEY (organization_id, id),
    UNIQUE (organization_id, provider, model, effective_from)
) STRICT;

CREATE TABLE IF NOT EXISTS usage_records (
    organization_id TEXT NOT NULL,
    id TEXT NOT NULL,
    origin_office_id TEXT NOT NULL,
    producer_id TEXT NOT NULL,
    source_event_id TEXT NOT NULL,
    source_sequence INTEGER NOT NULL CHECK (source_sequence >= 1),
    occurred_at TEXT NOT NULL,
    payload_sha256 TEXT NOT NULL,
    chain_sha256 TEXT,
    supersedes_id TEXT,
    json_payload TEXT NOT NULL,
    PRIMARY KEY (organization_id, id),
    UNIQUE (organization_id, origin_office_id, producer_id, source_event_id),
    UNIQUE (organization_id, origin_office_id, producer_id, source_sequence),
    UNIQUE (organization_id, supersedes_id)
) STRICT;

CREATE INDEX IF NOT EXISTS usage_records_org_occurred
    ON usage_records(organization_id, occurred_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS usage_stream_checkpoints (
    organization_id TEXT NOT NULL,
    origin_office_id TEXT NOT NULL,
    producer_id TEXT NOT NULL,
    high_water_sequence INTEGER NOT NULL CHECK (high_water_sequence >= 0),
    head_sha256 TEXT,
    chain_sha256 TEXT,
    state TEXT NOT NULL,
    checkpointed_at TEXT NOT NULL,
    PRIMARY KEY (organization_id, origin_office_id, producer_id)
) STRICT;

CREATE TABLE IF NOT EXISTS staged_usage_records (
    organization_id TEXT NOT NULL,
    id TEXT NOT NULL,
    origin_office_id TEXT NOT NULL,
    producer_id TEXT NOT NULL,
    source_event_id TEXT NOT NULL,
    source_sequence INTEGER NOT NULL CHECK (source_sequence >= 1),
    supersedes_id TEXT,
    json_payload TEXT NOT NULL,
    PRIMARY KEY (organization_id, id)
) STRICT;

CREATE INDEX IF NOT EXISTS staged_usage_stream
    ON staged_usage_records(organization_id, origin_office_id, producer_id);

CREATE TABLE IF NOT EXISTS subscription_usage_snapshots (
    organization_id TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    office_id TEXT NOT NULL,
    status TEXT NOT NULL,
    collected_at TEXT NOT NULL,
    fresh_until TEXT,
    json_payload TEXT NOT NULL,
    PRIMARY KEY (organization_id, connection_id)
) STRICT;
"#;
