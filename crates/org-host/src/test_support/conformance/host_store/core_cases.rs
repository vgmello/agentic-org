//! Core `HostStore` conformance cases: instance/ownership, credentials/links, lockout,
//! provisioning, web TLS, recovery ledger, and preferences/local-authless lifecycle.

use org_host_control::ops::TlsRefusal;

use super::{
    sample_public_record, sample_reservation, HostStoreHarness, MemorySecrets, ADMIN_ID, INSTANCE,
    MEMBER_ACC_ID, MEMBER_ID, OFFICE_ID, OP_ID, OP_ID_2, ORG_ID, OTHER_INSTANCE, PHC,
};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::rules::Username;
use crate::host_secrets::{HostSecrets, Name, Value};
use crate::hostdb::journal::{Attempt, Verifier};
use crate::hostdb::lockout::{Outcome, Socket};
use crate::hostdb::provisioning::Taken;
use crate::hostdb::recovery::State as RecoveryState;
use crate::hostdb::web_tls::{Source, StoredPair, TlsLookup, TlsStored};
use crate::hostdb::Error;
use crate::links::{LinkKind, Refused, Subject};

pub fn instance_verification_and_organization_ownership(h: HostStoreHarness<'_>) {
    assert_eq!(h.store.verify_instance(INSTANCE), Ok(()));
    assert_eq!(h.store.verify_instance(OTHER_INSTANCE), Err(Error::Unsafe));
    assert_eq!(h.store.has_administrator(), Ok(false));

    (h.seed)(ADMIN_ID, "Owner", true);
    (h.seed)(MEMBER_ACC_ID, "Member", false);
    assert_eq!(h.store.has_administrator(), Ok(true));

    let admin_id = AccountId::parse(ADMIN_ID).unwrap();
    let member_acc_id = AccountId::parse(MEMBER_ACC_ID).unwrap();
    let admin = h.store.account_authority(&admin_id).unwrap().unwrap();
    let member_acc = h.store.account_authority(&member_acc_id).unwrap().unwrap();

    // Non-administrator cannot establish ownership.
    assert_eq!(
        h.store
            .establish_organization_owner(&member_acc, ORG_ID, MEMBER_ID),
        Err(Error::Unsafe)
    );
    // Live administrator establishes ownership; exact replay is idempotent.
    h.store
        .establish_organization_owner(&admin, ORG_ID, MEMBER_ID)
        .unwrap();
    h.store
        .establish_organization_owner(&admin, ORG_ID, MEMBER_ID)
        .unwrap();
    assert_eq!(
        h.store.verify_selected_owner(ORG_ID, ADMIN_ID, MEMBER_ID),
        Ok(())
    );
    // Different owner member for same org conflicts.
    assert_eq!(
        h.store.establish_organization_owner(
            &admin,
            ORG_ID,
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff"
        ),
        Err(Error::Conflict)
    );
    // Authority snapshot includes the link.
    let reloaded = h.store.account_authority(&admin_id).unwrap().unwrap();
    assert_eq!(reloaded.links().len(), 1);
    assert_eq!(reloaded.links()[0].org_id(), ORG_ID);
    assert_eq!(reloaded.links()[0].member_id(), MEMBER_ID);

    // Revoking the administrator invalidates selected owner verification.
    (h.revoke)(&admin_id);
    assert_eq!(
        h.store.verify_selected_owner(ORG_ID, ADMIN_ID, MEMBER_ID),
        Err(Error::Unsafe)
    );
    assert_eq!(h.store.has_administrator(), Ok(false));
}

pub fn credentials_lookup_and_admin_guarded_links(h: HostStoreHarness<'_>) {
    let now = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;
    let subject = Subject::new("administrator").unwrap();
    // While no administrator exists, link issuance succeeds.
    let issued = h
        .store
        .issue_unless_administrator(LinkKind::Registration, &subject, now)
        .unwrap()
        .expect("issued when no admin exists");
    assert_eq!(
        h.store
            .consume(LinkKind::Registration, &issued.token.encode(), now + 1)
            .unwrap(),
        Ok(issued.link_id)
    );

    // Once an administrator exists, issue_unless_administrator yields None.
    (h.seed)(ADMIN_ID, "Alice", true);
    assert!(h
        .store
        .issue_unless_administrator(LinkKind::Registration, &subject, now + 2)
        .unwrap()
        .is_none());
    assert_eq!(
        h.store
            .consume(LinkKind::Registration, &issued.token.encode(), now + 3)
            .unwrap(),
        Err(Refused::Used)
    );

    let creds = h
        .store
        .account_credentials(&LoginKey::new("alice").unwrap())
        .unwrap()
        .expect("found by folded login key");
    assert_eq!(creds.account().account_id(), ADMIN_ID);
    assert_eq!(creds.account().login(), "Alice");
    assert!(creds.account().admin());
}

pub fn login_lockout_backoff_and_pruning(h: HostStoreHarness<'_>) {
    let base = org_proto::uuid_v7_time_ms(OP_ID).unwrap();
    let initial = h.store.login_lockout(Socket::Local, "alice", base).unwrap();
    assert!(!initial.is_locked());
    assert_eq!(initial.retry_after_ms(), 0);

    for i in 0..5 {
        let state = h
            .store
            .record_login(Socket::Local, "alice", base + i, Outcome::Failure)
            .unwrap();
        assert!(!state.is_locked());
    }
    // Sixth failure engages backoff.
    let locked = h
        .store
        .record_login(Socket::Local, "alice", base + 5, Outcome::Failure)
        .unwrap();
    assert!(locked.is_locked());
    assert_eq!(locked.retry_after_ms(), 1_000);

    // Separate socket is isolated.
    assert!(!h
        .store
        .login_lockout(Socket::Control, "alice", base + 6)
        .unwrap()
        .is_locked());

    // Reset clears a locked entry.
    let cleared = h
        .store
        .record_login(Socket::Local, "alice", base + 6, Outcome::Reset)
        .unwrap();
    assert!(!cleared.is_locked());

    // Single failure prunes after 30 minutes.
    h.store
        .record_login(Socket::Local, "bob", base + 10, Outcome::Failure)
        .unwrap();
    assert_eq!(
        h.store.prune_login_lockouts(base + 10 + 1_800_000).unwrap(),
        1
    );
}

pub fn provisioning_lifecycle(h: HostStoreHarness<'_>) {
    let now = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;
    (h.seed)(ADMIN_ID, "Owner", true);
    let admin_id = AccountId::parse(ADMIN_ID).unwrap();
    let admin = h.store.account_authority(&admin_id).unwrap().unwrap();

    assert_eq!(h.store.provisioning_taken("Acme", "acme"), Ok(None));
    assert_eq!(h.store.provisioning_known(OP_ID), Ok(false));
    assert!(h.store.provisioning_reserved().unwrap().is_empty());

    let r = sample_reservation(OP_ID, "Acme", "acme", ORG_ID, OFFICE_ID);
    assert_eq!(
        h.store.provisioning(&admin, OP_ID, &r.fingerprint, now),
        Ok(None)
    );
    let stored = h.store.reserve_provisioning(&admin, &r).unwrap();
    assert_eq!(stored, r);

    // Exact replay succeeds; conflicting fingerprint is OperationReused.
    assert_eq!(h.store.reserve_provisioning(&admin, &r).unwrap(), r);
    assert_eq!(
        h.store.provisioning(&admin, OP_ID, &[0x22; 32], now),
        Err(Error::OperationReused)
    );
    assert_eq!(
        h.store.provisioning_taken("ACME", "other"),
        Ok(Some(Taken::Name))
    );
    assert_eq!(
        h.store.provisioning_taken("Other", "acme"),
        Ok(Some(Taken::Slug))
    );
    assert_eq!(h.store.provisioning_known(OP_ID), Ok(true));
    assert_eq!(h.store.provisioning_reserved().unwrap().len(), 1);
    assert_eq!(h.store.provisioning_of(ADMIN_ID).unwrap().len(), 1);
    assert_eq!(
        h.store.provisioning_lookup(ADMIN_ID, OP_ID).unwrap(),
        Some(r.clone())
    );
    assert_eq!(
        h.store.provisioning_lookup(MEMBER_ACC_ID, OP_ID).unwrap(),
        None
    );
    assert!(h.store.provisioning_offices().unwrap().contains(OFFICE_ID));

    // Cannot link before key_fingerprint is stored.
    assert_eq!(h.store.provision_linked(&r), Err(Error::Conflict));
    h.store.provision_key_stored(&r, "fp-1").unwrap();
    h.store.provision_key_stored(&r, "fp-1").unwrap();
    assert_eq!(
        h.store.provision_key_stored(&r, "fp-other"),
        Err(Error::Conflict)
    );
    // Unlinked reservation is visible via provisioning_for_org (not provisioning_linked_to).
    assert!(h.store.provisioning_linked_to(ORG_ID).unwrap().is_none());
    let unlinked = h
        .store
        .provisioning_for_org(ORG_ID)
        .unwrap()
        .expect("unlinked org reservation");
    assert!(!unlinked.linked);
    h.store.provision_linked(&r).unwrap();
    assert!(h.store.provisioning_reserved().unwrap().is_empty());
    let linked = h
        .store
        .provisioning_linked_to(ORG_ID)
        .unwrap()
        .expect("linked");
    assert!(linked.linked);
    assert_eq!(linked.key_fingerprint.as_deref(), Some("fp-1"));
    assert!(h.store.provisioning_for_org(ORG_ID).unwrap().is_some());
}

pub fn web_tls_pair_and_journal_replay(h: HostStoreHarness<'_>) {
    let now = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;
    assert_eq!(h.store.web_tls_pair(), Ok(None));

    let made = StoredPair {
        source: Source::Made,
        cert_pem: "-----BEGIN CERTIFICATE-----\nmade\n-----END CERTIFICATE-----\n".into(),
        pair_fp: [0xaa; 32],
        set_at_ms: now,
    };
    h.store.set_web_tls_pair(&made).unwrap();
    assert_eq!(h.store.web_tls_pair(), Ok(Some(made.clone())));

    let supplied = StoredPair {
        source: Source::Supplied,
        cert_pem: "-----BEGIN CERTIFICATE-----\nsupplied\n-----END CERTIFICATE-----\n".into(),
        pair_fp: [0xbb; 32],
        set_at_ms: now + 10,
    };
    let attempt = Attempt {
        id: OP_ID,
        kind: "tls.set",
        fingerprint: [0x55; 32],
        account: ADMIN_ID,
    };
    assert_eq!(
        h.store.tls_set_lookup(&attempt, now + 10),
        Ok(TlsLookup::Vacant)
    );
    let completed = TlsStored::Completed {
        result: serde_json::json!({}),
    };
    assert_eq!(
        h.store
            .tls_set_write(&attempt, now + 10, &completed, Some(&supplied)),
        Ok(None)
    );
    assert_eq!(h.store.web_tls_pair(), Ok(Some(supplied.clone())));
    assert_eq!(
        h.store.tls_set_lookup(&attempt, now + 11),
        Ok(TlsLookup::Replay(completed.clone()))
    );
    assert_eq!(
        h.store
            .tls_set_write(&attempt, now + 12, &completed, Some(&supplied)),
        Ok(Some(completed))
    );

    // Rejected tls_key_mismatch also journals without modifying the stored pair.
    let reject_attempt = Attempt {
        id: OP_ID_2,
        kind: "tls.set",
        fingerprint: [0x66; 32],
        account: ADMIN_ID,
    };
    let rejected = TlsStored::Rejected {
        refusal: TlsRefusal::TlsKeyMismatch,
    };
    assert_eq!(
        h.store
            .tls_set_write(&reject_attempt, now + 13, &rejected, None),
        Ok(None)
    );
    assert_eq!(h.store.web_tls_pair(), Ok(Some(supplied)));
    assert_eq!(
        h.store.tls_set_lookup(&reject_attempt, now + 14),
        Ok(TlsLookup::Replay(rejected))
    );
}

pub fn recovery_ledger_and_reconciliation(h: HostStoreHarness<'_>) {
    let now = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;
    (h.seed)(ADMIN_ID, "Owner", true);
    let admin_id = AccountId::parse(ADMIN_ID).unwrap();
    let admin = h.store.account_authority(&admin_id).unwrap().unwrap();

    let r = sample_reservation(OP_ID, "Acme", "acme", ORG_ID, OFFICE_ID);
    h.store.reserve_provisioning(&admin, &r).unwrap();
    h.store.provision_key_stored(&r, &"a".repeat(64)).unwrap();
    h.store.provision_linked(&r).unwrap();

    let custody = MemorySecrets::default();
    let code = "ABCD-ABCD-ABCD-ABCD-ABCD-ABCD-ABCD-ABCD-ABCD-ABCD";
    custody
        .put(
            &Name::parse(&r.recovery_handle).unwrap(),
            &Value::new(
                serde_json::to_vec(
                    &serde_json::json!({"version": 1, "reservation": r, "code": code}),
                )
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();

    assert_eq!(h.store.recovery_state(OP_ID), Ok(None));
    assert_eq!(h.store.recovery_source(ORG_ID).unwrap().operation_id, OP_ID);

    let public = sample_public_record(&r);
    let prepared = h.store.recovery_prepare(&public).unwrap();
    assert_eq!(prepared.state, RecoveryState::Pending);
    assert_eq!(h.store.recovery_prepare(&public).unwrap(), prepared);
    assert_eq!(
        h.store.recovery_state(OP_ID),
        Ok(Some(RecoveryState::Pending))
    );
    assert_eq!(h.store.recovery_observed(OP_ID).unwrap(), prepared);

    let delivered = h
        .store
        .recovery_delivered(&prepared, ADMIN_ID, false, now + 10)
        .unwrap();
    assert_eq!(delivered.state, RecoveryState::Delivered);
    assert_eq!(delivered.ordinal, 1);

    let redelivered = h
        .store
        .recovery_delivered(&delivered, ADMIN_ID, true, now + 20)
        .unwrap();
    assert_eq!(redelivered.ordinal, 2);
    assert_eq!(redelivered.redelivery_count, 1);

    let ack_pending = h
        .store
        .recovery_ack_intent(&redelivered, ADMIN_ID, now + 30)
        .unwrap();
    assert_eq!(ack_pending.state, RecoveryState::AckPending);

    h.store.recovery_reconcile(&custody).unwrap();
    assert!(custody
        .load(&Name::parse(&r.recovery_handle).unwrap())
        .unwrap()
        .is_none());

    let acked = h.store.recovery_observed(OP_ID).unwrap();
    assert_eq!(acked.state, RecoveryState::Acknowledged);

    let status = h.store.recovery_status(&acked, 0).unwrap();
    assert_eq!(status.delivery_ordinal, "2");
    assert_eq!(status.audit.len(), 4);
    assert_eq!(status.audit[0].action, "show");
    assert_eq!(status.audit[1].action, "show");
    assert_eq!(status.audit[2].action, "ack_pending");
    assert_eq!(status.audit[3].action, "acknowledged");
}

pub fn account_preferences_and_local_authless_lifecycle(h: HostStoreHarness<'_>) {
    let now = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;
    assert_eq!(h.store.local_authless_account(), Ok(None));
    assert_eq!(h.store.auth_enabled(), Ok(true));
    assert_eq!(h.store.has_administrator(), Ok(false));
    assert_eq!(h.store.active_administrator_account(), Ok(None));

    let authless_id = h
        .store
        .init_local_authless_if_empty(now)
        .unwrap()
        .expect("created local authless account");
    assert_eq!(
        h.store.init_local_authless_if_empty(now + 1),
        Ok(Some(authless_id.clone()))
    );
    assert_eq!(
        h.store.local_authless_account(),
        Ok(Some(authless_id.clone()))
    );
    assert_eq!(h.store.auth_enabled(), Ok(false));
    assert_eq!(h.store.has_administrator(), Ok(false));
    assert_eq!(h.store.active_administrator_account(), Ok(None));

    let authless = h.store.account_authority(&authless_id).unwrap().unwrap();
    assert!(authless.admin());
    h.store
        .establish_organization_owner(&authless, ORG_ID, MEMBER_ID)
        .unwrap();

    // Preferences update supplied fields and leave None arguments unchanged.
    assert_eq!(
        h.store
            .set_account_preferences(
                &authless_id,
                Some(Some("Europe/Berlin")),
                Some(Some(ORG_ID)),
            )
            .unwrap(),
        (Some("Europe/Berlin".into()), Some(ORG_ID.into()))
    );
    assert_eq!(
        h.store
            .set_account_preferences(&authless_id, None, Some(None))
            .unwrap(),
        (Some("Europe/Berlin".into()), None)
    );
    let reloaded = h.store.account_authority(&authless_id).unwrap().unwrap();
    assert_eq!(reloaded.time_zone(), Some("Europe/Berlin"));
    assert_eq!(reloaded.last_org_id(), None);

    // Enabling local auth converts the authless account into the real administrator
    // while preserving organization ownership.
    let username = Username::new("Owner").unwrap();
    let verifier = Verifier::new(PHC.into()).unwrap();
    let promoted = h
        .store
        .enable_local_auth(&username, &verifier, now + 10)
        .unwrap();
    assert_eq!(promoted, authless_id);
    assert_eq!(h.store.local_authless_account(), Ok(None));
    assert_eq!(h.store.auth_enabled(), Ok(true));
    assert_eq!(h.store.has_administrator(), Ok(true));
    assert_eq!(
        h.store.active_administrator_account(),
        Ok(Some((authless_id.clone(), "Owner".into())))
    );
    assert_eq!(
        h.store.active_account_login(&authless_id),
        Ok(Some("Owner".into()))
    );
    assert_eq!(
        h.store.org_owner(ORG_ID),
        Ok(Some((authless_id.clone(), MEMBER_ID.into())))
    );
    // Second enable_local_auth refuses with Conflict.
    assert_eq!(
        h.store.enable_local_auth(&username, &verifier, now + 20),
        Err(Error::Conflict)
    );
}
