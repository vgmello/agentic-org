//! Link issuing rules, run against a real temp registry. Admission (the signing check, local
//! channel) is the same code path `recovery` uses; hello and the peer UID are enforced before
//! dispatch and tested in `service` and `listener`.
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::hostdb::HostDbInit;

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
pub(crate) const OPERATION: &str = "01890a5d-ac96-774b-bcce-b302099a8057";
const PHC: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

struct NoSecrets;
impl crate::host_secrets::HostSecrets for NoSecrets {
    fn put(
        &self,
        _: &crate::host_secrets::Name,
        _: &crate::host_secrets::Value,
    ) -> Result<(), crate::host_secrets::Error> {
        Ok(())
    }
    fn load(
        &self,
        _: &crate::host_secrets::Name,
    ) -> Result<Option<crate::host_secrets::Value>, crate::host_secrets::Error> {
        Ok(None)
    }
    fn delete(&self, _: &crate::host_secrets::Name) -> Result<(), crate::host_secrets::Error> {
        Ok(())
    }
    fn list(&self) -> Result<Vec<crate::host_secrets::Name>, crate::host_secrets::Error> {
        Ok(Vec::new())
    }
}

struct Refusing;
impl Presence for Refusing {
    fn local(&self, _: &Connection) -> bool {
        false
    }
    fn confirm(&self, _: &dyn Fn() -> bool) -> Result<(), ()> {
        Err(())
    }
}

/// An administrator service over a fresh temp registry, bound to [`OPERATION`]; its presence
/// check refuses.
pub(crate) fn admin() -> (tempfile::TempDir, Admin) {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let init = HostDbInit {
        instance_id: INSTANCE.into(),
        clock_high_ms: 1,
    };
    let db = Arc::new(HostDb::create(home.path(), &NoSecrets, &init).unwrap());
    let policy = AppPolicy::new(crate::HOST_SIGNING_ID, None).unwrap();
    let origin = ListenOrigin::parse("http://127.0.0.1:7420").unwrap();
    let admin = Admin::new(db, OPERATION, origin, policy, Arc::new(Refusing));
    (home, admin)
}

/// The registry behind `admin`, for tests outside this module.
pub(crate) fn registry(admin: &Admin) -> &Arc<HostDb> {
    &admin.db
}

pub(crate) fn seed_administrator(admin: &Admin) {
    admin
        .db
        .seed_account(
            "01J00000000000000000000003",
            "Sage",
            &crate::hostdb::journal::Verifier::new(PHC.into()).unwrap(),
            true,
            123,
        )
        .unwrap();
}

/// The token after `#`, checked against the configured origin.
fn token(result: AdminLinkResult) -> (String, i64) {
    let AdminLinkResult::Issued { url, expires_at_ms } = result else {
        panic!("no link issued: {result:?}");
    };
    let token = url
        .expose()
        .strip_prefix("http://127.0.0.1:7420/register#")
        .expect("link on the configured origin");
    assert_eq!(token.len(), 43);
    (token.to_owned(), expires_at_ms)
}

fn subject() -> Subject {
    Subject::new(SUBJECT).unwrap()
}

#[test]
fn the_initial_link_is_issued_once_and_a_replay_has_no_secret() {
    let (_home, admin) = admin();
    let now = wall().unwrap();
    let (first, expires) = token(admin.initial_link(OPERATION, now).unwrap());
    assert_eq!(expires, now + crate::links::REGISTRATION_TTL_MS);
    for _ in 0..2 {
        let replay = admin.initial_link(OPERATION, now + 1).unwrap();
        assert!(
            matches!(replay, AdminLinkResult::AlreadyIssued),
            "{replay:?}"
        );
        let json = serde_json::to_string(&replay).unwrap();
        assert_eq!(json, r#"{"outcome":"already_issued"}"#);
    }
    // Still refused once the first link is used.
    assert!(admin
        .db
        .consume(LinkKind::Registration, &first, now + 2)
        .unwrap()
        .is_ok());
    assert!(matches!(
        admin.initial_link(OPERATION, now + 3).unwrap(),
        AdminLinkResult::AlreadyIssued
    ));
}

#[test]
fn the_initial_link_is_bound_to_the_setup_operation() {
    let (_home, admin) = admin();
    let other = "01890a5d-ac96-774b-bcce-b302099a8058";
    let error = admin.initial_link(other, 1).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotAuthorized);
    // Nothing was issued, so the right operation still gets the link.
    token(admin.initial_link(OPERATION, 1).unwrap());
}

#[test]
fn no_link_is_issued_while_an_administrator_exists() {
    let (_home, admin) = admin();
    seed_administrator(&admin);
    assert!(matches!(
        admin.initial_link(OPERATION, 1).unwrap(),
        AdminLinkResult::AdministratorPresent
    ));
    let error = admin.reset_with(true, || Ok(())).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(
        admin
            .db
            .pending(LinkKind::Registration, &subject(), 1)
            .unwrap(),
        None
    );
}

#[test]
fn reset_requires_local_presence_and_confirmation() {
    let (_home, admin) = admin();
    let mut asked = false;
    let error = admin
        .reset_with(false, || {
            asked = true;
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotAuthorized);
    assert!(!asked, "no dialog for a remote caller");
    let error = admin.reset_with(true, || Err(())).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotAuthorized);
    assert_eq!(
        admin
            .db
            .pending(LinkKind::Registration, &subject(), 1)
            .unwrap(),
        None
    );
}

#[test]
fn reset_replaces_the_previous_link_and_only_the_newest_redeems() {
    let (_home, admin) = admin();
    let now = wall().unwrap();
    let (initial, _) = token(admin.initial_link(OPERATION, now).unwrap());
    let (second, _) = token(admin.reset_with(true, || Ok(())).unwrap());
    let (third, _) = token(admin.reset_with(true, || Ok(())).unwrap());
    let later = wall().unwrap() + 1;
    let consume = |t: &str| admin.db.consume(LinkKind::Registration, t, later).unwrap();
    assert_eq!(consume(&initial), Err(crate::links::Refused::Cancelled));
    assert_eq!(consume(&second), Err(crate::links::Refused::Cancelled));
    assert!(consume(&third).is_ok());
    assert_eq!(consume(&third), Err(crate::links::Refused::Used));
    // The initial call never issues again after a reset.
    assert!(matches!(
        admin.initial_link(OPERATION, later).unwrap(),
        AdminLinkResult::AlreadyIssued
    ));
}

#[test]
fn a_dialog_that_sees_a_new_administrator_issues_nothing() {
    let (_home, admin) = admin();
    let error = admin
        .reset_with(true, || {
            seed_administrator(&admin);
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(
        admin
            .db
            .pending(LinkKind::Registration, &subject(), 1)
            .unwrap(),
        None
    );
}

#[test]
fn a_reset_refused_for_an_administrator_keeps_the_pending_link() {
    let (_home, admin) = admin();
    let (_, _) = token(admin.reset_with(true, || Ok(())).unwrap());
    let before = admin
        .db
        .pending(LinkKind::Registration, &subject(), 1)
        .unwrap()
        .expect("pending link");
    let error = admin
        .reset_with(true, || {
            seed_administrator(&admin);
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    // The administrator check and the replacement share one transaction: a refused reset
    // neither cancels the pending link nor writes a new one.
    let after = admin
        .db
        .pending(LinkKind::Registration, &subject(), 1)
        .unwrap()
        .expect("still pending");
    assert_eq!(after.link_id, before.link_id);
}

#[test]
fn issue_unless_administrator_checks_and_writes_together() {
    let (_home, admin) = admin();
    let first = admin
        .db
        .issue_unless_administrator(LinkKind::Registration, &subject(), 10)
        .unwrap()
        .expect("issued with no administrator");
    let second = admin
        .db
        .issue_unless_administrator(LinkKind::Registration, &subject(), 20)
        .unwrap()
        .expect("replaces the pending link");
    assert_ne!(first.link_id, second.link_id);
    seed_administrator(&admin);
    assert!(admin
        .db
        .issue_unless_administrator(LinkKind::Registration, &subject(), 30)
        .unwrap()
        .is_none());
    let pending = admin
        .db
        .pending(LinkKind::Registration, &subject(), 31)
        .unwrap()
        .expect("unchanged");
    assert_eq!(pending.link_id, second.link_id);
}

#[test]
fn only_the_local_channel_may_administer_even_when_the_policy_would_allow() {
    use org_office_launch::v3::ChannelKind;
    let mut consulted = false;
    let refused = super::admit_with(ChannelKind::Gateway, || {
        consulted = true;
        true
    });
    assert_eq!(refused.unwrap_err().code, ErrorCode::NotAuthorized);
    assert!(
        !consulted,
        "the signing policy is never consulted off the local channel"
    );
    assert!(super::admit_with(ChannelKind::Local, || true).is_ok());
    assert_eq!(
        super::admit_with(ChannelKind::Local, || false)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
}
