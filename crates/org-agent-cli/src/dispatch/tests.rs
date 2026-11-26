use super::*;

#[test]
fn derived_entity_ids_are_valid_uuid_v7_deterministic_and_domain_separated() {
    let key = "0193a5c0-0000-7000-8000-000000000001";
    let a = derive_uuid_v7("team.create", key);
    let b = derive_uuid_v7("team.create", key);
    let c = derive_uuid_v7("project.create.id", key);
    assert!(org_proto::is_uuid_v7(&a), "{a}");
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_ne!(a, key);
}

#[test]
fn parse_cursor_accepts_non_negative_offsets_and_rejects_garbage() {
    assert_eq!(parse_cursor(None).unwrap(), 0);
    assert_eq!(parse_cursor(Some("0")).unwrap(), 0);
    assert_eq!(parse_cursor(Some("25")).unwrap(), 25);
    assert_eq!(
        parse_cursor(Some("not-a-number")).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}
