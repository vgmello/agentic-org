use super::*;
use org_office_launch::{CreateLine, HandoffFd, OfficeLine};
use org_proto::{GrantKey, OfficeSecretsKey};

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182";
const OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
fn office_secrets_key() -> OfficeSecretsKey {
    OfficeSecretsKey::from_bytes([0xb3; 32])
}
fn grant() -> GrantKey {
    GrantKey::from_bytes([7; 32])
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
        hq_creation: None,
    }
}
fn request(op: u8, key: bool) -> Request {
    match op {
        0 => Request::Launch {
            line: HostFirstLine::Create(create()),
            handoff: Handoff::Create {
                office_secrets_key: office_secrets_key(),
                grant_key: grant(),
                recovery_code: key.then(|| RecoveryCode::new("RECOVERY-TEST".into()).unwrap()),
                fds: vec![HandoffFd::Transfer],
            },
        },
        1 => Request::Launch {
            line: HostFirstLine::Start(office()),
            handoff: Handoff::Start {
                office_secrets_key: key.then(office_secrets_key),
                grant_key: grant(),
                fds: vec![HandoffFd::Transfer],
            },
        },
        _ => Request::Launch {
            line: HostFirstLine::Discard(office()),
            handoff: Handoff::Discard { fds: vec![] },
        },
    }
}
#[test]
fn launch_create_preserves_identity_keys_and_typed_recovery() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let Launch::Create(c) = Launch::from_request(request(0, true)).unwrap() else {
        panic!("not create")
    };
    assert_eq!(c.office.office_id, "OFFICE1");
    assert_eq!(c.office.office_name, "HQ");
    assert_eq!(c.office.org_id, "ORG2");
    assert_eq!(c.office.org_name, "Acme");
    assert_eq!(c.office.role, org_office::OfficeRole::Hq);
    assert_eq!(c.role, OfficeRole::Hq);
    assert!(c.key == Key::from_office_secrets_key(office_secrets_key()));
    assert_eq!(c.auth.grant_key(), &grant());
    assert_eq!(c.instance, INSTANCE);
    assert_eq!(c.owner_member_id, OWNER);
    assert_eq!(c.recovery.unwrap().expose(), "RECOVERY-TEST");
    // A create handoff without a recovery code reaches the workflow as `None`, which refuses it.
    let Launch::Create(c) = Launch::from_request(request(0, false)).unwrap() else {
        panic!("not create")
    };
    assert!(c.recovery.is_none());
}
#[test]
fn start_preserves_keys_authority_and_instance() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let Launch::Start { key, auth, office } = Launch::from_request(request(1, true)).unwrap()
    else {
        panic!("not start")
    };
    assert!(key == Some(Key::from_office_secrets_key(office_secrets_key())));
    assert_eq!(auth.grant_key(), &grant());
    assert_eq!(
        office,
        OfficeIdentity {
            office_id: "OFFICE1".into(),
            instance_id: INSTANCE.into(),
        }
    );
}
#[test]
fn a_start_without_the_office_secrets_key_proceeds_with_none() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let Launch::Start { key, auth, office } = Launch::from_request(request(1, false)).unwrap()
    else {
        panic!("not start")
    };
    assert!(key.is_none());
    assert_eq!(auth.grant_key(), &grant());
    assert_eq!(office.office_id, "OFFICE1");
}
#[test]
fn discard_carries_no_secret() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let Launch::Discard { office } = Launch::from_request(request(2, false)).unwrap() else {
        panic!("not discard")
    };
    assert_eq!(
        office,
        OfficeIdentity {
            office_id: "OFFICE1".into(),
            instance_id: INSTANCE.into(),
        }
    );
}
#[test]
fn unknown_or_mismatched_requests_never_normalize() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for line in [
        HostFirstLine::Create(create()),
        HostFirstLine::Start(office()),
        HostFirstLine::Unknown,
    ] {
        let request = Request::Launch {
            line,
            handoff: Handoff::Discard { fds: vec![] },
        };
        assert!(matches!(
            Launch::from_request(request),
            Err(LaunchError::Invalid("unknown launch request"))
        ));
    }
}

#[test]
fn every_mismatched_launch_pair_is_refused() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for first_op in 0..3 {
        for handoff_op in 0..3 {
            if first_op == handoff_op {
                continue;
            }
            let Request::Launch { line, .. } = request(first_op, true);
            let Request::Launch { handoff, .. } = request(handoff_op, true);
            assert!(Launch::from_request(Request::Launch { line, handoff }).is_err());
        }
    }
}

#[test]
fn create_preserves_unsupported_role_for_the_workflow_to_refuse() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    {
        for role in [OfficeRole::Satellite, OfficeRole::Unknown] {
            let mut request = request(0, true);
            match &mut request {
                Request::Launch {
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
fn distinct_launch_identities_are_preserved_on_every_operation() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    const OTHER_INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607184";
    const OTHER_OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
    for op in 0..3 {
        let Request::Launch { mut line, handoff } = request(op, true);
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
        match Launch::from_request(Request::Launch { line, handoff }).unwrap() {
            Launch::Create(c) => {
                assert_eq!(c.office.office_id, "OFFICE2");
                assert_eq!(c.instance, OTHER_INSTANCE);
                assert_eq!(c.owner_member_id, OTHER_OWNER);
            }
            Launch::Start { office, .. } | Launch::Discard { office, .. } => {
                assert_eq!(
                    office,
                    OfficeIdentity {
                        office_id: "OFFICE2".into(),
                        instance_id: OTHER_INSTANCE.into(),
                    }
                );
            }
        }
    }
}
