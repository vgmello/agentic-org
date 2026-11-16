//! Actual selected HTTP adapter + approved native client transport. This fixed
//! peer fixture checks protocol mapping; real account/storage evidence is the
//! separately executed protected host/office/browser journey.
use super::*;
use org_proto::{
    frame, Grant, GrantClaims, GrantMac, Hello, HelloResult, OfficeHello, OfficeInfo, OfficeRole,
    OfficeStatus, Response,
};
use std::{io::BufReader, os::unix::net::UnixStream};
struct Authorized {
    office: Arc<ApprovedOffice>,
}
impl SessionAuthority for Authorized {
    type Ticket = ();
    fn authenticate(&self, _: &Parts) -> Result<(), ApiReply> {
        Ok(())
    }
    fn office(&self, _: (), scope: &OfficeScope, _: Duration) -> Result<OfficeAccess, ApiReply> {
        assert_eq!(scope.org_id, "org");
        Ok(OfficeAccess {
            office: self.office.clone(),
            principal: Principal {
                grant: Grant::new(
                    GrantClaims {
                        account: "foreign-linked-account".into(),
                        member: INSTANCE.into(),
                        org_id: "org".into(),
                        office_id: "office".into(),
                        channel_id: "channel".into(),
                        session_id: "session".into(),
                        exp: u64::MAX,
                    },
                    GrantMac::from_bytes([17; 32]),
                ),
            },
        })
    }
}
#[test]
fn foreign_or_missing_native_receipt_is_exact_http_not_found_without_remapping_auth_failure() {
    for code in [
        org_proto::ErrorCode::NotFound,
        org_proto::ErrorCode::NotAuthorized,
    ] {
        let (client, server) = UnixStream::pair().unwrap();
        let peer = std::thread::spawn(move || {
            let mut reader = BufReader::new(server);
            let hello: org_proto::Request = frame::read_message(&mut reader).unwrap().unwrap();
            assert!(hello.is::<Hello>());
            let mut result = HelloResult::office(
                "fixture",
                "This Mac",
                OfficeHello {
                    organization_name: "Org".into(),
                    office: OfficeInfo {
                        id: "office".into(),
                        name: "HQ".into(),
                        role: OfficeRole::Hq,
                        organization_id: "org".into(),
                        host: "This Mac".into(),
                        status: OfficeStatus::Active,
                        status_reason: None,
                        next_restart_at: None,
                        endpoint: None,
                    },
                },
            )
            .with_instance_id(INSTANCE);
            result.api_version = 2;
            frame::write_message(
                reader.get_mut(),
                &Response::ok::<Hello>(hello.id, &result).unwrap(),
            )
            .unwrap();
            let request: org_proto::Request = frame::read_message(&mut reader).unwrap().unwrap();
            assert!(request.is::<org_proto::selected_tasks::Lookup>());
            let principal: Principal =
                serde_json::from_value(request.principal.as_ref().unwrap().clone()).unwrap();
            assert_eq!(principal.grant.claims.account, "foreign-linked-account");
            assert_eq!(
                request
                    .params::<org_proto::selected_tasks::Lookup>()
                    .unwrap()
                    .id,
                INSTANCE
            );
            frame::write_message(
                reader.get_mut(),
                &Response::error(
                    request.id,
                    org_proto::ApiError::new(code, "private native reason must not escape"),
                ),
            )
            .unwrap();
        });
        let office = ApprovedOffice::from_descriptor(
            client.into(),
            INSTANCE.into(),
            "org".into(),
            "office".into(),
            "channel".into(),
        )
        .unwrap();
        let api = SelectedApi::new(Authorized {
            office: Arc::new(office),
        });
        let path = format!(
            "/api/v1/operations/work/{INSTANCE}?instance_id={INSTANCE}&org_id=org&office_id=office"
        );
        let (parts, _) = HttpRequest::builder()
            .method(Method::GET)
            .uri(path)
            .body(())
            .unwrap()
            .into_parts();
        let route = api
            .route(&parts, &RequestPath::parse(parts.uri.path()).unwrap())
            .unwrap_or_else(|_| panic!("route"));
        let prepared = api
            .prepare(route, body(b""))
            .unwrap_or_else(|_| panic!("prepare"));
        let mut response = None;
        let _ = prepared.map(|call| response = Some(api.execute(call)));
        let reply = response.expect("adapter executed");
        if code == org_proto::ErrorCode::NotFound {
            assert_eq!(reply.inspected(), missing().inspected());
            assert_eq!(reply.inspected().0, StatusCode::NOT_FOUND);
        } else {
            assert_eq!(reply.inspected().0, StatusCode::FORBIDDEN);
        }
        assert!(!std::str::from_utf8(reply.inspected().1)
            .unwrap()
            .contains("private native reason"));
        peer.join().unwrap();
    }
}
