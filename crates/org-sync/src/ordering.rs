//! Order decisions use a coherent storage snapshot. They perform no writes or ACKs.

use org_sync_proto::{Digest, Epoch, Position};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    pub(crate) position: Position,
    pub(crate) digest: Digest,
    pub(crate) highest_epoch: Epoch,
    pub(crate) restore_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EventHead {
    pub(crate) position: Position,
    pub(crate) previous: Digest,
    pub(crate) digest: Digest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApplyOrder {
    /// The caller may apply domain payload + event + checkpoint in one transaction.
    /// Only that transaction's successful durable commit permits an ACK.
    Apply(Checkpoint),
    Duplicate,
    Gap {
        after: Position,
    },
    SnapshotRequired,
    RestorePending,
    StaleEpoch,
    Divergence,
    InvalidPosition,
}

/// Digests are validated by the codec before entering this internal policy.
/// `retained` is the committed digest at an earlier incoming position, if still retained.
/// A missing historical digest cannot establish that a repeated event is identical.
pub(crate) fn order_event(
    current: Checkpoint,
    incoming: EventHead,
    retained: Option<Digest>,
) -> ApplyOrder {
    if current.restore_pending {
        return ApplyOrder::RestorePending;
    }
    if incoming.position.seq == 0 {
        return ApplyOrder::InvalidPosition;
    }
    if incoming.position.epoch < current.highest_epoch {
        return ApplyOrder::StaleEpoch;
    }
    if incoming.position.epoch > current.highest_epoch
        || current.position.epoch != current.highest_epoch
    {
        // An event cannot authenticate a new authority or finish an epoch transition.
        return ApplyOrder::SnapshotRequired;
    }
    if incoming.position.seq <= current.position.seq {
        let known = if incoming.position == current.position {
            Some(current.digest)
        } else {
            retained
        };
        return match known {
            Some(digest) if digest == incoming.digest => ApplyOrder::Duplicate,
            Some(_) => ApplyOrder::Divergence,
            None => ApplyOrder::SnapshotRequired,
        };
    }
    if current.position.seq.checked_add(1) != Some(incoming.position.seq) {
        return ApplyOrder::Gap {
            after: current.position,
        };
    }
    if incoming.previous != current.digest {
        return ApplyOrder::Divergence;
    }
    ApplyOrder::Apply(Checkpoint {
        position: incoming.position,
        digest: incoming.digest,
        ..current
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkpoint(seq: u64) -> Checkpoint {
        Checkpoint {
            position: Position {
                epoch: Epoch::new(1).unwrap(),
                seq,
            },
            digest: Digest::from_bytes([seq as u8; 32]),
            highest_epoch: Epoch::new(1).unwrap(),
            restore_pending: false,
        }
    }
    fn event(seq: u64) -> EventHead {
        EventHead {
            position: Position {
                epoch: Epoch::new(1).unwrap(),
                seq,
            },
            previous: Digest::from_bytes([(seq - 1) as u8; 32]),
            digest: Digest::from_bytes([seq as u8; 32]),
        }
    }

    #[test]
    fn reordered_repeated_and_conflicting_events_have_distinct_decisions() {
        let zero = checkpoint(0);
        assert_eq!(
            order_event(zero, event(2), None),
            ApplyOrder::Gap {
                after: zero.position
            }
        );
        let ApplyOrder::Apply(one) = order_event(zero, event(1), None) else {
            panic!("first event")
        };
        let ApplyOrder::Apply(two) = order_event(one, event(2), None) else {
            panic!("second event")
        };
        assert_eq!(order_event(two, event(2), None), ApplyOrder::Duplicate);
        assert_eq!(
            order_event(two, event(1), Some(Digest::from_bytes([1; 32]))),
            ApplyOrder::Duplicate
        );
        assert_eq!(
            order_event(two, event(1), None),
            ApplyOrder::SnapshotRequired
        );
        let mut conflicting = event(2);
        conflicting.digest = Digest::from_bytes([99; 32]);
        assert_eq!(order_event(two, conflicting, None), ApplyOrder::Divergence);
        let mut broken_chain = event(3);
        broken_chain.previous = Digest::from_bytes([99; 32]);
        assert_eq!(order_event(two, broken_chain, None), ApplyOrder::Divergence);
        assert_eq!(two, checkpoint(2)); // pure decisions do not mutate committed state
    }

    #[test]
    fn an_event_cannot_change_authority_or_clear_a_restore_fence() {
        let mut current = checkpoint(2);
        current.highest_epoch = Epoch::new(2).unwrap();
        assert_eq!(order_event(current, event(3), None), ApplyOrder::StaleEpoch);
        let mut next_epoch = event(3);
        next_epoch.position.epoch = Epoch::new(2).unwrap();
        assert_eq!(
            order_event(current, next_epoch, None),
            ApplyOrder::SnapshotRequired
        );
        current.restore_pending = true;
        assert_eq!(
            order_event(current, next_epoch, None),
            ApplyOrder::RestorePending
        );
        let mut unverified_epoch = event(3);
        unverified_epoch.position.epoch = Epoch::new(3).unwrap();
        assert_eq!(
            order_event(checkpoint(2), unverified_epoch, None),
            ApplyOrder::SnapshotRequired
        );
    }

    #[test]
    fn zero_and_maximum_positions_never_wrap() {
        let mut invalid = event(1);
        invalid.position.seq = 0;
        assert_eq!(
            order_event(checkpoint(0), invalid, None),
            ApplyOrder::InvalidPosition
        );
        let maximum = checkpoint(u64::MAX);
        assert_eq!(
            order_event(maximum, event(1), Some(Digest::from_bytes([1; 32]))),
            ApplyOrder::Duplicate
        );
    }
}
