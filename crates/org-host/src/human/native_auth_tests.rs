//! Real native integration assertions, invoked after protected first bootstrap.
//! Additional accounts/authority changes are fixture-only protected SQL facts;
//! there is no test account-management operation in any production service.
use super::*;
use crate::{
    accounts::identity::AccountId,
    hostdb::{FirstAccount, HostDb},
};
use org_proto::{
    selected_tasks::{self as task, read},
    ApiError, ErrorCode,
};
use std::fs;

fn call<C: Call>(
    wire: &mut Wire,
    params: &C::Params,
    principal: Option<&Principal>,
) -> Result<C::Result, ApiError> {
    wire.id += 1;
    let mut request = org_proto::Request::new::<C>(wire.id, params).unwrap();
    if let Some(p) = principal {
        request.principal = Some(serde_json::to_value(p).unwrap());
    }
    org_proto::frame::write_message(&mut wire.write, &request).unwrap();
    let mut bytes = Vec::new();
    loop {
        let (byte, fds) = wire.byte(0).unwrap();
        assert!(fds.is_empty());
        if byte == b'\n' {
            break;
        }
        assert!(bytes.len() < org_proto::MAX_FRAME_BYTES);
        bytes.push(byte);
    }
    let response: Response = serde_json::from_slice(&bytes).unwrap();
    crate::secrets::zero(&mut bytes);
    assert_eq!(response.id, wire.id);
    response.into_result::<C>().map_err(|e| match e {
        org_proto::CallError::Api(e) => e,
        _ => panic!("invalid typed native response"),
    })
}
fn signed_in(socket: &Path, login: &str, password: &str) -> (Wire, ops::AccountVerifyResult) {
    let mut wire = Wire::new(UnixStream::connect(socket).unwrap()).unwrap();
    wire.call::<Hello>(
        &HelloParams::offering(org_host_control::SUPPORTED_API, "native-auth-test"),
        None,
    )
    .unwrap();
    let account = call::<ops::AccountVerify>(
        &mut wire,
        &ops::AccountVerifyParams {
            login: login.into(),
            password: Password::new(password.into()).unwrap(),
        },
        None,
    )
    .unwrap();
    (wire, account)
}
fn channel(host: &mut Wire, account: &ops::AccountVerifyResult, office: &str) -> (Wire, Principal) {
    let opened = call::<ops::OfficeOpenChannel>(
        host,
        &ops::OfficeOpenChannelParams {
            session: session(&account.session),
            office_id: office.into(),
        },
        None,
    )
    .unwrap();
    let (marker, mut fds) = host.byte(1).unwrap();
    assert_eq!(marker, ops::HOST_SOCK_MARKER);
    let mut worker =
        Wire::new(org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap()).unwrap();
    call::<ops::FdsReceived>(
        host,
        &ops::FdsReceivedParams {
            of: opened.channel_id.clone(),
        },
        None,
    )
    .unwrap();
    call::<Hello>(
        &mut worker,
        &HelloParams::offering(org_proto::ApiRange { min: 2, max: 2 }, "native-auth-test"),
        None,
    )
    .unwrap();
    let grant = call::<ops::SessionGrant>(
        host,
        &ops::SessionGrantParams {
            session: session(&account.session),
            office_id: office.into(),
            channel_id: opened.channel_id,
        },
        None,
    )
    .unwrap()
    .grant;
    (worker, Principal { grant })
}
pub(crate) fn exercise(
    root: &Path,
    socket: &Path,
    instance: &str,
    office: &org_host_control::provision::Provisioned,
    public: &Path,
    password: &str,
    db: &HostDb,
) {
    let (task_id, operation) = super::tests::adopt_and_assign_with_password(
        root, socket, instance, office, public, password,
    );
    let (mut first, a) = signed_in(socket, "Owner", password);
    let (mut second, b) = signed_in(socket, "Owner", password);
    assert_eq!(a.account, b.account);
    assert_ne!(a.session.expose(), b.session.expose());
    let (mut first_worker, first_principal) = channel(&mut first, &a, &office.office_id);
    let (mut second_worker, second_principal) = channel(&mut second, &b, &office.office_id);
    assert_ne!(
        first_principal.grant.claims.session_id,
        second_principal.grant.claims.session_id
    );
    let read::Result::Detail { task: card, .. } = call::<read::Read>(
        &mut first_worker,
        &read::Query::Detail {
            task_id: task_id.clone(),
        },
        Some(&first_principal),
    )
    .unwrap() else {
        panic!("detail omitted")
    };
    assert_eq!(card.revision, 2);
    let foreign_id = crate::ids::generate().unwrap();
    let foreign_password = format!("generated-{}", crate::ids::generate().unwrap());
    db.additional_account_fixture(
        &FirstAccount::new(
            &foreign_id,
            "Foreign",
            crate::service::authentication::hash_password(
                &Password::new(foreign_password.clone()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let (mut foreign, f) = signed_in(socket, "Foreign", &foreign_password);
    assert!(!f.admin && f.orgs.is_empty());
    assert_eq!(
        call::<ops::OfficeOpenChannel>(
            &mut foreign,
            &ops::OfficeOpenChannelParams {
                session: session(&f.session),
                office_id: office.office_id.clone()
            },
            None
        )
        .err()
        .unwrap()
        .code,
        ErrorCode::NotFound
    );
    // A real link and valid host-signed grant still confer no owner authority.
    db.link_account_fixture(
        &AccountId::parse(&foreign_id).unwrap(),
        &office.organization_id,
        &crate::ids::generate().unwrap(),
    )
    .unwrap();
    let (mut foreign_worker, foreign_principal) = channel(&mut foreign, &f, &office.office_id);
    assert_eq!(foreign_principal.grant.claims.account, foreign_id);
    for query in [
        read::Query::List {
            after: None,
            queue: read::Queue::All,
            limit: 1,
        },
        read::Query::Detail {
            task_id: task_id.clone(),
        },
        read::Query::History {
            task_id: task_id.clone(),
            after: None,
            limit: 1,
        },
        read::Query::Chunk {
            task_id: task_id.clone(),
            reference: card.snapshot.clone(),
            offset: 0,
            limit: 256,
        },
    ] {
        assert_eq!(
            call::<read::Read>(&mut foreign_worker, &query, Some(&foreign_principal))
                .err()
                .unwrap()
                .code,
            ErrorCode::NotAuthorized
        );
    }
    let lookup = org_proto::OperationGetParams {
        kind: org_proto::OperationKind::Work,
        id: operation.as_str().into(),
    };
    assert_eq!(
        call::<task::Lookup>(&mut foreign_worker, &lookup, Some(&foreign_principal))
            .err()
            .unwrap()
            .code,
        ErrorCode::NotFound
    );
    assert!(matches!(
        call::<task::Lookup>(&mut second_worker, &lookup, Some(&second_principal)).unwrap(),
        task::LookupOutcome::Committed {
            result: task::AssignmentResult::Completed { revision: 2, .. }
        }
    ));
    // Account-wide revocation must reach both real Office channels without
    // another sign-in/renewal. Accepted canonical work remains unchanged.
    db.change_authority_fixture(&AccountId::parse(&a.account).unwrap(), true)
        .unwrap();
    assert_both_grants_revoked(
        (&mut first_worker, &first_principal),
        (&mut second_worker, &second_principal),
        &task_id,
    );
    let final_bytes = fs::read_to_string(public.join(format!(
        "teams/{}/work/tasks/{task_id}.md",
        office.home_team_id
    )))
    .unwrap();
    assert!(final_bytes.contains("revision: 2\n"));
    assert!(final_bytes.ends_with("Original café body.\n"));
}

fn assert_both_grants_revoked(
    first: (&mut Wire, &Principal),
    second: (&mut Wire, &Principal),
    task_id: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let one = call::<read::Read>(
            first.0,
            &read::Query::Detail {
                task_id: task_id.into(),
            },
            Some(first.1),
        );
        let two = call::<read::Read>(
            second.0,
            &read::Query::Detail {
                task_id: task_id.into(),
            },
            Some(second.1),
        );
        let one = one.expect_err("revoked registry authority cannot return task data");
        let two = two.expect_err("revoked registry authority cannot return task data");
        if one.code == ErrorCode::NotAuthorized && two.code == ErrorCode::NotAuthorized {
            break;
        }
        // The writer rechecks protected owner authority before the asynchronous
        // session lifecycle transfer has necessarily reached this channel. Its
        // unavailable fence is not evidence that this grant was revoked. Keep
        // waiting for the explicit signed-session denial on BOTH channels.
        assert!(matches!(
            one.code,
            ErrorCode::NotAuthorized | ErrorCode::Unavailable
        ));
        assert!(matches!(
            two.code,
            ErrorCode::NotAuthorized | ErrorCode::Unavailable
        ));
        assert!(
            Instant::now() < deadline,
            "both issued grants must become explicitly unauthorized; observed {:?}/{:?}",
            one.code,
            two.code
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
