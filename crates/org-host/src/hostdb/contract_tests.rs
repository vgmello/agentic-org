//! Engine-independent `HostStore` conformance suite (ADR-0018 ENG-2 / ENG-3).
//!
//! Every case runs against both the SQLite adapter (`HostDb`) and the in-memory adapter
//! (`MemoryHostStore`), proving that all host-registry semantics stay engine-neutral.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

use org_host_control::ops::TlsRefusal;
use org_office_launch::recovery::PublicRecord;
use org_proto::selected_bootstrap::{CreatorV1, PlanV1, PreparedV1};
use sha2::{Digest, Sha256};

use super::journal::{Attempt, Verifier};
use super::lockout::{Outcome, Socket};
use super::memory::MemoryHostStore;
use super::provisioning::{Reservation, Taken};
use super::recovery::State as RecoveryState;
use super::web_tls::{Source, StoredPair, TlsLookup, TlsStored};
use super::{Error, HostDb, HostDbInit, HostStore};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::host_secrets::{HostSecrets, Name, Value};
use crate::links::{LinkKind, Refused, Subject};
use crate::sessions::contract_tests::NoSecrets;

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
const OTHER_INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b1";
const ADMIN_ID: &str = "01J0000000000000000000000A";
const MEMBER_ACC_ID: &str = "01J0000000000000000000000B";
const ORG_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";
const OFFICE_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c1";
const MEMBER_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c2";
const OP_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607110";
const OP_ID_2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607111";
const PHC: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

struct Harness<'a> {
    store: &'a dyn HostStore,
    seed: &'a dyn Fn(&str, &str, bool),
    revoke: &'a dyn Fn(&AccountId),
    seed_exempt_journal: &'a dyn Fn(&str, &str, &str, i64),
}

fn uuid_v7_at(at: i64, sequence: u64) -> String {
    let ms = u64::try_from(at).unwrap();
    let rand_a = ((sequence >> 62) & 0x0fff) as u16;
    let rand_b = sequence & ((1u64 << 62) - 1);
    let hi = (ms << 16) | (0x7 << 12) | u64::from(rand_a);
    let lo = (0b10u64 << 62) | rand_b;
    let hex = format!("{hi:016x}{lo:016x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn each(case: impl Fn(Harness<'_>)) {
    let verifier = Verifier::new(PHC.into()).unwrap();
    let now = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;

    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let init = HostDbInit {
        instance_id: INSTANCE.into(),
        clock_high_ms: now,
    };
    let sqlite = HostDb::create(home.path(), &NoSecrets, &init).unwrap();
    case(Harness {
        store: &sqlite,
        seed: &|id, login, admin| {
            sqlite
                .seed_account(id, login, &verifier, admin, now)
                .unwrap();
        },
        revoke: &|id| sqlite.change_authority_fixture(id, true).unwrap(),
        seed_exempt_journal: &|reserved_id, staged_id, account, expires_at_ms| {
            sqlite.seed_expired_exempt_journal_rows(reserved_id, staged_id, account, expires_at_ms);
        },
    });

    let memory = MemoryHostStore::new(INSTANCE, now);
    case(Harness {
        store: &memory,
        seed: &|id, login, admin| {
            memory
                .seed_account(id, login, &verifier, admin, now)
                .unwrap();
        },
        revoke: &|id| memory.revoke_account(id),
        seed_exempt_journal: &|reserved_id, staged_id, account, expires_at_ms| {
            memory.seed_expired_exempt_journal_rows(reserved_id, staged_id, account, expires_at_ms);
        },
    });
}

fn sample_reservation(op: &str, name: &str, slug: &str, org: &str, office: &str) -> Reservation {
    let prepared = PreparedV1 {
        plan: PlanV1 {
            version: 1,
            name: name.into(),
            slug: slug.into(),
            repository_remote_url: "https://example.test/org.git".into(),
            state_branch: "org/state".into(),
        },
        organization_id: org.into(),
        office_id: office.into(),
        repository_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c3".into(),
        home_team_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c4".into(),
        hr_employee_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c5".into(),
        creator: CreatorV1::from_login(MEMBER_ID.into(), "Owner"),
    };
    assert!(prepared.valid());
    Reservation {
        operation_id: op.into(),
        account: ADMIN_ID.into(),
        fingerprint: [0x11; 32],
        prepared,
        owner_member: MEMBER_ID.into(),
        recovery_handle: format!("recovery.{op}"),
        key_fingerprint: None,
        linked: false,
    }
}

#[test]
fn instance_verification_and_organization_ownership_conformance() {
    each(|h| {
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
    });
}

#[test]
fn credentials_lookup_and_admin_guarded_links_conformance() {
    each(|h| {
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
    });
}

#[test]
fn login_lockout_backoff_and_pruning_conformance() {
    each(|h| {
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
    });
}

#[test]
fn provisioning_lifecycle_conformance() {
    each(|h| {
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
    });
}

#[test]
fn web_tls_pair_and_journal_replay_conformance() {
    each(|h| {
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
    });
}

#[derive(Default)]
struct MemorySecrets(Mutex<BTreeMap<Name, Vec<u8>>>);

impl HostSecrets for MemorySecrets {
    fn put(&self, name: &Name, value: &Value) -> Result<(), crate::host_secrets::Error> {
        self.0
            .lock()
            .unwrap()
            .insert(name.clone(), value.expose().to_vec());
        Ok(())
    }
    fn load(&self, name: &Name) -> Result<Option<Value>, crate::host_secrets::Error> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(name)
            .map(|b| Value::new(b.clone()).unwrap()))
    }
    fn delete(&self, name: &Name) -> Result<(), crate::host_secrets::Error> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
    fn list(&self) -> Result<Vec<Name>, crate::host_secrets::Error> {
        Ok(self.0.lock().unwrap().keys().cloned().collect())
    }
}

fn sample_public_record(r: &Reservation) -> PublicRecord {
    let org = r.prepared.organization_id.as_bytes();
    let mut bytes = b"AGORGCA1".to_vec();
    bytes.extend((org.len() as u16).to_be_bytes());
    bytes.extend(org);
    bytes.push(1);
    bytes.extend([0; 28]);
    let mut key = [7; 65];
    key[0] = 4;
    bytes.extend(key);
    bytes.extend(17u32.to_be_bytes());
    bytes.extend([0; 17]);
    PublicRecord {
        version: 1,
        instance_id: INSTANCE.into(),
        office_id: r.prepared.office_id.clone(),
        organization_id: r.prepared.organization_id.clone(),
        operation_id: r.operation_id.clone(),
        escrow_version: 1,
        public_key: org_office_launch::recovery::hex(&key),
        package_sha256: org_office_launch::recovery::hex(&Sha256::digest(&bytes)),
        package_hex: org_office_launch::recovery::hex(&bytes),
    }
}

#[test]
fn recovery_ledger_and_reconciliation_conformance() {
    each(|h| {
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
    });
}

#[test]
fn account_preferences_and_local_authless_lifecycle_conformance() {
    each(|h| {
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
        let username = crate::accounts::rules::Username::new("Owner").unwrap();
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
    });
}

#[test]
fn multi_person_invites_resets_handovers_and_access_removal_conformance() {
    use super::multi_person::{
        account_subject, invite_subject, AdminHandoverOutcome, InviteAttach, InviteIssueError,
        InviteRegistration, OrgOwnerHandoverOutcome, RemoveAccessOutcome, SetPasswordOutcome,
    };
    use crate::accounts::rules::Username;
    use crate::accounts::store::NewAccount;
    use crate::sessions::{NewSession, Origin, Scheme, SessionDigest};

    const ORG_ID_2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d0";
    const MEMBER_2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d2";
    const MEMBER_3: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d3";
    const PHC_2: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA";
    const PHC_3: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCA";

    each(|h| {
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
    });
}

#[test]
fn operation_journal_lookup_and_commit_conformance() {
    use super::journal::{Effect, Lookup, Write};
    use org_host_control::HostOpResult;

    each(|h| {
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
    });
}

fn seed_journal_prune_backlog(
    store: &dyn HostStore,
    t0: i64,
    payload: &super::journal::Stored,
) -> (usize, String) {
    use super::journal::{Effect, Stored, Write, PRUNE_BATCH};

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
    use super::journal::{Effect, Stored};
    use org_proto::{ApiError, ErrorCode};

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

#[test]
fn operation_journal_prune_expired_and_rollback_conformance() {
    use super::journal::{Effect, Lookup, PruneOutcome, Stored, DAY};

    each(|h| {
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
    });
}

#[test]
fn memory_journal_prune_batch_cap_and_startup_wiring() {
    use std::sync::Arc;

    use super::journal::{Effect, Lookup, PruneOutcome, Stored, DAY, PRUNE_BATCH};
    use crate::authentication::Authentication;

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
    let capped = MemoryHostStore::new(INSTANCE, expired_at);
    for seq in 0..(usize::from(PRUNE_BATCH) + 5) {
        let ident = uuid_v7_at(expired_at, seq as u64);
        let op = Attempt {
            id: &ident,
            kind: "office.restart",
            fingerprint: [0x31; 32],
            account: ADMIN_ID,
        };
        capped
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
        capped.journal_prune_expired_capped(live_now, 1).unwrap(),
        PruneOutcome {
            removed: u64::from(PRUNE_BATCH),
            more_remaining: true,
        }
    );
    assert_eq!(
        capped.journal_prune_expired(live_now).unwrap(),
        PruneOutcome {
            removed: 5,
            more_remaining: false,
        }
    );

    // 2. Authentication::with_store invokes journal_prune_expired at startup on any HostStore.
    let memory = Arc::new(MemoryHostStore::new(INSTANCE, expired_at));
    let expired_create_id = uuid_v7_at(expired_at, 1);
    memory
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
        memory
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
    memory
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
    memory.seed_expired_exempt_journal_rows(&reserved_id, &staged_id, ADMIN_ID, live_now - 1_000);
    assert_eq!(memory.op_verifier_accounts().len(), 2);

    let store: Arc<dyn HostStore> = Arc::clone(&memory) as Arc<dyn HostStore>;
    let secrets: Arc<dyn HostSecrets> = Arc::new(MemorySecrets::default());
    let _auth = Authentication::with_store(Arc::clone(&store), secrets, INSTANCE)
        .expect("Authentication::with_store succeeds and prunes expired journal");

    assert_eq!(memory.op_verifier_accounts(), vec!["live-acct"]);
    assert_eq!(
        store.journal_prune_expired(live_now).unwrap(),
        PruneOutcome {
            removed: 0,
            more_remaining: false,
        }
    );
    assert!(matches!(
        store.journal_lookup(&live_create, live_now).unwrap(),
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
            store.journal_lookup(&exempt, live_now),
            Err(Error::Expired)
        ));
    }
}

#[test]
fn live_host_subsystems_run_end_to_end_on_memory_host_store() {
    use std::sync::Arc;

    use org_host_control::admin::AdminLinkResult;
    use org_host_control::browser::{
        BrowserOrigin, Digest32, LinkRegisterParams, SessionSignInParams, SessionValidateParams,
    };
    use org_host_control::browser_accounts::{
        InspectLinkKind, LinkInspectParams, LinkInspectResult, LinkSetPasswordParams,
    };
    use org_host_control::listen::ListenOrigin;
    use org_host_control::{LinkToken, Password};

    use crate::admin::{Admin, Presence};
    use crate::app_auth::AppPolicy;
    use crate::authentication::Authentication;
    use crate::control_listener::Connection;
    use crate::keys::memory::MemoryKeys;
    use crate::provisioning::Provisioning;
    use crate::web_tls::WebTls;
    use crate::writer::Writers;

    struct LocalApprove;
    impl Presence for LocalApprove {
        fn local(&self, _: &Connection) -> bool {
            true
        }
        fn confirm(&self, _: &dyn Fn() -> bool) -> Result<(), ()> {
            Ok(())
        }
    }

    let now = org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64;
    let store: Arc<dyn HostStore> = Arc::new(MemoryHostStore::new(INSTANCE, now));
    let secrets: Arc<dyn HostSecrets> = Arc::new(MemorySecrets::default());

    let auth = Authentication::with_store(Arc::clone(&store), Arc::clone(&secrets), INSTANCE)
        .expect("Authentication wired over MemoryHostStore");
    let admin = Admin::new(
        Arc::clone(&store),
        OP_ID,
        ListenOrigin::parse("http://127.0.0.1:7420").unwrap(),
        AppPolicy::new(crate::HOST_SIGNING_ID, None).unwrap(),
        Arc::new(LocalApprove),
    );

    // 1. Issue initial admin link through Admin -> register through Authentication -> validate & sign in.
    let (host_sock, _peer_sock) = std::os::unix::net::UnixStream::pair().unwrap();
    let root_conn = Connection::accepted_for_tests(&host_sock);
    root_conn.mark_root_authority();
    let received = crate::peer::Received {
        sent_at: 0,
        buffered_after: 0,
    };
    let AdminLinkResult::Issued { url, .. } = admin.reset(&root_conn, received).unwrap() else {
        panic!("expected initial registration link");
    };
    let token_str = url
        .expose()
        .strip_prefix("http://127.0.0.1:7420/register#")
        .expect("registration token")
        .to_owned();
    let live_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let origin = BrowserOrigin {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: 7420,
    };
    let mut ended = Vec::new();
    let registered = auth
        .register(
            &LinkRegisterParams {
                link: LinkToken::new(token_str).unwrap(),
                login: "Alice".into(),
                password: Password::new("initial-secret-passphrase".into()).unwrap(),
                client_address: "127.0.0.1".parse().unwrap(),
                cookie_digest: Digest32([0x11; 32]),
                proof_digest: Digest32([0x22; 32]),
                origin: origin.clone(),
            },
            live_now + 10,
            &mut ended,
        )
        .expect("browser registration over MemoryHostStore");
    assert_eq!(store.has_administrator(), Ok(true));
    assert_eq!(registered.account.login, "Alice");

    let validated = auth
        .validate_browser(
            &SessionValidateParams {
                cookie_digest: Digest32([0x11; 32]),
                proof_digest: Digest32([0x22; 32]),
                origin: origin.clone(),
            },
            live_now + 20,
            &mut ended,
        )
        .expect("browser session validates over MemoryHostStore");
    assert_eq!(validated.session_id, registered.session_id);

    let signed_in = auth
        .sign_in_browser(
            &SessionSignInParams {
                login: "alice".into(),
                password: Password::new("initial-secret-passphrase".into()).unwrap(),
                client_address: "127.0.0.1".parse().unwrap(),
                cookie_digest: Digest32([0x33; 32]),
                proof_digest: Digest32([0x44; 32]),
                origin,
            },
            live_now + 30,
            &mut ended,
        )
        .expect("browser sign-in over MemoryHostStore");
    assert_eq!(signed_in.account.id, registered.account.id);

    // 2. Admin reset issues password-reset link -> inspect & redeem through Authentication.
    let AdminLinkResult::ResetPassword { login, url, .. } =
        admin.reset(&root_conn, received).unwrap()
    else {
        panic!("expected ResetPassword link");
    };
    assert_eq!(login, "Alice");
    let reset_token = url
        .expose()
        .strip_prefix("http://127.0.0.1:7420/set-password#")
        .expect("set-password token")
        .to_owned();
    let reset_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let inspected = auth
        .inspect_link(
            &LinkInspectParams {
                link: LinkToken::new(reset_token.clone()).unwrap(),
                kind: InspectLinkKind::SetPassword,
            },
            reset_now,
            |_| String::new(),
        )
        .expect("inspect set-password link over MemoryHostStore");
    assert_eq!(
        inspected,
        LinkInspectResult::SetPassword {
            login: "Alice".into(),
        }
    );
    ended.clear();
    let updated = auth
        .redeem_set_password(
            &LinkSetPasswordParams {
                link: LinkToken::new(reset_token).unwrap(),
                password: Password::new("rotated-secret-passphrase".into()).unwrap(),
                client_address: "127.0.0.1".parse().unwrap(),
            },
            reset_now + 10,
            &mut ended,
        )
        .expect("redeem set-password link over MemoryHostStore");
    assert_eq!(updated.login, "Alice");
    assert_eq!(ended.len(), 2, "both prior browser sessions ended on reset");

    // 3. WebTls, Writers, and Provisioning compose cleanly over MemoryHostStore + MemorySecrets.
    let tls = WebTls::start(
        Arc::clone(&store),
        Arc::clone(&secrets),
        ListenOrigin::parse("https://127.0.0.1:8443").unwrap(),
        None,
        crate::web_tls::now().unwrap(),
    )
    .expect("WebTls over MemoryHostStore");
    assert!(store.web_tls_pair().unwrap().is_some());
    drop(tls);

    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let state_root = root.path().canonicalize().unwrap();
    let office_home = state_root.join("office-state");
    std::fs::create_dir_all(office_home.join(org_office_launch::OFFICES_DIR_NAME)).unwrap();
    let writers = Writers::open(
        Arc::clone(&secrets),
        Arc::clone(&store),
        INSTANCE,
        &state_root,
    )
    .expect("Writers over MemoryHostStore");
    let _provisioning = Provisioning::new(
        Arc::clone(&store),
        Arc::clone(&secrets),
        Arc::new(MemoryKeys::default()),
        writers,
        INSTANCE,
        &office_home,
    );
}
