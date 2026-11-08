//! The library's face for the office: run a write through the store's single writer, and build
//! reads.

use std::sync::Arc;

use org_proto::work::{WorkItem, WORK_GET_MAX_LISTED};

use crate::call::{Caller, WorkCall};
use crate::engine::{execute, TxnOutput};
use crate::kpi::time_figures;
use crate::model::{work_item, ItemReadParts, StoredItem};
use crate::ports::{Clock, OrgView, StoreError, WorkReads, WorkStore};

/// Work writes for one office.
pub struct WorkService<S> {
    store: S,
    clock: Arc<dyn Clock + Send + Sync>,
    office_id: String,
}

impl<S: WorkStore> WorkService<S> {
    /// `office_id` is this office's id: it owns what it creates (phase 1: HQ).
    pub fn new(
        store: S,
        clock: Arc<dyn Clock + Send + Sync>,
        office_id: impl Into<String>,
    ) -> Self {
        Self {
            store,
            clock,
            office_id: office_id.into(),
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// Runs one work write as `caller`. The body given to [`WorkStore::transact`] reads
    /// everything it decides on inside it and has no effect outside its transaction, so the
    /// writer may run it again.
    pub fn call(&self, caller: Caller, call: WorkCall) -> Result<TxnOutput, StoreError> {
        let clock = Arc::clone(&self.clock);
        let office_id = self.office_id.clone();
        self.store.transact(Arc::new(move |org, txn| {
            execute(org, txn, clock.as_ref(), &office_id, &caller, &call)
        }))
    }
}

/// An item in full, for `work.get`: its checklist, links (targets that are deleted marked so),
/// backlinks and time figures as of `as_of_ms`.
pub fn read_item(
    org: &dyn OrgView,
    reads: &dyn WorkReads,
    item: &StoredItem,
    as_of_ms: u64,
) -> Result<WorkItem, StoreError> {
    let mut events = vec![];
    let mut after = 0;
    loop {
        let page = reads.events(&item.id, after, 500)?;
        let Some(last) = page.last() else { break };
        if last.version <= after {
            break; // a store that ignores `after_version`: never loop
        }
        after = last.version;
        events.extend(page);
    }
    let owner_id = org.owner_id()?;
    let time = time_figures(item.kind, &item.created_by, &owner_id, &events, as_of_ms);
    let backlinks = reads.backlinks(&item.id, (WORK_GET_MAX_LISTED + 1) as u32)?;
    let mut deleted = std::collections::HashSet::new();
    for l in item.live_links() {
        if let org_proto::work::LinkTarget::Item { item_id } = &l.to {
            if reads.get(item_id)?.is_some_and(|t| t.deleted) {
                deleted.insert(item_id.clone());
            }
        }
    }
    Ok(work_item(
        item,
        ItemReadParts {
            backlinks,
            is_deleted: Box::new(move |id| deleted.contains(id)),
            time,
        },
    ))
}
