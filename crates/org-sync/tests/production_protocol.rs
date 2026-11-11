#![cfg(not(feature = "testkit"))]
use org_sync::versions::{may_raise, negotiate, protocol_profile, OfficeSupport, Range};

#[test]
fn production_cannot_select_or_raise_to_an_unreleased_fixture_level() {
    assert!(protocol_profile(1).is_some());
    assert!(protocol_profile(2).is_none());
    let range = Range::new(1, 2).unwrap();
    assert_eq!(negotiate(range, range, 2), None);
    assert!(!may_raise(
        1,
        2,
        range,
        100,
        &[OfficeSupport {
            range,
            last_seen_ms: 100
        }]
    ));
}
