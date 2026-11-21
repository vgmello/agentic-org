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
const OP_ID: &str = "01J00000000000000000000010";
const OP_ID_2: &str = "01J00000000000000000000011";
const PHC: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

struct Harness<'a> {
    store: &'a dyn HostStore,
    seed: &'a dyn Fn(&str, &str, bool),
    revoke: &'a dyn Fn(&AccountId),
}

fn each(case: impl Fn(Harness<'_>)) {
    let verifier = Verifier::new(PHC.into()).unwrap();
    let now = org_proto::ulid_time_ms(OP_ID).unwrap() as i64;

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
        let now = org_proto::ulid_time_ms(OP_ID).unwrap() as i64;
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
        let base = org_proto::ulid_time_ms(OP_ID).unwrap();
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
        let now = org_proto::ulid_time_ms(OP_ID).unwrap() as i64;
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
        h.store.provision_linked(&r).unwrap();
        assert!(h.store.provisioning_reserved().unwrap().is_empty());
        let linked = h
            .store
            .provisioning_linked_to(ORG_ID)
            .unwrap()
            .expect("linked");
        assert!(linked.linked);
        assert_eq!(linked.key_fingerprint.as_deref(), Some("fp-1"));
    });
}

#[test]
fn web_tls_pair_and_journal_replay_conformance() {
    each(|h| {
        let now = org_proto::ulid_time_ms(OP_ID).unwrap() as i64;
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
        let now = org_proto::ulid_time_ms(OP_ID).unwrap() as i64;
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
