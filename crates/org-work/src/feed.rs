//! `work.changed` (§4.10): one per committed call, split into parts so each encodes under
//! [`WORK_CHANGED_MAX_BYTES`].

use org_proto::work::{WorkChange, WorkChangedData, WorkPosition, WORK_CHANGED_MAX_BYTES};

use crate::engine::Draft;

/// Room left for the envelope around the changes (position, change id, part numbers).
const ENVELOPE_BYTES: usize = 512;

/// The `work.changed` parts for one committed call: each changed item, whole, followed by the
/// events it recorded.
pub(crate) fn parts(
    position: WorkPosition,
    change_id: Option<&str>,
    drafts: &[Draft],
) -> Vec<WorkChangedData> {
    let changes = drafts.iter().flat_map(|d| {
        std::iter::once(WorkChange::ItemUpserted {
            item: d.item.summary(),
        })
        .chain(
            d.events
                .iter()
                .map(|e| WorkChange::EventAdded { event: e.clone() }),
        )
    });
    split(
        position,
        change_id,
        changes.collect(),
        WORK_CHANGED_MAX_BYTES,
    )
}

/// Packs `changes`, in order, into as few parts as fit `max_bytes` each. A change too big for a
/// part on its own gets a part of its own.
pub(crate) fn split(
    position: WorkPosition,
    change_id: Option<&str>,
    changes: Vec<WorkChange>,
    max_bytes: usize,
) -> Vec<WorkChangedData> {
    let budget = max_bytes.saturating_sub(ENVELOPE_BYTES + change_id.map_or(0, str::len));
    let mut groups: Vec<Vec<WorkChange>> = vec![];
    let mut current: Vec<WorkChange> = vec![];
    let mut used = 0usize;
    for change in changes {
        // `+ 1` for the comma between changes.
        let size = serde_json::to_vec(&change).map_or(0, |v| v.len()) + 1;
        if !current.is_empty() && used + size > budget {
            groups.push(std::mem::take(&mut current));
            used = 0;
        }
        used += size;
        current.push(change);
    }
    if !current.is_empty() || groups.is_empty() {
        groups.push(current);
    }
    let parts = u32::try_from(groups.len()).unwrap_or(u32::MAX);
    groups
        .into_iter()
        .enumerate()
        .map(|(i, changes)| WorkChangedData {
            position,
            change_id: change_id.map(str::to_owned),
            part: u32::try_from(i + 1).unwrap_or(u32::MAX),
            parts,
            changes,
        })
        .collect()
}
