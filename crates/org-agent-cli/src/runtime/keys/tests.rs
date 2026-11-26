use super::*;

#[test]
fn generates_distinct_canonical_uuid_v7_keys() {
    let mut keys = UuidV7KeySource;
    let a = keys.new_key();
    let b = keys.new_key();
    assert!(org_proto::is_uuid_v7(&a), "{a}");
    assert!(org_proto::is_uuid_v7(&b), "{b}");
    assert_ne!(a, b);
}
