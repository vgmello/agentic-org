use super::convert::role_to_wire;
use super::*;
use crate::organization::records::{By, Meta, Role};

#[test]
fn role_to_wire_canonicalizes_rfc3339_variants_and_leap_seconds() {
    let role = Role {
        id: Id::parse("01900000-0000-7000-8000-0000000000a1").unwrap(),
        key: Some("eng".into()),
        name: "Engineer".into(),
        body: "## Mission\nBuild.\n".into(),
        meta: Meta {
            revision: 2,
            created_at: Some("2028-02-29t08:00:00+02:00".into()),
            created_by: By::System,
            updated_at: Some("2026-12-31t23:59:60z".into()),
            updated_by: By::Employee(Id::parse("01900000-0000-7000-8000-000000000001").unwrap()),
        },
    };
    let wire = role_to_wire(&role);
    assert_eq!(wire.created_at.as_deref(), Some("2028-02-29T06:00:00.000Z"));
    assert_eq!(wire.updated_at.as_deref(), Some("2027-01-01T00:00:00.000Z"));
    assert_eq!(wire.created_by_kind.as_deref(), Some("system"));
    assert_eq!(wire.updated_by_kind.as_deref(), Some("employee"));
    assert_eq!(
        wire.updated_by_id.as_deref(),
        Some("01900000-0000-7000-8000-000000000001")
    );

    // Offset timestamps whose UTC year exceeds 9999 preserve their validated 4-digit offset form;
    // leap second 9999-12-31T23:59:60Z clamps to 9999-12-31T23:59:59.999Z for browser parity.
    let boundary = Role {
        meta: Meta {
            created_at: Some("9999-12-31t23:59:59-00:01".into()),
            updated_at: Some("9999-12-31T23:59:60Z".into()),
            ..role.meta
        },
        ..role
    };
    let wire_boundary = role_to_wire(&boundary);
    assert_eq!(
        wire_boundary.created_at.as_deref(),
        Some("9999-12-31T23:59:59-00:01")
    );
    assert_eq!(
        wire_boundary.updated_at.as_deref(),
        Some("9999-12-31T23:59:59.999Z")
    );
}
