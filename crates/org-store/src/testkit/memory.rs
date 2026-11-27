//! The second engine (ADR-0018): the projection as plain maps in memory. `begin` clones
//! the state, `commit` swaps it in, a snapshot is a shared pointer and a mark is a clone.
//! Slow on large data and never used in production; it proves the seam and keeps family
//! tests fast.

use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use crate::{
    change::{Id, Receipt, Sha256},
    engine::{CorePort, CoreRead, Cursor, Engine, Mark},
    error::{EngineError, StoreError},
    path::DurablePath,
    testkit::Dump,
};

/// One family's tables in the memory engine. A family in another crate implements this
/// for its own struct of maps and reaches it with [`MemoryTx::table_mut`].
pub trait FamilyTable: Any + Send + Sync {
    /// A stable name; dumps list families in its order.
    fn name(&self) -> &'static str;
    fn clone_box(&self) -> Box<dyn FamilyTable>;
    /// Every row as one line, sorted.
    fn dump(&self) -> Vec<String>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[derive(Default)]
struct State {
    cursor: Option<Cursor>,
    sources: BTreeMap<DurablePath, Sha256>,
    receipts: BTreeMap<Id, Receipt>,
    families: BTreeMap<TypeId, Box<dyn FamilyTable>>,
}

impl Clone for State {
    fn clone(&self) -> Self {
        Self {
            cursor: self.cursor.clone(),
            sources: self.sources.clone(),
            receipts: self.receipts.clone(),
            families: self
                .families
                .iter()
                .map(|(key, table)| (*key, table.clone_box()))
                .collect(),
        }
    }
}

impl State {
    fn table<T: FamilyTable>(&self) -> Option<&T> {
        self.families
            .get(&TypeId::of::<T>())
            .and_then(|table| table.as_any().downcast_ref())
    }

    fn sources_under(&self, prefix: &DurablePath) -> Vec<DurablePath> {
        let under = self.sources.keys().filter(|path| path.is_under(prefix));
        under.cloned().collect()
    }

    fn dump(&self) -> Vec<String> {
        let mut lines = vec![format!("cursor {:?}", self.cursor)];
        for (path, hash) in &self.sources {
            lines.push(format!("source {path} {hash}"));
        }
        for receipt in self.receipts.values() {
            lines.push(format!("receipt {receipt:?}"));
        }
        let mut families: Vec<_> = self.families.values().collect();
        families.sort_by_key(|table| table.name());
        for table in families {
            let name = table.name();
            lines.extend(table.dump().into_iter().map(|row| format!("{name} {row}")));
        }
        lines
    }
}

#[derive(Default)]
struct Knobs {
    fail_commits: AtomicUsize,
    fail_checks: AtomicUsize,
    /// Counts down on every write of a transaction; the write that reaches zero ends it.
    end_transaction_in: AtomicUsize,
}

fn take(counter: &AtomicUsize) -> bool {
    counter
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
}

#[derive(Default)]
pub struct MemoryEngine {
    committed: Mutex<Arc<State>>,
    knobs: Arc<Knobs>,
}

pub struct MemoryTx {
    state: State,
    mark: Option<(u64, State)>,
    marks: u64,
    ended: bool,
    knobs: Arc<Knobs>,
}

/// An immutable view of one committed state.
pub struct MemorySnapshot(Arc<State>);

impl MemoryEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn committed(&self) -> Arc<State> {
        Arc::clone(&self.committed.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn install(&self, state: State) {
        *self.committed.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(state);
    }

    /// The next `count` commits fail and roll back.
    pub fn fail_commits(&self, count: usize) {
        self.knobs.fail_commits.store(count, Ordering::SeqCst);
    }

    /// The next `count` calls of `check` report a violation.
    pub fn fail_checks(&self, count: usize) {
        self.knobs.fail_checks.store(count, Ordering::SeqCst);
    }

    /// The `nth` write from now (a source, a receipt or a family table taken for writing)
    /// ends its transaction, as an engine does on a full disk.
    pub fn end_transaction_at_write(&self, nth: usize) {
        self.knobs.end_transaction_in.store(nth, Ordering::SeqCst);
    }

    /// Forgets the projection, as deleting its file does.
    pub fn wipe(&self) {
        self.install(State::default());
    }
}

impl Engine for MemoryEngine {
    type Tx = MemoryTx;
    type Snapshot = MemorySnapshot;

    fn begin(&self) -> Result<MemoryTx, EngineError> {
        Ok(MemoryTx {
            state: self.committed().as_ref().clone(),
            mark: None,
            marks: 0,
            ended: false,
            knobs: Arc::clone(&self.knobs),
        })
    }

    fn commit(&self, mut tx: MemoryTx, cursor: &Cursor) -> Result<(), EngineError> {
        tx.alive()?;
        if take(&self.knobs.fail_commits) {
            return Err(EngineError::new("injected commit failure"));
        }
        tx.state.cursor = Some(cursor.clone());
        self.install(tx.state);
        Ok(())
    }

    fn snapshot(&self) -> Result<MemorySnapshot, EngineError> {
        Ok(MemorySnapshot(self.committed()))
    }

    fn replace(
        &self,
        build: &mut dyn FnMut(&mut MemoryTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
        let mut tx = MemoryTx {
            state: State::default(),
            mark: None,
            marks: 0,
            ended: false,
            knobs: Arc::clone(&self.knobs),
        };
        let cursor = build(&mut tx)?;
        tx.alive()?;
        tx.state.cursor = Some(cursor);
        self.install(tx.state);
        Ok(())
    }
}

impl Dump for MemoryEngine {
    fn dump(&self) -> Vec<String> {
        self.committed().dump()
    }
}

impl MemoryTx {
    fn alive(&self) -> Result<(), EngineError> {
        if self.ended {
            return Err(EngineError::ended("the transaction has ended"));
        }
        Ok(())
    }

    fn write(&mut self) -> Result<(), EngineError> {
        self.alive()?;
        let knob = &self.knobs.end_transaction_in;
        let reached = knob
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok_and(|before| before == 1);
        if reached {
            self.ended = true;
            return Err(EngineError::ended(
                "injected: the engine ended the transaction",
            ));
        }
        Ok(())
    }

    /// A family's tables, if it has written any.
    pub fn table<T: FamilyTable>(&self) -> Option<&T> {
        self.state.table()
    }

    /// A family's tables for writing, created empty on first use.
    pub fn table_mut<T: FamilyTable + Default>(&mut self) -> Result<&mut T, EngineError> {
        self.write()?;
        let table = self
            .state
            .families
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()));
        Ok(table
            .as_any_mut()
            .downcast_mut()
            .expect("keyed by its own type"))
    }
}

impl MemorySnapshot {
    pub fn table<T: FamilyTable>(&self) -> Option<&T> {
        self.0.table()
    }
}

impl CoreRead for MemorySnapshot {
    fn cursor(&self) -> Result<Option<Cursor>, EngineError> {
        Ok(self.0.cursor.clone())
    }

    fn source_hash(&self, path: &DurablePath) -> Result<Option<Sha256>, EngineError> {
        Ok(self.0.sources.get(path).copied())
    }

    fn sources_under(&self, prefix: &DurablePath) -> Result<Vec<DurablePath>, EngineError> {
        Ok(self.0.sources_under(prefix))
    }

    fn receipt(&self, id: &Id) -> Result<Option<Receipt>, EngineError> {
        Ok(self.0.receipts.get(id).cloned())
    }

    fn list_receipts_by_prefix(&self, prefix: &str) -> Result<Vec<Receipt>, EngineError> {
        let mut out: Vec<Receipt> = self
            .0
            .receipts
            .values()
            .filter(|r| r.kind.starts_with(prefix))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.ordinal.cmp(&b.ordinal).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }
}

impl CoreRead for MemoryTx {
    fn cursor(&self) -> Result<Option<Cursor>, EngineError> {
        self.alive()?;
        Ok(self.state.cursor.clone())
    }

    fn source_hash(&self, path: &DurablePath) -> Result<Option<Sha256>, EngineError> {
        self.alive()?;
        Ok(self.state.sources.get(path).copied())
    }

    fn sources_under(&self, prefix: &DurablePath) -> Result<Vec<DurablePath>, EngineError> {
        self.alive()?;
        Ok(self.state.sources_under(prefix))
    }

    fn receipt(&self, id: &Id) -> Result<Option<Receipt>, EngineError> {
        self.alive()?;
        Ok(self.state.receipts.get(id).cloned())
    }

    fn list_receipts_by_prefix(&self, prefix: &str) -> Result<Vec<Receipt>, EngineError> {
        self.alive()?;
        let mut out: Vec<Receipt> = self
            .state
            .receipts
            .values()
            .filter(|r| r.kind.starts_with(prefix))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.ordinal.cmp(&b.ordinal).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }
}

impl CorePort for MemoryTx {
    fn mark(&mut self) -> Result<Mark, EngineError> {
        self.alive()?;
        self.marks += 1;
        self.mark = Some((self.marks, self.state.clone()));
        Ok(Mark(self.marks))
    }

    fn undo_to(&mut self, mark: Mark) -> Result<(), EngineError> {
        self.alive()?;
        match self.mark.take() {
            Some((id, state)) if id == mark.0 => {
                self.state = state;
                Ok(())
            }
            other => {
                self.mark = other;
                Err(EngineError::new(
                    "only the most recent mark can be undone to",
                ))
            }
        }
    }

    fn set_source(&mut self, path: &DurablePath, hash: Option<Sha256>) -> Result<(), EngineError> {
        self.write()?;
        match hash {
            Some(hash) => self.state.sources.insert(path.clone(), hash),
            None => self.state.sources.remove(path),
        };
        Ok(())
    }

    fn put_receipt(&mut self, receipt: &Receipt) -> Result<(), EngineError> {
        self.write()?;
        if self.state.receipts.contains_key(&receipt.id) {
            return Err(EngineError::new("a receipt is never replaced"));
        }
        self.state
            .receipts
            .insert(receipt.id.clone(), receipt.clone());
        Ok(())
    }

    fn check(&mut self) -> Result<(), EngineError> {
        self.alive()?;
        if take(&self.knobs.fail_checks) {
            return Err(EngineError::new(
                "injected: a reference would fail at commit",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::engine_suite::cursor;

    crate::engine_conformance_tests!(|| (MemoryEngine::new(), ()));

    #[derive(Clone, Default)]
    struct Notes(BTreeMap<String, String>);

    impl FamilyTable for Notes {
        fn name(&self) -> &'static str {
            "notes"
        }
        fn clone_box(&self) -> Box<dyn FamilyTable> {
            Box::new(self.clone())
        }
        fn dump(&self) -> Vec<String> {
            self.0.iter().map(|(k, v)| format!("{k}={v}")).collect()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn a_family_in_another_crate_adds_its_tables_and_they_follow_marks_commits_and_dumps() {
        let engine = MemoryEngine::new();
        let mut tx = engine.begin().unwrap();
        assert!(tx.table::<Notes>().is_none());
        tx.table_mut::<Notes>()
            .unwrap()
            .0
            .insert("a".into(), "1".into());
        let mark = tx.mark().unwrap();
        tx.table_mut::<Notes>()
            .unwrap()
            .0
            .insert("b".into(), "2".into());
        tx.undo_to(mark).unwrap();
        assert_eq!(tx.table::<Notes>().unwrap().0.len(), 1);
        assert!(engine.snapshot().unwrap().table::<Notes>().is_none());
        engine.commit(tx, &cursor(1)).unwrap();
        let snapshot = engine.snapshot().unwrap();
        assert_eq!(snapshot.table::<Notes>().unwrap().0["a"], "1");
        let dump = engine.dump();
        assert!(dump[0].starts_with("cursor Some("), "{dump:?}");
        assert_eq!(dump[1], "notes a=1");
        engine.wipe();
        assert_eq!(engine.dump(), ["cursor None"]);
    }

    #[test]
    fn the_injected_failures_act_once_and_an_ended_transaction_refuses_everything() {
        let engine = MemoryEngine::new();
        let path = DurablePath::parse("state/a").unwrap();
        engine.end_transaction_at_write(2);
        let mut tx = engine.begin().unwrap();
        tx.set_source(&path, Some(Sha256::of(b"1"))).unwrap();
        let ended = tx.set_source(&path, Some(Sha256::of(b"2"))).unwrap_err();
        assert!(ended.transaction_ended);
        assert!(tx.source_hash(&path).unwrap_err().transaction_ended);
        assert!(tx.mark().unwrap_err().transaction_ended);
        assert!(engine.commit(tx, &cursor(1)).unwrap_err().transaction_ended);
        assert_eq!(engine.snapshot().unwrap().cursor().unwrap(), None);

        engine.fail_commits(1);
        engine.fail_checks(1);
        let mut tx = engine.begin().unwrap();
        tx.set_source(&path, Some(Sha256::of(b"1"))).unwrap();
        assert!(!tx.check().unwrap_err().transaction_ended);
        tx.check().unwrap();
        assert!(!engine.commit(tx, &cursor(1)).unwrap_err().transaction_ended);
        assert_eq!(engine.snapshot().unwrap().source_hash(&path).unwrap(), None);
        let tx = engine.begin().unwrap();
        engine.commit(tx, &cursor(1)).unwrap();
    }
}
