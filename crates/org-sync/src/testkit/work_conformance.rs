//! Reuse against B1-14b's native writer, immutable event codec, pruning and reopen paths.
use crate::{
    error::{Refusal, SyncError},
    work::{LocalOutbox, SourceKey},
};
use std::fmt::Debug;

pub trait LocalWorkHarness: Sized {
    /// Must observe all native domain versions, source allocation, queue, clock and fence.
    type Snapshot: Debug + PartialEq;
    fn snapshot(&self) -> Self::Snapshot;
    fn queue(&self) -> LocalOutbox;
    fn source_counter(&self) -> u64;
    fn native_version_count(&self) -> usize;
    /// Change one harness-owned record's business field to tag, yielding one native event
    /// codec-encoded and queued inside its actual append savepoint. Reuse the same record.
    fn publish_fixture(&mut self, tag: u8) -> Result<SourceKey, SyncError>;
    fn fail_next_commit(&mut self);
    fn set_restore_pending(&mut self, pending: bool);
    /// Seed a valid queue exactly at its count or byte quota using native codec fixtures.
    fn exhaust_pending_quota(&mut self);
    fn reopen(&self) -> Self;
}

/// Two distinct disposable offices. No filesystem locations or native business fields
/// belong here: the harness must use its actual codec and native transactional append.
pub fn local_work_adapter<H: LocalWorkHarness>(first: &mut H, second: &mut H) {
    assert_ne!(first.queue().origin(), second.queue().origin());
    let before = first.snapshot();
    let counter = first.source_counter();
    let versions = first.native_version_count();
    first.fail_next_commit();
    assert!(matches!(
        first.publish_fixture(1),
        Err(SyncError::Storage(_))
    ));
    assert_eq!(first.snapshot(), before);
    *first = first.reopen();
    assert_eq!(first.snapshot(), before);
    let key = first.publish_fixture(1).unwrap();
    assert_eq!(key.seq.get(), counter + 1);
    assert_eq!(first.source_counter(), counter + 1);
    assert_eq!(first.native_version_count(), versions + 1);
    let frozen = first.queue().pending().back().unwrap().clone();
    *first = first.reopen();
    assert_eq!(first.queue().pending().back(), Some(&frozen));
    assert_eq!(first.publish_fixture(2).unwrap().seq.get(), counter + 2);
    *first = first.reopen();
    assert_eq!(first.queue().pending().front(), Some(&frozen));
    assert_eq!(
        first.queue().pending().back().unwrap().record_id,
        frozen.record_id
    );
    assert_ne!(
        first.queue().pending().back().unwrap().payload,
        frozen.payload
    );

    first.set_restore_pending(true);
    let fenced = first.snapshot();
    assert!(matches!(
        first.publish_fixture(3),
        Err(SyncError::Refused(Refusal::RestorePending))
    ));
    assert_eq!(first.snapshot(), fenced);
    let counter = second.source_counter();
    assert_eq!(second.publish_fixture(3).unwrap().seq.get(), counter + 1);
    assert_eq!(first.snapshot(), fenced);
    *first = first.reopen();
    assert_eq!(first.snapshot(), fenced);

    first.set_restore_pending(false);
    first.exhaust_pending_quota();
    let full = first.snapshot();
    let other = second.snapshot();
    assert!(matches!(
        first.publish_fixture(4),
        Err(SyncError::Refused(Refusal::QuotaExceeded))
    ));
    assert_eq!(first.snapshot(), full);
    *first = first.reopen();
    assert_eq!(first.snapshot(), full);
    assert_eq!(first.queue().pending().front(), Some(&frozen));
    assert_eq!(second.snapshot(), other);
}
