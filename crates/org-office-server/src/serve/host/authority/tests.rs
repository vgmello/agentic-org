use super::*;
use org_proto::{GrantClaims, GrantMac, Principal};
use std::cell::RefCell;

struct Probe {
    valid: bool,
    now: Option<u64>,
    revoked: bool,
    owner: Option<String>,
    calls: RefCell<Vec<&'static str>>,
}
impl Default for Probe {
    fn default() -> Self {
        Self {
            valid: true,
            now: Some(9),
            revoked: false,
            owner: Some("owner".into()),
            calls: RefCell::default(),
        }
    }
}
impl Checks for Probe {
    fn verify(&self, _: &Grant) -> bool {
        self.calls.borrow_mut().push("verify");
        self.valid
    }
    fn now_ms(&self) -> Option<u64> {
        self.calls.borrow_mut().push("clock");
        self.now
    }
    fn revoked(&self, session: &str) -> bool {
        assert_eq!(session, "session");
        self.calls.borrow_mut().push("revoked");
        self.revoked
    }
    fn owner(&self, member: &str) -> Option<String> {
        assert_eq!(member, "owner");
        self.calls.borrow_mut().push("owner");
        self.owner.clone()
    }
}
fn claims() -> GrantClaims {
    GrantClaims {
        account: "account".into(),
        member: "owner".into(),
        org_id: "org".into(),
        office_id: "office".into(),
        channel_id: "channel".into(),
        session_id: "session".into(),
        exp: 10,
    }
}
fn request(claims: GrantClaims) -> Request {
    Request {
        id: 7,
        method: "org.snapshot".into(),
        params: serde_json::json!({}),
        principal: Some(
            serde_json::to_value(Principal {
                grant: Grant::new(claims, GrantMac::from_bytes([0; 32])),
            })
            .unwrap(),
        ),
    }
}
fn office() -> OfficeInfo {
    serde_json::from_value(serde_json::json!({"id":"office","organization_id":"org","name":"HQ","role":"hq","host":"test","status":"active"})).unwrap()
}
#[test]
fn authority_checks_stop_at_each_failure_in_frozen_order() {
    let cases = [
        (
            Probe {
                valid: false,
                ..Probe::default()
            },
            ErrorCode::NotAuthorized,
            vec!["verify"],
        ),
        (
            Probe {
                now: Some(10),
                ..Probe::default()
            },
            ErrorCode::NotAuthorized,
            vec!["verify", "clock"],
        ),
        (
            Probe {
                now: None,
                ..Probe::default()
            },
            ErrorCode::NotAuthorized,
            vec!["verify", "clock"],
        ),
        (
            Probe {
                revoked: true,
                ..Probe::default()
            },
            ErrorCode::NotAuthorized,
            vec!["verify", "clock", "revoked"],
        ),
        (
            Probe {
                owner: None,
                ..Probe::default()
            },
            ErrorCode::NotAuthorized,
            vec!["verify", "clock", "revoked", "owner"],
        ),
    ];
    for (checks, error, calls) in cases {
        assert_eq!(
            authorize(&request(claims()), "channel", &office(), &checks),
            Err(error)
        );
        assert_eq!(*checks.calls.borrow(), calls);
    }
    let checks = Probe::default();
    assert_eq!(
        authorize(&request(claims()), "different", &office(), &checks),
        Err(ErrorCode::NotAuthorized)
    );
    assert!(checks.calls.borrow().is_empty());
    let checks = Probe::default();
    assert_eq!(
        authorize(&request(claims()), "channel", &office(), &checks),
        Ok("owner".into())
    );
    assert_eq!(
        *checks.calls.borrow(),
        ["verify", "clock", "revoked", "owner"]
    );
}
#[test]
fn scope_clauses_and_expiry_precedence_do_not_touch_later_state() {
    for field in ["office", "org"] {
        for (expired, revoked, owner) in [
            (true, false, true),
            (false, false, true),
            (false, true, true),
            (false, false, false),
        ] {
            let mut claim = claims();
            if field == "office" {
                claim.office_id = "other".into();
            } else {
                claim.org_id = "other".into();
            }
            let checks = Probe {
                now: Some(if expired { 10 } else { 9 }),
                revoked,
                owner: owner.then(|| "owner".into()),
                ..Probe::default()
            };
            let expected = if expired {
                ErrorCode::NotAuthorized
            } else {
                ErrorCode::NotFound
            };
            assert_eq!(
                authorize(&request(claim), "channel", &office(), &checks),
                Err(expected)
            );
            assert_eq!(*checks.calls.borrow(), ["verify", "clock"]);
        }
    }
}
#[test]
fn missing_null_or_malformed_principals_run_no_checks() {
    for principal in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!({"grant":{}})),
        Some(serde_json::json!([])),
    ] {
        let mut request = request(claims());
        request.principal = principal;
        let checks = Probe::default();
        assert_eq!(
            authorize(&request, "channel", &office(), &checks),
            Err(ErrorCode::NotAuthorized)
        );
        assert!(checks.calls.borrow().is_empty());
    }
}

#[test]
fn expiry_is_strict_at_zero_adjacent_and_maximum_boundaries() {
    for (exp, now, accepted) in [
        (0, 0, false),
        (1, 0, true),
        (1, 1, false),
        (u64::MAX, u64::MAX - 1, true),
        (u64::MAX, u64::MAX, false),
    ] {
        let mut claim = claims();
        claim.exp = exp;
        let checks = Probe {
            now: Some(now),
            ..Probe::default()
        };
        let response = authorize(&request(claim), "channel", &office(), &checks);
        assert_eq!(response.is_ok(), accepted, "exp={exp}, now={now}");
        assert_eq!(
            *checks.calls.borrow(),
            if accepted {
                vec!["verify", "clock", "revoked", "owner"]
            } else {
                vec!["verify", "clock"]
            }
        );
    }
}

#[test]
fn subscription_usage_methods_require_owner_at_host_authority() {
    for method in [
        "cost.subscription_usage.list",
        "cost.subscription_usage.collect",
        "cost.subscription_usage.credential_set",
        "cost.subscription_usage.credential_revoke",
    ] {
        let mut req = request(claims());
        req.method = method.into();
        let checks = Probe {
            owner: None,
            ..Probe::default()
        };
        assert_eq!(
            authorize(&req, "channel", &office(), &checks),
            Err(ErrorCode::NotAuthorized),
            "non-owner must be rejected for {method}"
        );
        let owner_checks = Probe::default();
        assert_eq!(
            authorize(&req, "channel", &office(), &owner_checks),
            Ok("owner".into()),
            "owner must be allowed for {method}"
        );
    }
}

#[test]
fn satellite_operator_grant_allows_only_sync_status_on_satellite_after_security_checks() {
    let satellite: OfficeInfo = serde_json::from_value(serde_json::json!({
        "id": "office",
        "organization_id": "org",
        "name": "Berlin",
        "role": "satellite",
        "host": "test",
        "status": "active"
    }))
    .unwrap();
    let mut sat_claims = claims();
    sat_claims.member = "office".into();

    // Read-only `office.sync_status` succeeds on a satellite after verify + clock + revoked.
    let mut sync_req = request(sat_claims.clone());
    sync_req.method = "office.sync_status".into();
    let checks = Probe {
        owner: None,
        ..Probe::default()
    };
    assert_eq!(
        authorize(&sync_req, "channel", &satellite, &checks),
        Ok("office".into())
    );
    assert_eq!(*checks.calls.borrow(), ["verify", "clock", "revoked"]);

    // Expired or revoked satellite operator grant is still refused.
    let expired_checks = Probe {
        now: Some(10),
        owner: None,
        ..Probe::default()
    };
    assert_eq!(
        authorize(&sync_req, "channel", &satellite, &expired_checks),
        Err(ErrorCode::NotAuthorized)
    );
    let revoked_checks = Probe {
        revoked: true,
        owner: None,
        ..Probe::default()
    };
    assert_eq!(
        authorize(&sync_req, "channel", &satellite, &revoked_checks),
        Err(ErrorCode::NotAuthorized)
    );

    // Every other method (org reads, projects, invitations, mutations) is refused for an unlinked
    // satellite operator grant because `office` is not a live person or owner.
    struct DenyNonMember;
    impl Checks for DenyNonMember {
        fn verify(&self, _: &Grant) -> bool {
            true
        }
        fn now_ms(&self) -> Option<u64> {
            Some(9)
        }
        fn revoked(&self, _: &str) -> bool {
            false
        }
        fn owner(&self, _: &str) -> Option<String> {
            None
        }
    }
    for method in [
        "office.directory",
        "provider.connection.list",
        "provider.binding.list",
        "provider.binding.set",
        "provider.connection.refresh",
        "model_profile.list",
    ] {
        let mut req = request(sat_claims.clone());
        req.method = method.into();
        assert_eq!(
            authorize(&req, "channel", &satellite, &DenyNonMember),
            Ok("office".into()),
            "satellite operator grant must authorize local/read-only {method}"
        );
    }

    for method in [
        "org.snapshot",
        "project.list",
        "who",
        "team.create",
        "provider.connection.create",
        "office.invitation.list",
        "office.remove",
    ] {
        let mut req = request(sat_claims.clone());
        req.method = method.into();
        assert_eq!(
            authorize(&req, "channel", &satellite, &DenyNonMember),
            Err(ErrorCode::NotAuthorized),
            "satellite operator grant must NOT authorize {method}"
        );
    }

    // On HQ, `claims.member == office.id` does NOT bypass live person verification even for sync_status.
    assert_eq!(
        authorize(&sync_req, "channel", &office(), &DenyNonMember),
        Err(ErrorCode::NotAuthorized)
    );
}

/// A live person who is not the owner, or nobody.
struct PersonOnly(Option<String>);
impl Checks for PersonOnly {
    fn verify(&self, _: &Grant) -> bool {
        true
    }
    fn now_ms(&self) -> Option<u64> {
        Some(9)
    }
    fn revoked(&self, _: &str) -> bool {
        false
    }
    fn owner(&self, _: &str) -> Option<String> {
        None
    }
    fn person(&self, _: &str) -> Option<String> {
        self.0.clone()
    }
}

#[test]
fn attachment_methods_are_person_scoped_never_owner_only() {
    for method in org_proto::attachments::ATTACHMENT_METHODS {
        assert!(!requires_owner(method), "{method}");
        assert!(is_member_scoped_method(method), "{method}");
        let mut req = request(claims());
        req.method = method.into();
        assert_eq!(
            authorize(
                &req,
                "channel",
                &office(),
                &PersonOnly(Some("person".into()))
            ),
            Ok("person".into()),
            "any live person may call {method}"
        );
        assert_eq!(
            authorize(&req, "channel", &office(), &PersonOnly(None)),
            Err(ErrorCode::NotAuthorized),
            "{method} needs a live person"
        );
    }
    assert!(!is_member_scoped_method("attachment.future"));
}

#[test]
fn office_metrics_requires_owner_at_host_authority() {
    assert!(requires_owner("office.metrics"));
    let mut req = request(claims());
    req.method = "office.metrics".into();

    // Non-owner live person must be rejected with NotAuthorized.
    assert_eq!(
        authorize(
            &req,
            "channel",
            &office(),
            &PersonOnly(Some("person".into()))
        ),
        Err(ErrorCode::NotAuthorized),
    );
    // Unlinked / non-owner grant must be rejected.
    let non_owner = Probe {
        owner: None,
        ..Probe::default()
    };
    assert_eq!(
        authorize(&req, "channel", &office(), &non_owner),
        Err(ErrorCode::NotAuthorized),
    );
    // Revoked owner grant must be rejected.
    let revoked = Probe {
        revoked: true,
        ..Probe::default()
    };
    assert_eq!(
        authorize(&req, "channel", &office(), &revoked),
        Err(ErrorCode::NotAuthorized),
    );
    // Valid owner grant succeeds.
    assert_eq!(
        authorize(&req, "channel", &office(), &Probe::default()),
        Ok("owner".into()),
    );
}
