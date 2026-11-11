//! Office work ports; org.db snapshot checkout is supplied by the next slice.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};

use org_proto::work::{EventRef, LinkKind, WorkEvent, WorkPosition};
use org_proto::Position;
use org_work::{
    Clock, NumberScope, OrgView, StoreError, StoredItem, StoredResult, TxnFn, TxnOutput, WorkStore,
    WorkTxn,
};
use rusqlite::{params, Params};

use super::{
    call::{self, WorkCall, WorkCallError},
    metadata,
};
use crate::store::{Store, WriteJob};
use crate::OfficeError;

// Update LINK_KINDS when the org-proto exhaustive LinkKind inventory adds a known variant.
// Serde wire spellings are pinned by the proto literal and each typed variant.
const LINK_KINDS: &str =
    r#"["fixes","part_of","from_request","reported_from","relates_to","pays_back"]"#;

pub(crate) trait OrgSource: Send + Sync {
    fn snapshot(&self) -> Result<Box<dyn OrgView + '_>, StoreError>;
}

pub(crate) struct OfficeWorkStore {
    store: Arc<Store>,
    org: Arc<dyn OrgSource>,
    clock: Arc<OfficeClock>,
}

struct ClockState {
    attempt: Option<(ThreadId, u64)>,
    committed: u64,
}

struct OfficeClock {
    state: Mutex<ClockState>,
    wall: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl OfficeClock {
    fn state(&self) -> MutexGuard<'_, ClockState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Clock for OfficeClock {
    fn now_ms(&self) -> u64 {
        let state = self.state();
        if let Some((id, served)) = state.attempt {
            if id == thread::current().id() {
                return served;
            }
        }
        let committed = state.committed;
        drop(state);
        (self.wall)().max(committed)
    }
}

struct AttemptClock(Arc<OfficeClock>);

impl Drop for AttemptClock {
    fn drop(&mut self) {
        self.0.state().attempt = None;
    }
}

struct Reply {
    answer: call::Reply<(TxnOutput, u64)>,
    clock: Arc<OfficeClock>,
}

fn port(error: WorkCallError) -> StoreError {
    match error {
        WorkCallError::Port(error) => error,
        other => StoreError::new(other),
    }
}

fn storage(error: OfficeError) -> StoreError {
    port(WorkCallError::Store(error))
}

impl Reply {
    fn recv(self) -> Result<TxnOutput, StoreError> {
        let (output, served) = self.answer.recv().map_err(port)?;
        let mut state = self.clock.state();
        state.committed = state.committed.max(served);
        Ok(output)
    }
}

impl OfficeWorkStore {
    pub(crate) fn new(
        store: Arc<Store>,
        org: Arc<dyn OrgSource>,
        wall: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Result<Self, StoreError> {
        let committed = store.read(metadata::high_water).map_err(storage)?;
        Ok(Self {
            store,
            org,
            clock: Arc::new(OfficeClock {
                state: Mutex::new(ClockState {
                    attempt: None,
                    committed,
                }),
                wall,
            }),
        })
    }

    pub(crate) fn clock(&self) -> Arc<dyn Clock + Send + Sync> {
        self.clock.clone()
    }

    fn transact_job(&self, f: Arc<TxnFn>) -> (WriteJob, Reply) {
        let org = self.org.clone();
        let clock = self.clock.clone();
        let (job, answer) = call::transact_job(
            move |db| {
                let view = org.snapshot().map_err(WorkCallError::Port)?;
                let served =
                    (clock.wall)().max(metadata::high_water(db).map_err(WorkCallError::Store)?);
                clock.state().attempt = Some((thread::current().id(), served));
                let _attempt = AttemptClock(clock.clone());
                let output = call::run(db, |call| {
                    f(&*view, &mut NativeTxn { call, served }).map_err(WorkCallError::Port)
                })?;
                metadata::raise_high_water(db, served).map_err(WorkCallError::Store)?;
                Ok((output, served))
            },
            |_| false,
        );
        (
            job,
            Reply {
                answer,
                clock: self.clock.clone(),
            },
        )
    }
}

impl WorkStore for OfficeWorkStore {
    fn transact(&self, f: Arc<TxnFn>) -> Result<TxnOutput, StoreError> {
        let (job, answer) = self.transact_job(f);
        self.store.send_job(job).map_err(storage)?;
        answer.recv()
    }
}

struct NativeTxn<'a, 'db> {
    call: &'a mut WorkCall<'db>,
    served: u64,
}

impl NativeTxn<'_, '_> {
    fn owner(&self, id: &str, kind: &str) -> Result<Option<StoredItem>, StoreError> {
        match super::owner_of(self.call.conn(), id).map_err(storage)? {
            Some((id, found)) if found == kind => {
                super::get(self.call.conn(), &id).map_err(storage)
            }
            _ => Ok(None),
        }
    }

    fn select(&self, sql: &str, params: impl Params) -> Result<Vec<StoredItem>, StoreError> {
        let db = self.call.conn();
        let mut query = db
            .prepare(sql)
            .map_err(OfficeError::storage)
            .map_err(storage)?;
        let ids = query
            .query_map(params, |row| row.get::<_, String>(0))
            .map_err(OfficeError::storage)
            .map_err(storage)?;
        ids.map(|id| {
            let id = id.map_err(OfficeError::storage).map_err(storage)?;
            super::get(db, &id)
                .map_err(storage)?
                .ok_or_else(|| storage(OfficeError::Invalid("selected work item is missing")))
        })
        .collect()
    }
}

impl WorkTxn for NativeTxn<'_, '_> {
    fn get(&mut self, id: &str) -> Result<Option<StoredItem>, StoreError> {
        super::get(self.call.conn(), id).map_err(storage)
    }
    fn item_of_step(&mut self, id: &str) -> Result<Option<StoredItem>, StoreError> {
        self.owner(id, "step")
    }
    fn item_of_link(&mut self, id: &str) -> Result<Option<StoredItem>, StoreError> {
        self.owner(id, "link")
    }
    fn id_in_use(&mut self, id: &str) -> Result<bool, StoreError> {
        Ok(super::owner_of(self.call.conn(), id)
            .map_err(storage)?
            .is_some())
    }
    fn linked_to(&mut self, id: &str, kind: LinkKind) -> Result<Vec<StoredItem>, StoreError> {
        self.select(
            "SELECT id FROM work_items WHERE EXISTS (
            SELECT 1 FROM json_each(data, '$.links') AS link
            WHERE json_extract(link.value,'$.removed') = 0
                AND CASE WHEN json_extract(link.value,'$.kind') IN (SELECT value FROM json_each(?3))
                    THEN json_extract(link.value,'$.kind') ELSE 'unknown' END = json_extract(?2,'$')
                AND json_extract(link.value,'$.to.type') = 'item'
                AND json_extract(link.value,'$.to.item_id') = ?1) ORDER BY id",
            params![
                id,
                serde_json::to_string(&kind)
                    .map_err(OfficeError::storage)
                    .map_err(storage)?,
                LINK_KINDS
            ],
        )
    }
    fn open_bugs(&mut self, project: Option<&str>) -> Result<Vec<StoredItem>, StoreError> {
        self.select(
            "SELECT id FROM work_items WHERE json_extract(data,'$.kind') = 'bug'
            AND json_extract(data,'$.deleted') = 0 AND json_extract(data,'$.status') IS NOT 'closed'
            AND (?1 IS NULL OR json_extract(data,'$.project_id') = ?1) ORDER BY id",
            [project],
        )
    }
    fn events_since(&mut self, id: &str, after: u64) -> Result<Vec<WorkEvent>, StoreError> {
        Ok(super::event_records(self.call.conn(), id, after)
            .map_err(storage)?
            .into_iter()
            .map(|(_, event)| event)
            .collect())
    }
    fn change_result(&mut self, id: &str) -> Result<Option<StoredResult>, StoreError> {
        metadata::change_result(self.call.conn(), id).map_err(storage)
    }
    fn allocate_number(&mut self, scope: &NumberScope) -> Result<u64, StoreError> {
        self.call.allocate_number(scope).map_err(port)
    }
    fn put_item(&mut self, item: &StoredItem, events: &[WorkEvent]) -> Result<(), StoreError> {
        debug_assert!(item.updated_at_ms <= self.served);
        self.call.put_item(item, events).map_err(port)
    }
    fn put_change_result(&mut self, id: &str, result: &StoredResult) -> Result<(), StoreError> {
        self.call.put_change_result(id, result).map_err(port)
    }
    fn record_change(
        &mut self,
        position: Position,
        refs: &[EventRef],
        id: Option<&str>,
    ) -> Result<WorkPosition, StoreError> {
        self.call
            .record_change(position, refs, id, self.served)
            .map_err(port)
    }
}

#[cfg(test)]
#[path = "tests/assembly.rs"]
mod tests;
