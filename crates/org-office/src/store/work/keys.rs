//! Development-v6 work key admission. Creation, upgrade and reopen share this routine.

use rusqlite::Transaction;

use crate::OfficeError;

// NumberScope defines task/bug numbers per project and requests per organization. JSON
// expressions match native reads; tombstones deliberately keep their keys. No projection
// columns or mutable counters can weaken the permanent uniqueness constraint.
const INDEXES: &str = "CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_project_number
    ON work_items(json_extract(data,'$.project_id'), json_extract(data,'$.kind'), json_extract(data,'$.number'))
    WHERE json_extract(data,'$.kind') IN ('task','bug') AND json_extract(data,'$.number') IS NOT NULL;
    CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_request_number
    ON work_items(json_extract(data,'$.number'))
    WHERE json_extract(data,'$.kind') = 'request' AND json_extract(data,'$.number') IS NOT NULL;";

/// Inside the schema-open transaction: invalid legacy shapes or duplicate keys refuse the
/// entire admission. No rows are repaired or renumbered, and no version is advanced on error.
pub(crate) fn admit(tx: &Transaction<'_>) -> Result<(), OfficeError> {
    let invalid: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM work_items
            WHERE json_extract(data,'$.kind') IN ('task','bug')
                AND json_extract(data,'$.number') IS NOT NULL
                AND json_extract(data,'$.project_id') IS NULL)",
            [],
            |row| row.get(0),
        )
        .map_err(OfficeError::storage)?;
    if invalid {
        return Err(OfficeError::Invalid(
            "work_items contains a numbered task or bug without a project",
        ));
    }
    tx.execute_batch(INDEXES).map_err(OfficeError::storage)
}
