//! Multi-person lifecycle and operation journal conformance cases for `HostStore`.

use std::sync::Arc;

use org_host_control::HostOpResult;
use org_proto::{ApiError, ErrorCode};

use super::{
    uuid_v7_at, HostStoreHarness, JournalPruneHarness, MemorySecrets, ADMIN_ID, INSTANCE,
    MEMBER_ACC_ID, MEMBER_ID, OP_ID, ORG_ID, PHC,
};
use crate::accounts::identity::AccountId;
use crate::accounts::rules::Username;
use crate::accounts::store::NewAccount;
use crate::authentication::Authentication;
use crate::host_secrets::HostSecrets;
use crate::hostdb::journal::{
    Attempt, Effect, Lookup, PruneOutcome, Stored, Verifier, Write, DAY, PRUNE_BATCH,
};
use crate::hostdb::multi_person::{
    account_subject, invite_subject, AdminHandoverOutcome, InviteAttach, InviteIssueError,
    InviteRegistration, OrgOwnerHandoverOutcome, RemoveAccessOutcome, SetPasswordOutcome,
};
use crate::hostdb::{Error, HostStore};
use crate::links::{LinkKind, Refused};
use crate::sessions::{NewSession, Origin, Scheme, SessionDigest};

pub fn multi_person_invites_resets_handovers_and_access_removal(h: HostStoreHarness<'_>) {
    const ORG_ID_2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d0";
    const MEMBER_2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d2";
    const MEMBER_3: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d3";
    const PHC_2: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA";
    const PHC_3: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCA";

    let now = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;
    (h.seed)(ADMIN_ID, "Owner", true);
    let admin_id = AccountId::parse(ADMIN_ID).unwrap();
    let admin = h.store.account_authority(&admin_id).unwrap().unwrap();
    h.store
        .establish_organization_owner(&admin, ORG_ID, MEMBER_ID)
        .unwrap();
    h.store
        .establish_organization_owner(&admin, ORG_ID_2, MEMBER_ID)
        .unwrap();

    assert_eq!(
        h.store.org_owner(ORG_ID),
        Ok(Some((admin_id.clone(), MEMBER_ID.into())))
    );
    assert_eq!(
        h.store
            .issue_org_invite(ORG_ID, MEMBER_ID, now)
            .unwrap_err(),
        InviteIssueError::AlreadyLinked
    );

    let invite = h.store.issue_org_invite(ORG_ID, MEMBER_2, now).unwrap();
    assert_eq!(
        h.store
            .peek_link_subject(LinkKind::Registration, &invite.token, now + 1)
            .unwrap(),
        Ok(invite_subject(ORG_ID, MEMBER_2)
            .unwrap()
            .as_str()
            .to_owned())
    );

    let entries = h.store.list_org_access(ORG_ID, now + 1).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries
        .iter()
        .any(|e| e.member_id == MEMBER_ID && e.owner && e.admin));
    assert!(entries
        .iter()
        .any(|e| e.member_id == MEMBER_2 && e.pending_invite && e.account_id.is_none()));

    let bob_new = NewAccount {
        account_id: AccountId::parse(MEMBER_ACC_ID).unwrap(),
        username: Username::new("Bob").unwrap(),
        verifier: Verifier::new(PHC.into()).unwrap(),
    };
    let bob_id = bob_new.account_id.clone();
    assert_eq!(
        h.store
            .redeem_invite_register(&invite.token, &bob_new, now + 5)
            .unwrap(),
        InviteRegistration::Registered {
            account_id: bob_id.clone(),
            org_id: ORG_ID.into(),
            member_id: MEMBER_2.into(),
        }
    );
    assert_eq!(
        h.store
            .redeem_invite_register(&invite.token, &bob_new, now + 6)
            .unwrap(),
        InviteRegistration::Link(Refused::Used)
    );

    // Attach Bob to a second organization via an invite link.
    let invite_2 = h
        .store
        .issue_org_invite(ORG_ID_2, MEMBER_3, now + 7)
        .unwrap();
    assert_eq!(
        h.store
            .redeem_invite_attach(&invite_2.token, &bob_id, now + 8)
            .unwrap(),
        InviteAttach::Attached {
            org_id: ORG_ID_2.into(),
            member_id: MEMBER_3.into(),
        }
    );

    // Create two browser sessions for Bob, then reset password via one-time link.
    let origin = Origin::new(Scheme::Http, "127.0.0.1", 7420).unwrap();
    let s1 = NewSession::new(
        bob_id.as_str(),
        SessionDigest([1; 32]),
        SessionDigest([2; 32]),
        origin.clone(),
    )
    .unwrap();
    let s2 = NewSession::new(
        bob_id.as_str(),
        SessionDigest([3; 32]),
        SessionDigest([4; 32]),
        origin.clone(),
    )
    .unwrap();
    h.store.create(&s1, now + 9).unwrap();
    h.store.create(&s2, now + 9).unwrap();

    let reset_link = h
        .store
        .issue_member_password_reset_link(ORG_ID, MEMBER_2, now + 10)
        .unwrap();
    assert_eq!(
        h.store
            .peek_link_subject(LinkKind::SetPassword, &reset_link.token, now + 11)
            .unwrap(),
        Ok(account_subject(&bob_id).unwrap().as_str().to_owned())
    );

    let v2 = Verifier::new(PHC_2.into()).unwrap();
    let SetPasswordOutcome::Updated {
        account_id,
        login,
        ended,
    } = h
        .store
        .redeem_set_password(&reset_link.token, &v2, now + 12)
        .unwrap()
    else {
        panic!("expected password update");
    };
    assert_eq!(account_id, bob_id);
    assert_eq!(login, "Bob");
    assert_eq!(ended.len(), 2);

    // Own password change preserves the kept session and ends all others.
    let s3 = NewSession::new(
        bob_id.as_str(),
        SessionDigest([5; 32]),
        SessionDigest([6; 32]),
        origin.clone(),
    )
    .unwrap();
    let s4 = NewSession::new(
        bob_id.as_str(),
        SessionDigest([7; 32]),
        SessionDigest([8; 32]),
        origin,
    )
    .unwrap();
    h.store.create(&s3, now + 13).unwrap();
    h.store.create(&s4, now + 13).unwrap();
    let v3 = Verifier::new(PHC_3.into()).unwrap();
    let ended_own = h
        .store
        .change_own_password(&bob_id, &s3.id, &v3, now + 14)
        .unwrap();
    assert_eq!(ended_own.len(), 1);
    assert_eq!(ended_own[0].id, s4.id);

    // Admin password reset link targets the active administrator.
    let (adm_reset_login, _adm_link) = h
        .store
        .issue_admin_password_reset_link(now + 15)
        .unwrap()
        .expect("admin reset link");
    assert_eq!(adm_reset_login, "Owner");

    // Org owner handover + restore.
    assert_eq!(
        h.store
            .handover_org_owner(ORG_ID, &admin_id, MEMBER_2)
            .unwrap(),
        OrgOwnerHandoverOutcome::Transferred {
            previous_member_id: MEMBER_ID.into(),
            to_account_id: bob_id.clone(),
            to_member_id: MEMBER_2.into(),
        }
    );
    assert_eq!(
        h.store.org_owner(ORG_ID),
        Ok(Some((bob_id.clone(), MEMBER_2.into())))
    );
    h.store
        .restore_org_owner(ORG_ID, &admin_id, MEMBER_ID)
        .unwrap();
    assert_eq!(
        h.store.org_owner(ORG_ID),
        Ok(Some((admin_id.clone(), MEMBER_ID.into())))
    );

    // Administrator handover to Bob and back to Owner.
    assert_eq!(
        h.store.handover_administrator(&admin_id, &bob_id).unwrap(),
        AdminHandoverOutcome::Transferred
    );
    assert_eq!(
        h.store.active_administrator_account(),
        Ok(Some((bob_id.clone(), "Bob".into())))
    );
    assert_eq!(
        h.store.handover_administrator(&bob_id, &admin_id).unwrap(),
        AdminHandoverOutcome::Transferred
    );

    // Removing the owner refuses; removing Bob succeeds and ends his remaining session.
    assert_eq!(
        h.store
            .remove_org_access(ORG_ID, MEMBER_ID, now + 16)
            .unwrap(),
        RemoveAccessOutcome::IsOwner
    );
    let RemoveAccessOutcome::Removed {
        account_id: removed_acct,
        ended: removed_sessions,
    } = h
        .store
        .remove_org_access(ORG_ID, MEMBER_2, now + 17)
        .unwrap()
    else {
        panic!("expected Removed");
    };
    assert_eq!(removed_acct, Some(bob_id));
    assert_eq!(removed_sessions.len(), 1);
    assert_eq!(removed_sessions[0].id, s3.id);
}

pub fn operation_journal_lookup_and_commit(h: HostStoreHarness<'_>) {
    let now = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;
    let attempt = Attempt {
        id: OP_ID,
        kind: "office.restart",
        fingerprint: [0x42; 32],
        account: ADMIN_ID,
    };
    assert!(matches!(
        h.store.journal_lookup(&attempt, now).unwrap(),
        Lookup::Vacant
    ));

    let payload = HostOpResult::Completed {
        result: serde_json::json!({"restarted": true}),
    };
    assert!(matches!(
        h.store
            .journal_commit(
                &attempt,
                now,
                Effect {
                    stored: payload.clone(),
                    verifier: None,
                },
            )
            .unwrap(),
        Write::Committed(ref stored) if stored == &payload
    ));

    assert!(matches!(
        h.store.journal_lookup(&attempt, now + 1).unwrap(),
        Lookup::Replay(ref stored) if stored == &payload
    ));
    assert!(matches!(
        h.store
            .journal_commit(
                &attempt,
                now + 2,
                Effect {
                    stored: payload.clone(),
                    verifier: None,
                },
            )
            .unwrap(),
        Write::Existing(Lookup::Replay(ref stored)) if stored == &payload
    ));

    // Different fingerprint on the same operation ID is OperationReused.
    let reused = Attempt {
        fingerprint: [0x99; 32],
        ..attempt
    };
    assert_eq!(
        h.store.journal_lookup(&reused, now + 3).err(),
        Some(Error::OperationReused)
    );
}

fn seed_journal_prune_backlog(store: &dyn HostStore, t0: i64, payload: &Stored) -> (usize, String) {
    let first_attempt = Attempt {
        id: OP_ID,
        kind: "office.restart",
        fingerprint: [0x42; 32],
        account: ADMIN_ID,
    };
    assert!(matches!(
        store
            .journal_commit(
                &first_attempt,
                t0,
                Effect {
                    stored: payload.clone(),
                    verifier: None,
                },
            )
            .unwrap(),
        Write::Committed(_)
    ));
    let extra = usize::from(PRUNE_BATCH) + 5;
    for seq in 0..extra {
        let ident = uuid_v7_at(t0, 100 + seq as u64);
        let op = Attempt {
            id: &ident,
            kind: "office.restart",
            fingerprint: [0x51; 32],
            account: ADMIN_ID,
        };
        store
            .journal_commit(
                &op,
                t0,
                Effect {
                    stored: payload.clone(),
                    verifier: None,
                },
            )
            .unwrap();
    }
    let expired_create_id = uuid_v7_at(t0, 900);
    let expired_create = Attempt {
        id: &expired_create_id,
        kind: "account.create",
        fingerprint: [0x52; 32],
        account: ADMIN_ID,
    };
    store
        .journal_commit(
            &expired_create,
            t0,
            Effect {
                stored: Stored::Completed {
                    result: serde_json::json!({"account": "expired-acct"}),
                },
                verifier: Some(("expired-acct".into(), Verifier::new(PHC.into()).unwrap())),
            },
        )
        .unwrap();
    (extra, expired_create_id)
}

fn assert_failed_journal_commits_preserve_clock(store: &dyn HostStore, future_ms: i64) {
    let bad_refusal_id = uuid_v7_at(future_ms, 1);
    let bad_refusal = Attempt {
        id: &bad_refusal_id,
        kind: "office.restart",
        fingerprint: [0x11; 32],
        account: ADMIN_ID,
    };
    assert!(matches!(
        store.journal_commit(
            &bad_refusal,
            future_ms,
            Effect {
                stored: Stored::Rejected {
                    refusal: ApiError::new(ErrorCode::NotAuthorized, "not stored"),
                },
                verifier: None,
            },
        ),
        Err(Error::Unsafe)
    ));
    let bad_create_id = uuid_v7_at(future_ms, 2);
    let bad_create = Attempt {
        id: &bad_create_id,
        kind: "account.create",
        fingerprint: [0x12; 32],
        account: ADMIN_ID,
    };
    assert!(matches!(
        store.journal_commit(
            &bad_create,
            future_ms,
            Effect {
                stored: Stored::Completed {
                    result: serde_json::json!({"account": "created"}),
                },
                verifier: None,
            },
        ),
        Err(Error::Unsafe)
    ));
}

pub fn operation_journal_prune_expired_and_rollback(h: HostStoreHarness<'_>) {
    let t0 = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;
    let payload = Stored::Completed {
        result: serde_json::json!({"restarted": true}),
    };
    let (extra, expired_create_id) = seed_journal_prune_backlog(h.store, t0, &payload);
    let first_attempt = Attempt {
        id: OP_ID,
        kind: "office.restart",
        fingerprint: [0x42; 32],
        account: ADMIN_ID,
    };
    let expired_create = Attempt {
        id: &expired_create_id,
        kind: "account.create",
        fingerprint: [0x52; 32],
        account: ADMIN_ID,
    };

    // Failed journal_commit calls at a future wall_ms must not advance clock_high_ms or prune.
    assert_failed_journal_commits_preserve_clock(h.store, t0 + 30 * DAY + 1_000);

    // Live rows at t0 + DAY still commit cleanly and first_attempt still replays before expiry.
    let live_restart_id = uuid_v7_at(t0 + DAY, 901);
    let live_restart = Attempt {
        id: &live_restart_id,
        kind: "office.restart",
        fingerprint: [0x53; 32],
        account: ADMIN_ID,
    };
    h.store
        .journal_commit(
            &live_restart,
            t0 + DAY,
            Effect {
                stored: payload.clone(),
                verifier: None,
            },
        )
        .unwrap();
    let live_create_id = uuid_v7_at(t0 + DAY, 902);
    let live_create = Attempt {
        id: &live_create_id,
        kind: "account.create",
        fingerprint: [0x54; 32],
        account: ADMIN_ID,
    };
    h.store
        .journal_commit(
            &live_create,
            t0 + DAY,
            Effect {
                stored: Stored::Completed {
                    result: serde_json::json!({"account": "live-acct"}),
                },
                verifier: Some(("live-acct".into(), Verifier::new(PHC.into()).unwrap())),
            },
        )
        .unwrap();
    assert!(matches!(
        h.store
            .journal_lookup(&first_attempt, t0 + 30 * DAY - 1)
            .unwrap(),
        Lookup::Replay(ref stored) if stored == &payload
    ));

    let prune_at = t0 + 30 * DAY;
    let reserved_id = uuid_v7_at(t0 + 2 * DAY, 903);
    let staged_id = uuid_v7_at(t0 + 2 * DAY, 904);
    (h.seed_exempt_journal)(&reserved_id, &staged_id, ADMIN_ID, prune_at);

    assert_eq!(h.store.journal_prune_expired(-1), Err(Error::Unsafe));
    let expected_removed = 1 + extra as u64 + 1;
    assert_eq!(
        h.store.journal_prune_expired(prune_at).unwrap(),
        PruneOutcome {
            removed: expected_removed,
            more_remaining: false,
        }
    );
    assert_eq!(
        h.store.journal_prune_expired(prune_at).unwrap(),
        PruneOutcome {
            removed: 0,
            more_remaining: false,
        }
    );

    assert!(matches!(
        h.store.journal_lookup(&live_restart, prune_at).unwrap(),
        Lookup::Replay(ref stored) if stored == &payload
    ));
    assert!(matches!(
        h.store.journal_lookup(&live_create, prune_at).unwrap(),
        Lookup::NeedsVerification { .. }
    ));
    assert!(matches!(
        h.store.journal_lookup(&expired_create, prune_at),
        Err(Error::Expired)
    ));
    for exempt_id in [&reserved_id, &staged_id] {
        let exempt = Attempt {
            id: exempt_id,
            kind: "organization.create",
            fingerprint: [7u8; 32],
            account: ADMIN_ID,
        };
        assert!(matches!(
            h.store.journal_lookup(&exempt, prune_at),
            Err(Error::Expired)
        ));
    }
}

pub fn journal_prune_batch_cap_and_startup_wiring(make: impl Fn(i64) -> JournalPruneHarness) {
    const HOUR: i64 = 3_600_000;
    let live_now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    // Expires one hour before startup, so the later live write at `live_now - DAY` does not
    // prune these entries prematurely.
    let expired_at = live_now - 30 * DAY - HOUR;

    // 1. Bounded batch cap sets `more_remaining: true` when rows remain past the cap.
    let capped = make(expired_at);
    for seq in 0..(usize::from(PRUNE_BATCH) + 5) {
        let ident = uuid_v7_at(expired_at, seq as u64);
        let op = Attempt {
            id: &ident,
            kind: "office.restart",
            fingerprint: [0x31; 32],
            account: ADMIN_ID,
        };
        capped
            .store
            .journal_commit(
                &op,
                expired_at,
                Effect {
                    stored: Stored::Completed {
                        result: serde_json::json!({"ok": true}),
                    },
                    verifier: None,
                },
            )
            .unwrap();
    }
    assert_eq!(
        (capped.prune_capped)(live_now, 1).unwrap(),
        PruneOutcome {
            removed: u64::from(PRUNE_BATCH),
            more_remaining: true,
        }
    );
    assert_eq!(
        capped.store.journal_prune_expired(live_now).unwrap(),
        PruneOutcome {
            removed: 5,
            more_remaining: false,
        }
    );

    // 2. Authentication::with_store invokes journal_prune_expired at startup on any HostStore.
    let boot = make(expired_at);
    let expired_create_id = uuid_v7_at(expired_at, 1);
    boot.store
        .journal_commit(
            &Attempt {
                id: &expired_create_id,
                kind: "account.create",
                fingerprint: [0x61; 32],
                account: ADMIN_ID,
            },
            expired_at,
            Effect {
                stored: Stored::Completed {
                    result: serde_json::json!({"account": "expired-acct"}),
                },
                verifier: Some(("expired-acct".into(), Verifier::new(PHC.into()).unwrap())),
            },
        )
        .unwrap();
    for seq in 0..(usize::from(PRUNE_BATCH) + 10) {
        let ident = uuid_v7_at(expired_at, 100 + seq as u64);
        boot.store
            .journal_commit(
                &Attempt {
                    id: &ident,
                    kind: "office.restart",
                    fingerprint: [0x62; 32],
                    account: ADMIN_ID,
                },
                expired_at,
                Effect {
                    stored: Stored::Completed {
                        result: serde_json::json!({"ok": true}),
                    },
                    verifier: None,
                },
            )
            .unwrap();
    }
    let live_create_id = uuid_v7_at(live_now - DAY, 2);
    let live_create = Attempt {
        id: &live_create_id,
        kind: "account.create",
        fingerprint: [0x63; 32],
        account: ADMIN_ID,
    };
    boot.store
        .journal_commit(
            &live_create,
            live_now - DAY,
            Effect {
                stored: Stored::Completed {
                    result: serde_json::json!({"account": "live-acct"}),
                },
                verifier: Some(("live-acct".into(), Verifier::new(PHC.into()).unwrap())),
            },
        )
        .unwrap();
    let reserved_id = uuid_v7_at(live_now - DAY, 3);
    let staged_id = uuid_v7_at(live_now - DAY, 4);
    (boot.seed_exempt_journal)(&reserved_id, &staged_id, ADMIN_ID, live_now - 1_000);
    assert_eq!((boot.op_verifier_accounts)().len(), 2);

    let secrets: Arc<dyn HostSecrets> = Arc::new(MemorySecrets::default());
    let _auth = Authentication::with_store(Arc::clone(&boot.store), secrets, INSTANCE)
        .expect("Authentication::with_store succeeds and prunes expired journal");

    assert_eq!((boot.op_verifier_accounts)(), vec!["live-acct"]);
    assert_eq!(
        boot.store.journal_prune_expired(live_now).unwrap(),
        PruneOutcome {
            removed: 0,
            more_remaining: false,
        }
    );
    assert!(matches!(
        boot.store.journal_lookup(&live_create, live_now).unwrap(),
        Lookup::NeedsVerification { .. }
    ));
    for exempt_id in [&reserved_id, &staged_id] {
        let exempt = Attempt {
            id: exempt_id,
            kind: "organization.create",
            fingerprint: [7u8; 32],
            account: ADMIN_ID,
        };
        assert!(matches!(
            boot.store.journal_lookup(&exempt, live_now),
            Err(Error::Expired)
        ));
    }
}
