//! Organization levels are ordinal gates. Each published level fixes per-dimension ranges;
//! connection negotiation is bounded by those ranges. Time/membership come from adapters.
use org_sync_proto::Codec;

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
    pub const fn min(self) -> u32 {
        self.min
    }
    pub const fn max(self) -> u32 {
        self.max
    }
    fn supports(self, level: u32) -> bool {
        self.min <= level && level <= self.max
    }
}

#[derive(Clone, Copy, Debug)]
struct CodecSupport {
    id: &'static str,
    versions: Range,
}

#[derive(Clone, Copy, Debug)]
pub struct ProtocolProfile {
    pub wire: Range,
    pub schema: Range,
    codecs: &'static [CodecSupport],
}
impl ProtocolProfile {
    pub fn codec_range(&self, id: &str) -> Option<Range> {
        self.codecs
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.versions)
    }
}
const V1: Range = Range { min: 1, max: 1 };
const LEVEL_ONE: ProtocolProfile = ProtocolProfile {
    wire: V1,
    schema: V1,
    codecs: &[
        CodecSupport {
            id: "org.native.changes",
            versions: V1,
        },
        CodecSupport {
            id: "org.native.snapshot",
            versions: V1,
        },
        #[cfg(any(test, feature = "testkit"))]
        CodecSupport {
            id: "conformance",
            versions: V1,
        },
    ],
};

/// Only released levels are selectable by a production binary. Level two is an injected
/// test codec/policy fixture; it intentionally keeps every dimension at version one.
pub fn protocol_profile(level: u32) -> Option<&'static ProtocolProfile> {
    match level {
        1 => Some(&LEVEL_ONE),
        #[cfg(any(test, feature = "testkit"))]
        2 => Some(&LEVEL_ONE),
        _ => None,
    }
}

pub fn codec_supported(level: u32, codec: &Codec) -> bool {
    protocol_profile(level)
        .and_then(|profile| profile.codec_range(codec.id()))
        .is_some_and(|range| range.supports(codec.version()))
}

/// Organization-level selection cannot bypass its published profile or raise gate.
/// Actual wire/schema/codec negotiation uses negotiate_dimension with that profile's range.
pub fn negotiate(local: Range, remote: Range, organization_level: u32) -> Option<u32> {
    (protocol_profile(organization_level).is_some()
        && local.supports(organization_level)
        && remote.supports(organization_level))
    .then_some(organization_level)
}

pub fn negotiate_dimension(local: Range, remote: Range, allowed: Range) -> Option<u32> {
    let min = local.min.max(remote.min).max(allowed.min);
    let max = local.max.min(remote.max).min(allowed.max);
    (min <= max).then_some(max)
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
    protocol_profile(current).is_some()
        && protocol_profile(target).is_some()
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
    fn fixture_level_does_not_turn_ordinal_two_into_codec_or_wire_version_two() {
        let offered = Range::new(1, 2).unwrap();
        let profile = protocol_profile(2).unwrap();
        assert_eq!(negotiate_dimension(offered, offered, profile.wire), Some(1));
        assert_eq!(
            negotiate_dimension(offered, offered, profile.schema),
            Some(1)
        );
        assert!(codec_supported(2, &Codec::new("conformance", 1).unwrap()));
        assert!(!codec_supported(2, &Codec::new("conformance", 2).unwrap()));
        assert!(!codec_supported(1, &Codec::new("foreign", 1).unwrap()));
        assert!(!codec_supported(
            99,
            &Codec::new("org.native.changes", 1).unwrap()
        ));
        assert_eq!(
            negotiate_dimension(Range::new(2, 2).unwrap(), offered, profile.wire),
            None
        );
    }

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
