//! An in-memory key store for unit tests, with fault injection: each operation can be made to
//! fail with a chosen error, and every call is recorded, so a test can check what the host did
//! with its keys and in what order.

use std::collections::HashMap;
use std::io;
use std::sync::Mutex;

use org_office_launch::is_plain_id;
use org_proto::DbKey;

use super::{KeyStore, KeyStoreError};
use crate::sync::lock;

/// A store operation, for faults and the call record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Op {
    Create,
    Load,
    Delete,
}

/// An error to inject (a [`KeyStoreError`] isn't `Clone`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    Corrupt,
    Os(i32),
    Io,
}

impl Fault {
    fn error(self) -> KeyStoreError {
        match self {
            Fault::Corrupt => KeyStoreError::Corrupt,
            Fault::Os(status) => KeyStoreError::Os(status),
            Fault::Io => KeyStoreError::Io(io::Error::other("an injected fault")),
        }
    }
}

/// Keys in a map, by office id.
#[derive(Default)]
pub(crate) struct MemoryKeys {
    keys: Mutex<HashMap<String, Box<DbKey>>>,
    faults: Mutex<HashMap<Op, Fault>>,
    calls: Mutex<Vec<(Op, String)>>,
}

impl MemoryKeys {
    /// From now on, every `op` fails with `fault` (until [`MemoryKeys::clear_fault`]).
    pub(crate) fn fail(&self, op: Op, fault: Fault) {
        lock(&self.faults).insert(op, fault);
    }

    pub(crate) fn clear_fault(&self, op: Op) {
        lock(&self.faults).remove(&op);
    }

    /// Every call so far, in order, failed ones included.
    pub(crate) fn calls(&self) -> Vec<(Op, String)> {
        lock(&self.calls).clone()
    }

    /// Whether the store holds a key for `office_id`.
    pub(crate) fn has(&self, office_id: &str) -> bool {
        lock(&self.keys).contains_key(office_id)
    }

    /// Whether the store holds exactly `key` for `office_id`.
    pub(crate) fn holds(&self, office_id: &str, key: &DbKey) -> bool {
        lock(&self.keys)
            .get(office_id)
            .is_some_and(|stored| **stored == *key)
    }

    fn begin(&self, op: Op, office_id: &str) -> Result<(), KeyStoreError> {
        lock(&self.calls).push((op, office_id.to_owned()));
        if !is_plain_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        match lock(&self.faults).get(&op) {
            Some(fault) => Err(fault.error()),
            None => Ok(()),
        }
    }
}

/// A copy of `key` (test only: `DbKey` isn't `Clone`).
fn copy(key: &DbKey) -> Box<DbKey> {
    Box::new(DbKey::from_bytes(*key.expose_bytes()))
}

impl KeyStore for MemoryKeys {
    fn create(&self, office_id: &str, key: &DbKey) -> Result<(), KeyStoreError> {
        self.begin(Op::Create, office_id)?;
        let mut keys = lock(&self.keys);
        if keys.contains_key(office_id) {
            return Err(KeyStoreError::Exists);
        }
        keys.insert(office_id.to_owned(), copy(key));
        Ok(())
    }

    fn load(&self, office_id: &str) -> Result<Option<Box<DbKey>>, KeyStoreError> {
        self.begin(Op::Load, office_id)?;
        Ok(lock(&self.keys).get(office_id).map(|key| copy(key)))
    }

    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError> {
        self.begin(Op::Delete, office_id)?;
        lock(&self.keys).remove(office_id);
        Ok(())
    }
}

mod tests {
    use super::*;

    const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn create_only_adds_load_returns_it_and_delete_of_a_missing_key_is_ok() {
        let store = MemoryKeys::default();
        let (one, two) = (DbKey::from_bytes([1; 32]), DbKey::from_bytes([2; 32]));
        assert!(store.load(A).unwrap().is_none());
        store.delete(A).unwrap();
        store.create(A, &one).unwrap();
        assert!(matches!(store.create(A, &two), Err(KeyStoreError::Exists)));
        assert!(store.holds(A, &one));
        assert_eq!(*store.load(A).unwrap().unwrap(), one);
        store.delete(A).unwrap();
        assert!(!store.has(A));
        store.delete(A).unwrap();
    }

    #[test]
    fn faults_fail_their_operation_until_cleared_and_change_nothing() {
        let store = MemoryKeys::default();
        let key = DbKey::from_bytes([3; 32]);
        store.fail(Op::Create, Fault::Os(-25308));
        assert!(matches!(
            store.create(A, &key),
            Err(KeyStoreError::Os(-25308))
        ));
        assert!(!store.has(A));
        store.clear_fault(Op::Create);
        store.create(A, &key).unwrap();

        store.fail(Op::Load, Fault::Corrupt);
        assert!(matches!(store.load(A), Err(KeyStoreError::Corrupt)));
        store.fail(Op::Delete, Fault::Io);
        assert!(matches!(store.delete(A), Err(KeyStoreError::Io(_))));
        assert!(store.holds(A, &key));
        store.clear_fault(Op::Load);
        assert!(store.load(A).unwrap().is_some());

        assert!(matches!(store.load("../x"), Err(KeyStoreError::NotPlainId)));
        let ops: Vec<Op> = store.calls().into_iter().map(|(op, _)| op).collect();
        use Op::{Create, Delete, Load};
        assert_eq!(ops, [Create, Create, Load, Delete, Load, Load]);
    }
}
