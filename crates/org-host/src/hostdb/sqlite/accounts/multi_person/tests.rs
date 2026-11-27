use crate::accounts::identity::AccountId;
use crate::accounts::rules::Username;
use crate::accounts::store::NewAccount;
use crate::hostdb::journal::Verifier;
use crate::hostdb::lockout::{Outcome, Socket};
use crate::hostdb::sqlite::tests::fixture;
use crate::links::Refused;
use crate::sessions::{BrowserSessions, NewSession, Origin, Scheme, SessionDigest};

use super::*;

const PHC_A: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const PHC_B: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn verifier(phc: &str) -> Verifier {
    Verifier::new(phc.to_owned()).unwrap()
}

#[test]
fn invite_register_attach_and_password_reset_flows() {
    let (_home, _secrets, db) = fixture();
    let owner_id = AccountId::parse(crate::hostdb::sqlite::tests::admin_fixture::ACCOUNT).unwrap();
    let org1 = crate::ids::uuid_v7().unwrap();
    let org2 = crate::ids::uuid_v7().unwrap();
    let owner_member = crate::ids::uuid_v7().unwrap();
    let sam_member = crate::ids::uuid_v7().unwrap();
    let sam_member_org2 = crate::ids::uuid_v7().unwrap();

    db.seed_owner_for_test(owner_id.as_str(), &org1, &owner_member)
        .unwrap();
    db.seed_owner_for_test(owner_id.as_str(), &org2, &owner_member)
        .unwrap();

    // 1. Issue invite, re-issue cancels previous, redeem creates UUIDv7 non-admin account.
    let inv1 = db.issue_org_invite(&org1, &sam_member, 10_000).unwrap();
    let inv2 = db.issue_org_invite(&org1, &sam_member, 10_100).unwrap();
    let access = db.list_org_access(&org1, 10_200).unwrap();
    assert!(access
        .iter()
        .any(|e| e.member_id == sam_member && e.pending_invite && e.account_id.is_none()));

    let new_sam = NewAccount::new(Username::new("Sam").unwrap(), verifier(PHC_A)).unwrap();
    assert!(org_proto::is_uuid_v7(new_sam.account_id.as_str()));
    assert_eq!(
        db.redeem_invite_register(&inv1.token, &new_sam, 10_300)
            .unwrap(),
        InviteRegistration::Link(Refused::Cancelled)
    );
    let reg = db
        .redeem_invite_register(&inv2.token, &new_sam, 10_400)
        .unwrap();
    assert_eq!(
        reg,
        InviteRegistration::Registered {
            account_id: new_sam.account_id.clone(),
            org_id: org1.clone(),
            member_id: sam_member.clone(),
        }
    );
    let sam_id = new_sam.account_id.clone();

    // Already-linked member cannot be invited again.
    assert_eq!(
        db.issue_org_invite(&org1, &sam_member, 10_500).unwrap_err(),
        InviteIssueError::AlreadyLinked
    );

    // 2. SIGN-35: attach Sam's existing account to org2 via invite link.
    let inv_org2 = db
        .issue_org_invite(&org2, &sam_member_org2, 10_600)
        .unwrap();
    assert_eq!(
        db.redeem_invite_attach(&inv_org2.token, &sam_id, 10_700)
            .unwrap(),
        InviteAttach::Attached {
            org_id: org2.clone(),
            member_id: sam_member_org2.clone(),
        }
    );

    // 3. Own password change ends OTHER sessions while keeping the current session.
    let origin = Origin::new(Scheme::Http, "127.0.0.1", 7420).unwrap();
    let s1 = NewSession::new(
        sam_id.as_str(),
        SessionDigest::cookie(b"sam-c1"),
        SessionDigest::proof(b"sam-p1"),
        origin.clone(),
    )
    .unwrap();
    let s2 = NewSession::new(
        sam_id.as_str(),
        SessionDigest::cookie(b"sam-c2"),
        SessionDigest::proof(b"sam-p2"),
        origin,
    )
    .unwrap();
    db.create(&s1, 11_000).unwrap();
    db.create(&s2, 11_000).unwrap();
    let ended = db
        .change_own_password(&sam_id, &s1.id, &verifier(PHC_B), 11_500)
        .unwrap();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].id, s2.id);

    // 4. Member set-password link clears lockout and ends ALL sessions.
    let sam_key = crate::accounts::identity::LoginKey::new("Sam").unwrap();
    for n in 0..8 {
        db.record_login(
            Socket::Control,
            sam_key.as_str(),
            12_000 + n,
            Outcome::Failure,
        )
        .unwrap();
    }
    assert!(db
        .login_lockout(Socket::Control, sam_key.as_str(), 12_010)
        .unwrap()
        .is_locked());
    let reset = db
        .issue_member_password_reset_link(&org1, &sam_member, 12_100)
        .unwrap();
    let outcome = db
        .redeem_set_password(&reset.token, &verifier(PHC_A), 12_200)
        .unwrap();
    match outcome {
        SetPasswordOutcome::Updated {
            account_id,
            login,
            ended,
        } => {
            assert_eq!(account_id, sam_id);
            assert_eq!(login, "Sam");
            assert_eq!(ended.len(), 1);
            assert_eq!(ended[0].id, s1.id);
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
    assert!(!db
        .login_lockout(Socket::Control, sam_key.as_str(), 12_201)
        .unwrap()
        .is_locked());
}

#[test]
fn owner_and_admin_handover_and_access_removal() {
    let (_home, _secrets, db) = fixture();
    let owner_id = AccountId::parse(crate::hostdb::sqlite::tests::admin_fixture::ACCOUNT).unwrap();
    let org1 = crate::ids::uuid_v7().unwrap();
    let owner_member = crate::ids::uuid_v7().unwrap();
    let sam_member = crate::ids::uuid_v7().unwrap();
    db.seed_owner_for_test(owner_id.as_str(), &org1, &owner_member)
        .unwrap();

    let inv = db.issue_org_invite(&org1, &sam_member, 10_000).unwrap();
    let new_sam = NewAccount::new(Username::new("Sam").unwrap(), verifier(PHC_A)).unwrap();
    let sam_id = match db
        .redeem_invite_register(&inv.token, &new_sam, 10_100)
        .unwrap()
    {
        InviteRegistration::Registered { account_id, .. } => account_id,
        other => panic!("unexpected registration: {other:?}"),
    };

    // Removing owner access is refused with IsOwner; handover + restore + handover unlocks it.
    assert_eq!(
        db.remove_org_access(&org1, &owner_member, 13_000).unwrap(),
        RemoveAccessOutcome::IsOwner
    );
    assert_eq!(
        db.handover_org_owner(&org1, &owner_id, &sam_member)
            .unwrap(),
        OrgOwnerHandoverOutcome::Transferred {
            previous_member_id: owner_member.clone(),
            to_account_id: sam_id.clone(),
            to_member_id: sam_member.clone(),
        }
    );
    assert_eq!(
        db.org_owner(&org1).unwrap(),
        Some((sam_id.clone(), sam_member.clone()))
    );
    db.restore_org_owner(&org1, &owner_id, &owner_member)
        .unwrap();
    assert_eq!(
        db.org_owner(&org1).unwrap(),
        Some((owner_id.clone(), owner_member.clone()))
    );
    assert_eq!(
        db.handover_org_owner(&org1, &owner_id, &sam_member)
            .unwrap(),
        OrgOwnerHandoverOutcome::Transferred {
            previous_member_id: owner_member.clone(),
            to_account_id: sam_id.clone(),
            to_member_id: sam_member.clone(),
        }
    );

    // Administrator handover swaps admin flag atomically.
    assert_eq!(
        db.handover_administrator(&owner_id, &sam_id).unwrap(),
        AdminHandoverOutcome::Transferred
    );
    assert!(!db.account_authority(&owner_id).unwrap().unwrap().admin());
    assert!(db.account_authority(&sam_id).unwrap().unwrap().admin());
    assert!(db.administrator_exists().unwrap());

    // Removing old owner's access unlinks org1 and ends sessions, keeping account unrevoked (SIGN-9).
    let origin = Origin::new(Scheme::Http, "127.0.0.1", 7420).unwrap();
    let o_sess = NewSession::new(
        owner_id.as_str(),
        SessionDigest::cookie(b"own-c1"),
        SessionDigest::proof(b"own-p1"),
        origin,
    )
    .unwrap();
    db.create(&o_sess, 13_100).unwrap();
    let removed = db.remove_org_access(&org1, &owner_member, 13_200).unwrap();
    match removed {
        RemoveAccessOutcome::Removed { account_id, ended } => {
            assert_eq!(account_id, Some(owner_id.clone()));
            assert_eq!(ended.len(), 1);
            assert_eq!(ended[0].id, o_sess.id);
        }
        other => panic!("unexpected remove outcome: {other:?}"),
    }
    let owner_after = db.account_authority(&owner_id).unwrap().unwrap();
    assert!(owner_after.revoked_at_ms().is_none());
    assert!(!owner_after.links().iter().any(|l| l.org_id() == org1));
}
