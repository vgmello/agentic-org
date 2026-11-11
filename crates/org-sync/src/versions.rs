//! Organization-level negotiation; wall time and membership are supplied by the adapter.

pub const ACTIVE_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    min: u32,
    max: u32,
}

impl Range {
    pub fn new(min: u32, max: u32) -> Option<Self> {
        (min > 0 && min <= max).then_some(Self { min, max })
    }
    fn supports(self, level: u32) -> bool {
        self.min <= level && level <= self.max
    }
}

/// Current level applies independently to the wire/schema/codec dimension being checked.
/// Pairwise range intersection cannot bypass or silently lower organization policy.
pub fn negotiate(local: Range, remote: Range, organization_level: u32) -> Option<u32> {
    (local.supports(organization_level) && remote.supports(organization_level))
        .then_some(organization_level)
}

#[derive(Clone, Copy, Debug)]
pub struct OfficeSupport {
    pub range: Range,
    pub last_seen_ms: u64,
}

/// The adapter must supply the complete current registry, including HQ. A future timestamp
/// remains active; a backward clock cannot accidentally drop a blocking office.
pub fn may_raise(
    current: u32,
    target: u32,
    hq: Range,
    now_ms: u64,
    offices: &[OfficeSupport],
) -> bool {
    current > 0
        && target > current
        && hq.supports(target)
        && !offices.is_empty()
        && offices.iter().all(|office| {
            now_ms.saturating_sub(office.last_seen_ms) > ACTIVE_MS || office.range.supports(target)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_ranges_and_pairwise_upgrade_cannot_bypass_the_organization_level() {
        assert!(Range::new(0, 1).is_none());
        assert!(Range::new(2, 1).is_none());
        let old = Range::new(1, 1).unwrap();
        let new = Range::new(1, 2).unwrap();
        assert_eq!(negotiate(new, new, 1), Some(1));
        assert_eq!(negotiate(old, new, 2), None);
        assert_eq!(negotiate(new, new, 0), None);
    }
    #[test]
    fn all_active_offices_gate_a_raise_and_an_old_returning_office_must_update() {
        let now = ACTIVE_MS + 100;
        let new = OfficeSupport {
            range: Range::new(1, 2).unwrap(),
            last_seen_ms: now,
        };
        let mut old = OfficeSupport {
            range: Range::new(1, 1).unwrap(),
            last_seen_ms: 100,
        };
        assert!(!may_raise(1, 2, new.range, now, &[new, old]));
        old.last_seen_ms = 99;
        assert!(may_raise(1, 2, new.range, now, &[new, old]));
        assert_eq!(negotiate(new.range, old.range, 2), None);
        old.last_seen_ms = now + 1;
        assert!(!may_raise(1, 2, new.range, now, &[new, old]));
        assert!(!may_raise(1, 1, new.range, now, &[new]));
        assert!(!may_raise(1, 2, new.range, now, &[]));
        assert!(!may_raise(0, 2, new.range, now, &[new]));
        assert!(!may_raise(1, 2, old.range, now, &[new]));
    }
}
