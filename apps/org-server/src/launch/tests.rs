use super::*;
use org_office_launch::v3::{CreateLine, HandoffFd, OfficeLine};
use org_office_launch::{CreateOffice, DiscardOffice, StartOffice};
use org_proto::{AppToken, DbKey, GrantKey};

const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
fn db_key() -> DbKey {
    DbKey::from_bytes([0xb3; 32])
}
fn grant() -> GrantKey {
    GrantKey::from_bytes([7; 32])
}
fn token() -> AppToken {
    AppToken::from_bytes([0xa7; 32])
}
fn office() -> OfficeLine {
    OfficeLine {
        instance_id: INSTANCE.into(),
        office_id: "OFFICE1".into(),
        rendezvous: "/unused".into(),
    }
}
fn create() -> CreateLine {
    CreateLine {
        office: office(),
        org_id: "ORG2".into(),
        org_name: "Acme".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        owner_member_id: OWNER.into(),
    }
}
fn request(version: u8, op: u8, key: bool) -> Request {
    match (version, op) {
        (2, 0) => Request::V2(HostMessage::Create(CreateOffice {
            org_id: "org-2".into(),
            org_name: "Acme".into(),
            office_id: "office-1".into(),
            office_name: "HQ".into(),
            role: OfficeRole::Hq,
            app_token: token(),
            db_key: db_key(),
        })),
        (2, 1) => Request::V2(HostMessage::Start(StartOffice {
            app_token: token(),
            db_key: db_key(),
        })),
        (2, _) => Request::V2(HostMessage::Discard(DiscardOffice {
            db_key: key.then(db_key),
        })),
        (3, 0) => Request::V3 {
            line: HostFirstLine::Create(create()),
            handoff: Handoff::Create {
                db_key: db_key(),
                grant_key: grant(),
                recovery_code: RecoveryCode::new("RECOVERY-TEST".into()).unwrap(),
                fds: vec![HandoffFd::Transfer],
            },
        },
        (3, 1) => Request::V3 {
            line: HostFirstLine::Start(office()),
            handoff: Handoff::Start {
                db_key: db_key(),
                grant_key: grant(),
                fds: vec![HandoffFd::Transfer],
            },
        },
        _ => Request::V3 {
            line: HostFirstLine::Discard(office()),
            handoff: Handoff::Discard {
                db_key: key.then(db_key),
                fds: vec![],
            },
        },
    }
}
fn assert_auth(auth: &LaunchAuth, version: u8) {
    match (auth, version) {
        (LaunchAuth::Legacy(actual), 2) => assert_eq!(actual, &token()),
        (LaunchAuth::Grants { _key }, 3) => assert_eq!(_key, &grant()),
        _ => panic!("wrong authority kind"),
    }
}
#[test]
fn versions_preserve_create_identity_keys_and_typed_recovery() {
    for version in [2, 3] {
        let Launch::Create(c) = Launch::from_request(request(version, 0, true)).unwrap() else {
            panic!("not create")
        };
        assert_eq!(
            c.office.office_id,
            if version == 2 { "office-1" } else { "OFFICE1" }
        );
        assert_eq!(c.office.office_name, "HQ");
        assert_eq!(c.office.org_id, if version == 2 { "org-2" } else { "ORG2" });
        assert_eq!(c.office.org_name, "Acme");
        assert_eq!(c.office.role, org_office::OfficeRole::Hq);
        assert_eq!(c.role, OfficeRole::Hq);
        assert!(c.key == Key::from_db_key(db_key()));
        assert_auth(&c.auth, version);
        assert_eq!(
            c.instance.as_deref(),
            if version == 3 { Some(INSTANCE) } else { None }
        );
        assert_eq!(
            c.owner_member_id.as_deref(),
            (version == 3).then_some(OWNER)
        );
        match (c._recovery, version) {
            (CreationRecovery::Legacy, 2) => {}
            (CreationRecovery::V3 { _code }, 3) => assert_eq!(_code.expose(), "RECOVERY-TEST"),
            _ => panic!("lost typed recovery"),
        }
    }
}
#[test]
fn versions_preserve_start_keys_authority_and_instance() {
    for version in [2, 3] {
        let Launch::Start { key, auth, office } =
            Launch::from_request(request(version, 1, true)).unwrap()
        else {
            panic!("not start")
        };
        assert!(key == Key::from_db_key(db_key()));
        assert_auth(&auth, version);
        assert_eq!(
            office,
            (version == 3).then(|| OfficeIdentity {
                office_id: "OFFICE1".into(),
                instance_id: INSTANCE.into(),
            })
        );
    }
}
#[test]
fn versions_preserve_discard_null_and_present_key() {
    for version in [2, 3] {
        for present in [false, true] {
            let Launch::Discard { key, office } =
                Launch::from_request(request(version, 2, present)).unwrap()
            else {
                panic!("not discard")
            };
            assert!(key == present.then(|| Key::from_db_key(db_key())));
            assert_eq!(
                office,
                (version == 3).then(|| OfficeIdentity {
                    office_id: "OFFICE1".into(),
                    instance_id: INSTANCE.into(),
                })
            );
        }
    }
}
#[test]
fn unknown_or_mismatched_requests_never_normalize() {
    for line in [
        HostFirstLine::Create(create()),
        HostFirstLine::Start(office()),
        HostFirstLine::Unknown,
    ] {
        let request = Request::V3 {
            line,
            handoff: Handoff::Discard {
                db_key: None,
                fds: vec![],
            },
        };
        assert!(matches!(
            Launch::from_request(request),
            Err(LaunchError::Invalid("unknown launch request"))
        ));
    }
}

#[test]
fn every_mismatched_v3_pair_and_unknown_legacy_request_is_refused() {
    assert!(Launch::from_request(Request::V2(HostMessage::Unknown)).is_err());
    for first_op in 0..3 {
        for handoff_op in 0..3 {
            if first_op == handoff_op {
                continue;
            }
            let Request::V3 { line, .. } = request(3, first_op, true) else {
                unreachable!()
            };
            let Request::V3 { handoff, .. } = request(3, handoff_op, true) else {
                unreachable!()
            };
            assert!(Launch::from_request(Request::V3 { line, handoff }).is_err());
        }
    }
}

#[test]
fn create_preserves_unsupported_role_for_the_workflow_to_refuse() {
    for version in [2, 3] {
        for role in [OfficeRole::Satellite, OfficeRole::Unknown] {
            let mut request = request(version, 0, true);
            match &mut request {
                Request::V2(HostMessage::Create(c)) => c.role = role,
                Request::V3 {
                    line: HostFirstLine::Create(c),
                    ..
                } => c.role = role,
                _ => unreachable!(),
            }
            let Launch::Create(c) = Launch::from_request(request).unwrap() else {
                panic!("not create")
            };
            assert_eq!(c.role, role);
        }
    }
}

#[test]
fn distinct_v3_identities_are_preserved_on_every_operation() {
    const OTHER_INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAZ";
    const OTHER_OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
    for op in 0..3 {
        let Request::V3 { mut line, handoff } = request(3, op, true) else {
            unreachable!()
        };
        let office = match &mut line {
            HostFirstLine::Create(c) => {
                c.owner_member_id = OTHER_OWNER.into();
                &mut c.office
            }
            HostFirstLine::Start(office) | HostFirstLine::Discard(office) => office,
            _ => unreachable!(),
        };
        office.office_id = "OFFICE2".into();
        office.instance_id = OTHER_INSTANCE.into();
        match Launch::from_request(Request::V3 { line, handoff }).unwrap() {
            Launch::Create(c) => {
                assert_eq!(c.office.office_id, "OFFICE2");
                assert_eq!(c.instance.as_deref(), Some(OTHER_INSTANCE));
                assert_eq!(c.owner_member_id.as_deref(), Some(OTHER_OWNER));
            }
            Launch::Start { office, .. } | Launch::Discard { office, .. } => {
                assert_eq!(
                    office,
                    Some(OfficeIdentity {
                        office_id: "OFFICE2".into(),
                        instance_id: OTHER_INSTANCE.into(),
                    })
                );
            }
        }
    }
}
