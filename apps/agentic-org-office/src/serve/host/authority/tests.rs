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
