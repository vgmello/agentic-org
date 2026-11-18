//! In-memory `BrowserSessions` adapter. It proves the storage seam (ADR-0018
//! ENG-2) and runs the same contract suite as the SQLite registry.
use std::collections::BTreeSet;
use std::sync::Mutex;

use super::{
    is_live, Admission, BrowserSessions, Ended, NewSession, Origin, SessionDigest, SessionRecord,
    StoreError, SESSION_LIMIT,
};

struct Row {
    record: SessionRecord,
    cookie: SessionDigest,
    proof: SessionDigest,
}

#[derive(Default)]
struct State {
    rows: Vec<Row>,
    /// The latest time applied; calls never observe an earlier one.
    high_ms: i64,
}

/// Sessions held in memory. Like the registry's foreign key, only accounts
/// named at construction can hold a session.
pub(crate) struct MemorySessions {
    accounts: BTreeSet<String>,
    state: Mutex<State>,
}
impl MemorySessions {
    pub(crate) fn new(accounts: &[&str], clock_high_ms: i64) -> Self {
        Self {
            accounts: accounts.iter().map(|a| (*a).to_owned()).collect(),
            state: Mutex::new(State {
                rows: Vec::new(),
                high_ms: clock_high_ms,
            }),
        }
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, StoreError> {
        self.state.lock().map_err(|_| StoreError::Unavailable)
    }
}

fn ended(row: &Row) -> Ended {
    Ended {
        id: row.record.id.clone(),
        account_id: row.record.account_id.clone(),
    }
}
/// Removes the rows `drop` selects and returns them sorted by ID.
fn remove(rows: &mut Vec<Row>, drop: impl Fn(&Row) -> bool) -> Vec<Ended> {
    let mut gone: Vec<Ended> = rows.iter().filter(|r| drop(r)).map(ended).collect();
    rows.retain(|r| !drop(r));
    gone.sort();
    gone
}

impl BrowserSessions for MemorySessions {
    fn create(&self, new: &NewSession, now_ms: i64) -> Result<Admission, StoreError> {
        let mut state = self.lock()?;
        let now = now_ms.max(state.high_ms);
        let duplicate = state
            .rows
            .iter()
            .any(|r| r.record.id == new.id || r.cookie == new.cookie);
        if !self.accounts.contains(&new.account_id) || duplicate {
            return Err(StoreError::Unavailable);
        }
        state.high_ms = now;
        let reaped = remove(&mut state.rows, |r| {
            !is_live(r.record.last_activity_ms, now)
        });
        if state.rows.len() >= SESSION_LIMIT {
            return Ok(Admission::Full { reaped });
        }
        state.rows.push(Row {
            record: SessionRecord {
                id: new.id.clone(),
                account_id: new.account_id.clone(),
                created_ms: now,
                last_activity_ms: now,
                origin: new.origin.clone(),
            },
            cookie: new.cookie,
            proof: new.proof,
        });
        Ok(Admission::Created { reaped })
    }
    fn find(
        &self,
        cookie: &SessionDigest,
        proof: &SessionDigest,
        origin: &Origin,
        now_ms: i64,
    ) -> Result<Option<SessionRecord>, StoreError> {
        let state = self.lock()?;
        let now = now_ms.max(state.high_ms);
        Ok(state
            .rows
            .iter()
            .find(|r| r.cookie == *cookie)
            .filter(|r| {
                r.proof.ct_eq(proof)
                    && r.record.origin == *origin
                    && is_live(r.record.last_activity_ms, now)
            })
            .map(|r| r.record.clone()))
    }
    fn touch(&self, activity: &[(&str, i64)]) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        for (id, ms) in activity {
            if let Some(row) = state.rows.iter_mut().find(|r| r.record.id == *id) {
                row.record.last_activity_ms = row.record.last_activity_ms.max(*ms);
            }
        }
        Ok(())
    }
    fn end(&self, id: &str) -> Result<Option<Ended>, StoreError> {
        let mut state = self.lock()?;
        Ok(remove(&mut state.rows, |r| r.record.id == id).pop())
    }
    fn end_account(
        &self,
        account_id: &str,
        except: Option<&str>,
    ) -> Result<Vec<Ended>, StoreError> {
        let mut state = self.lock()?;
        Ok(remove(&mut state.rows, |r| {
            r.record.account_id == account_id && Some(r.record.id.as_str()) != except
        }))
    }
    fn reap(&self, now_ms: i64) -> Result<Vec<Ended>, StoreError> {
        let mut state = self.lock()?;
        let now = now_ms.max(state.high_ms);
        state.high_ms = now;
        Ok(remove(&mut state.rows, |r| {
            !is_live(r.record.last_activity_ms, now)
        }))
    }
}
