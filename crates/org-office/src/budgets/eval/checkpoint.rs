//! Verified stream-watermark checkpoint handling for budget periods: regression detection
//! and monotonic merging of `ledger_checkpoint_ref` values (`TASK-97`).

use org_proto::{ApiError, ErrorCode};

/// Returns `true` if any previously verified stream watermark in `prev` (structured JSON array of
/// `VerifiedStreamWatermark`) is malformed, missing, regressed, or replaced/forked according to
/// `continues` (`TASK-97` / Final Gate Pass 7 #2, Pass 8 #1/#3).
pub fn checkpoint_ref_regressed(
    prev: Option<&str>,
    continues: &mut dyn FnMut(
        &crate::accounting::VerifiedStreamWatermark,
    ) -> Result<bool, ApiError>,
) -> Result<bool, ApiError> {
    let Some(prev_watermarks) = parse_verified_watermarks(prev) else {
        return Ok(true);
    };
    for wm in &prev_watermarks {
        if wm.high_water_sequence > 0 && !continues(wm)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Monotonically merges previously saved verified watermarks with newly verified watermarks in
/// `current`, never overwriting a prior watermark whose chain did not continue and never saving
/// unverified checkpoints (`TASK-97` / Final Gate Pass 8 #1/#3).
pub(super) fn merge_checkpoint_refs(
    prev: Option<&str>,
    current: Option<&str>,
    continues: &mut dyn FnMut(
        &crate::accounting::VerifiedStreamWatermark,
    ) -> Result<bool, ApiError>,
) -> Result<Option<String>, ApiError> {
    let Some(prev_list) = parse_verified_watermarks(prev) else {
        return Ok(prev.map(str::to_owned));
    };
    let mut merged: std::collections::BTreeMap<
        (String, String),
        crate::accounting::VerifiedStreamWatermark,
    > = std::collections::BTreeMap::new();
    for wm in prev_list {
        merged.insert((wm.origin_office_id.clone(), wm.producer_id.clone()), wm);
    }
    if let Some(cur_list) = parse_verified_watermarks(current) {
        for cur in cur_list {
            let key = (cur.origin_office_id.clone(), cur.producer_id.clone());
            match merged.get(&key) {
                Some(existing) => {
                    if cur.high_water_sequence >= existing.high_water_sequence
                        && (existing.high_water_sequence == 0 || continues(existing)?)
                    {
                        merged.insert(key, cur);
                    }
                }
                None => {
                    merged.insert(key, cur);
                }
            }
        }
    }
    if merged.is_empty() {
        Ok(None)
    } else {
        let vec: Vec<crate::accounting::VerifiedStreamWatermark> = merged.into_values().collect();
        serde_json::to_string(&vec)
            .map(Some)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to encode checkpoint_ref"))
    }
}

fn parse_verified_watermarks(
    raw: Option<&str>,
) -> Option<Vec<crate::accounting::VerifiedStreamWatermark>> {
    let Some(raw) = raw else {
        return Some(Vec::new());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(Vec::new());
    }
    serde_json::from_str(trimmed).ok()
}
