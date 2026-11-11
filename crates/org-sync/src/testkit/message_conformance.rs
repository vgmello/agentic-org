//! Retention contract for native MessageTxn adapters. Use a disposable native database;
//! seed one committed identity, then invoke the adapter's actual pruning and reopen path.
use crate::{error::StoreError, messages::Message};

pub trait MessageRetentionHarness {
    /// Start with a clean fixture and commit a delivered proof or rejected expiry identity.
    fn seed_identity(&mut self, message: &Message, delivered: bool) -> Result<(), StoreError>;
    fn prune_and_reopen(&mut self, clock_ms: u64) -> Result<(), StoreError>;
    /// Read the retained native proof independently of the core expiry-first replay gate.
    fn retained_identity(&self, message: &Message) -> Result<Option<Message>, StoreError>;
}

pub fn assert_message_retention(
    harness: &mut dyn MessageRetentionHarness,
    message: &Message,
    delivered: bool,
) {
    assert!(message.deadline_ms > 0);
    harness
        .seed_identity(message, delivered)
        .expect("seed committed identity");
    assert_eq!(
        harness.retained_identity(message).unwrap(),
        Some(message.clone())
    );
    // deadline_ms includes the native grace. Neither body pruning nor reopen may erase
    // identity before that effective deadline, even when transport output was lost.
    harness
        .prune_and_reopen(message.deadline_ms - 1)
        .expect("prune and reopen");
    assert_eq!(
        harness.retained_identity(message).unwrap(),
        Some(message.clone())
    );
}
