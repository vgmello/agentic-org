//! Registered consumers and their durable watermarks, which cap journal release.

use super::*;

impl<E: Engine> Store<E> {
    /// Registers a durable journal consumer (such as multi-office replication, `ARCH-03` /
    /// `TASK-52`) so the flusher will never release journal segments past its watermark. Returns
    /// the consumer's current persisted watermark (reconstructing unreleased retained entries
    /// from disk segments when needed, or starting at the consistent materialized boundary so a
    /// consumer can never skip an evicted entry).
    pub fn register_consumer(&self, consumer_id: &str) -> Result<u64, StoreError> {
        validate_consumer_id(consumer_id)?;
        let _gate = self
            .core
            .release_gate
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let flow = self.core.flow();
            if let Some(&existing) = flow.consumer_watermarks.get(consumer_id) {
                return Ok(existing);
            }
        }
        self.core.journal.reconstruct_retained_from_segments()?;
        let mut flow = self.core.flow();
        if let Some(&existing) = flow.consumer_watermarks.get(consumer_id) {
            return Ok(existing);
        }
        let base = self.core.journal.base().ordinal;
        let initial = if self.core.journal.after_contiguous(base).is_ok() {
            base
        } else {
            flow.materialized.max(base)
        };
        persist_consumer_watermark(self.core.journal.dir(), consumer_id, initial)?;
        flow.consumer_watermarks
            .insert(consumer_id.to_owned(), initial);
        Ok(initial)
    }

    /// Advances a registered consumer's durable watermark monotonically and releases any
    /// retained segments now covered by all consumers (`ARCH-03`), serialized through `release_gate`.
    pub fn advance_consumer(&self, consumer_id: &str, ordinal: u64) -> Result<(), StoreError> {
        validate_consumer_id(consumer_id)?;
        let _gate = self
            .core
            .release_gate
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let release_target = {
            let mut flow = self.core.flow();
            let current = flow
                .consumer_watermarks
                .get(consumer_id)
                .copied()
                .ok_or_else(|| {
                    StoreError::Invalid(format!("unknown journal consumer `{consumer_id}`"))
                })?;
            let next = current.max(ordinal);
            if next > current {
                persist_consumer_watermark(self.core.journal.dir(), consumer_id, next)?;
                flow.consumer_watermarks
                    .insert(consumer_id.to_owned(), next);
            }
            flow.release_cap(flow.materialized)
        };
        if release_target > 0 {
            self.core.journal.evict_retained_through(release_target);
            self.core.journal.release_through(release_target)?;
        }
        Ok(())
    }

    /// Returns the persisted watermark for `consumer_id`, if registered.
    pub fn consumer_watermark(&self, consumer_id: &str) -> Option<u64> {
        self.core
            .flow()
            .consumer_watermarks
            .get(consumer_id)
            .copied()
    }
}

pub(super) fn validate_consumer_id(consumer_id: &str) -> Result<(), StoreError> {
    if consumer_id.is_empty()
        || consumer_id.len() > 64
        || !consumer_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(StoreError::Invalid(format!(
            "invalid journal consumer id `{consumer_id}`"
        )));
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct ConsumerWatermarkFile {
    v: u32,
    consumer_id: String,
    ordinal: u64,
}

pub(super) fn load_consumer_watermarks(
    journal_dir: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, u64>, StoreError> {
    let dir = journal_dir.join("consumers");
    let mut out = std::collections::BTreeMap::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir)
        .map_err(|e| StoreError::Internal(format!("read consumers dir: {e}")))?
    {
        let entry =
            entry.map_err(|e| StoreError::Internal(format!("read consumers entry: {e}")))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path)
            .map_err(|e| StoreError::Internal(format!("read consumer watermark: {e}")))?;
        let parsed: ConsumerWatermarkFile = serde_json::from_slice(&bytes)
            .map_err(|e| StoreError::Internal(format!("parse consumer watermark: {e}")))?;
        validate_consumer_id(&parsed.consumer_id)?;
        out.insert(parsed.consumer_id, parsed.ordinal);
    }
    Ok(out)
}

pub(super) fn persist_consumer_watermark(
    journal_dir: &std::path::Path,
    consumer_id: &str,
    ordinal: u64,
) -> Result<(), StoreError> {
    let dir = journal_dir.join("consumers");
    match org_platform::fs::create_owner_only_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(StoreError::Internal(format!("create consumers dir: {e}")));
        }
    }
    // Always sync the parent journal directory before publishing a consumer watermark so a retry
    // after a crash or failed fsync following `mkdir` never skips parent durability (`TASK-52` #15 / P3-#14).
    std::fs::File::open(journal_dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| StoreError::Internal(format!("sync journal dir: {e}")))?;
    let bytes = serde_json::to_vec_pretty(&ConsumerWatermarkFile {
        v: 1,
        consumer_id: consumer_id.to_owned(),
        ordinal,
    })
    .map_err(|e| StoreError::Internal(format!("encode consumer watermark: {e}")))?;
    crate::files::publish_one(&dir, &format!("{consumer_id}.json"), &bytes, false)
        .map_err(StoreError::Files)
}
