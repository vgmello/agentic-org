use super::*;
use crate::host_secrets::Value;
use org_proto::selected_bootstrap::{PlanV1, PreparedV1};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[cfg(debug_assertions)]
pub(super) mod crash;

struct Fixture {
    _dir: tempfile::TempDir,
    db: Arc<HostDb>,
    custody: Arc<dyn HostSecrets>,
    r: provisioning::Reservation,
    public: PublicRecord,
}
fn fixture() -> Fixture {
    let instance = crate::ids::uuid_v7().unwrap();
    let (dir, auth, _) = crate::authentication::tests::fixture_for(&instance);
    let (db, custody) = auth.provisioning_storage();
    let credentials = db
        .account_credentials(&crate::accounts::identity::LoginKey::new("Owner").unwrap())
        .unwrap()
        .unwrap();
    let id =
        crate::accounts::identity::AccountId::parse(credentials.account().account_id()).unwrap();
    let account = db.account_authority(&id).unwrap().unwrap();
    let op = crate::ids::uuid_v7().unwrap();
    let owner_member = crate::ids::uuid_v7().unwrap();
    let r = provisioning::Reservation {
        operation_id: op.clone(),
        account: account.account_id().into(),
        fingerprint: [7; 32],
        prepared: PreparedV1 {
            plan: PlanV1 {
                version: 1,
                name: "Example".into(),
                slug: "example".into(),
                repository_remote_url: "https://example.test/org.git".into(),
                state_branch: "org/state".into(),
            },
            organization_id: crate::ids::uuid_v7().unwrap(),
            office_id: crate::ids::uuid_v7().unwrap(),
            repository_id: crate::ids::uuid_v7().unwrap(),
            home_team_id: crate::ids::uuid_v7().unwrap(),
            hr_employee_id: crate::ids::uuid_v7().unwrap(),
            creator: org_proto::selected_bootstrap::CreatorV1::from_login(
                owner_member.clone(),
                "Ana",
            ),
        },
        owner_member,
        recovery_handle: format!("recovery.{op}"),
        key_fingerprint: None,
        linked: false,
    };
    db.reserve_provisioning(&account, &r).unwrap();
    let code = "ABCD-ABCD-ABCD-ABCD-ABCD-ABCD-ABCD-ABCD-ABCD-ABCD";
    custody
        .put(
            &Name::parse(&r.recovery_handle).unwrap(),
            &Value::new(
                serde_json::to_vec(&serde_json::json!({"version":1,"reservation":r,"code":code}))
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    db.provision_key_stored(&r, &"a".repeat(64)).unwrap();
    db.provision_linked(&r).unwrap();
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
    let public = PublicRecord {
        version: 1,
        instance_id: instance,
        office_id: r.prepared.office_id.clone(),
        organization_id: r.prepared.organization_id.clone(),
        operation_id: op,
        escrow_version: 1,
        public_key: org_office_launch::recovery::hex(&key),
        package_sha256: org_office_launch::recovery::hex(&Sha256::digest(&bytes)),
        package_hex: org_office_launch::recovery::hex(&bytes),
    };
    Fixture {
        _dir: dir,
        db,
        custody,
        r,
        public,
    }
}
#[cfg(debug_assertions)]
#[test]
fn unlinked_reservation_reopen_requires_original_recovery_custody() {
    use crate::host_secrets::dev::DevSecrets;
    use std::os::unix::fs::PermissionsExt;

    for case in ["healthy", "missing", "malformed", "mismatched"] {
        let Fixture {
            _dir: home,
            db,
            custody: memory,
            r,
            public,
        } = fixture();
        // A worker can commit its HQ before the host records provision_linked.
        // Reproduce that durable host state without creating a delivery ledger.
        let frozen = {
            let connection = db.connection.lock().unwrap();
            connection
                .execute(
                    "UPDATE selected_provisioning SET linked=0 WHERE operation_id=?1",
                    [&r.operation_id],
                )
                .unwrap();
            let reservation = provisioning::read(&connection, &r.operation_id)
                .unwrap()
                .unwrap();
            assert!(!reservation.linked && reservation.key_fingerprint.is_some());
            serde_json::to_value(reservation).unwrap()
        };
        let base = tempfile::tempdir().unwrap();
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let stored = DevSecrets::open(base.path(), home.path(), &public.instance_id).unwrap();
        for name in memory.list().unwrap() {
            stored
                .put(&name, &memory.load(&name).unwrap().unwrap())
                .unwrap();
        }
        let name = Name::parse(&r.recovery_handle).unwrap();
        match case {
            "missing" => stored.delete(&name).unwrap(),
            "malformed" => stored
                .put(&name, &Value::new(b"{".to_vec()).unwrap())
                .unwrap(),
            "mismatched" => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(stored.load(&name).unwrap().unwrap().expose()).unwrap();
                value["reservation"]["operation_id"] = crate::ids::uuid_v7().unwrap().into();
                stored
                    .put(
                        &name,
                        &Value::new(serde_json::to_vec(&value).unwrap()).unwrap(),
                    )
                    .unwrap();
            }
            "healthy" => {}
            _ => unreachable!(),
        }
        let mut names = stored.list().unwrap();
        names.sort();
        let retained: Vec<_> = names
            .iter()
            .map(|name| (name.clone(), stored.load(name).unwrap().unwrap()))
            .collect();
        drop(db);
        drop(memory);
        drop(stored);

        let custody: Arc<dyn HostSecrets> =
            Arc::new(DevSecrets::open(base.path(), home.path(), &public.instance_id).unwrap());
        // Production startup reconciles before starting any catalogued office.
        let reopened = crate::authentication::Authentication::open(
            home.path(),
            custody.clone(),
            &public.instance_id,
        );
        assert_eq!(reopened.is_ok(), case == "healthy", "{case}");
        drop(reopened);

        let db = HostDb::open(home.path(), &*custody).unwrap();
        let connection = db.connection.lock().unwrap();
        let reservation = provisioning::read(&connection, &r.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(serde_json::to_value(reservation).unwrap(), frozen);
        let count: i64 = connection
            .query_row(
                "SELECT (SELECT count(*) FROM selected_recovery) + (SELECT count(*) FROM selected_recovery_audit)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "startup cannot infer delivery or acknowledgement");
        let mut after = custody.list().unwrap();
        after.sort();
        assert_eq!(after, names);
        for (name, value) in retained {
            assert!(custody.load(&name).unwrap().unwrap().expose() == value.expose());
        }
    }
}
#[test]
fn durable_show_redelivery_ack_intent_deletion_and_exact_retry() {
    let f = fixture();
    let db = &f.db;
    let waiting = db.recovery_prepare(&f.public).unwrap();
    assert_eq!(waiting.state, State::Pending);
    assert_eq!(db.recovery_prepare(&f.public).unwrap(), waiting);
    let first = db
        .recovery_delivered(&waiting, &f.r.account, true, 10)
        .unwrap();
    assert_eq!(first.ordinal, 1);
    assert_eq!(first.redelivery_count, 0);
    assert!(db
        .recovery_delivered(&first, &f.r.account, false, 11)
        .is_err());
    assert!(db
        .recovery_delivered(&waiting, &f.r.account, true, 11)
        .is_err());
    let second = db
        .recovery_delivered(&first, &f.r.account, true, i64::MAX - 1)
        .unwrap();
    assert_eq!(second.ordinal, 2);
    assert_eq!(second.redelivery_count, 1);
    assert!(crate::hostdb::pending_recovery::pending_recovery(
        &*f.custody,
        &db.recovery_source(&f.public.organization_id).unwrap()
    )
    .is_ok());
    let intent = db.recovery_ack_intent(&second, &f.r.account, 12).unwrap();
    assert_eq!(intent.state, State::AckPending);
    assert_eq!(intent.acknowledged_ms, Some(i64::MAX - 1));
    assert!(
        f.custody
            .load(&Name::parse(&f.r.recovery_handle).unwrap())
            .unwrap()
            .is_some(),
        "intent precedes deletion"
    );
    assert!(db
        .recovery_delivered(&intent, &f.r.account, true, 13)
        .is_err());
    db.recovery_reconcile(&*f.custody).unwrap();
    let done = db.recovery_prepare(&f.public).unwrap();
    assert_eq!(done.state, State::Acknowledged);
    assert_eq!(done.acknowledged_ms, intent.acknowledged_ms);
    assert_eq!(done.acknowledged_by, intent.acknowledged_by);
    assert!(f
        .custody
        .load(&Name::parse(&f.r.recovery_handle).unwrap())
        .unwrap()
        .is_none());
    let c = db.connection.lock().unwrap();
    let count: i64 = c
        .query_row("SELECT count(*) FROM selected_recovery_audit", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 4);
    drop(c);
    db.recovery_reconcile(&*f.custody).unwrap();
    assert_eq!(db.recovery_prepare(&f.public).unwrap(), done);
    assert!(db
        .recovery_delivered(&done, &f.r.account, true, 14)
        .is_err());
}
#[test]
fn missing_custody_is_not_acknowledgement_and_public_identity_never_changes() {
    let f = fixture();
    let waiting = f.db.recovery_prepare(&f.public).unwrap();
    let mut changed = f.public.clone();
    changed.package_sha256 = "b".repeat(64);
    assert!(f.db.recovery_prepare(&changed).is_err());
    assert_eq!(f.db.recovery_prepare(&f.public).unwrap(), waiting);
    f.custody
        .delete(&Name::parse(&f.r.recovery_handle).unwrap())
        .unwrap();
    assert!(f.db.recovery_reconcile(&*f.custody).is_err());
    assert_eq!(
        f.db.recovery_prepare(&f.public).unwrap().state,
        State::Pending
    );
}
#[test]
fn lost_reply_after_delete_reconciles_frozen_actor_and_dates_without_secret() {
    let f = fixture();
    let initial = f.db.recovery_prepare(&f.public).unwrap();
    let shown =
        f.db.recovery_delivered(&initial, &f.r.account, false, 100)
            .unwrap();
    let intent = f.db.recovery_ack_intent(&shown, &f.r.account, 200).unwrap();
    f.custody
        .delete(&Name::parse(&f.r.recovery_handle).unwrap())
        .unwrap();
    let reopened = HostDb::open(f._dir.path(), &*f.custody).unwrap();
    reopened.recovery_reconcile(&*f.custody).unwrap();
    let done = reopened.recovery_prepare(&f.public).unwrap();
    assert_eq!(done.state, State::Acknowledged);
    assert_eq!(done.acknowledged_by, intent.acknowledged_by);
    assert_eq!(done.acknowledged_ms, intent.acknowledged_ms);
}

#[test]
fn status_pages_keep_ack_actions_with_their_delivery_and_reject_stale_reads() {
    let f = fixture();
    let mut l = f.db.recovery_prepare(&f.public).unwrap();
    let waiting = f.db.recovery_status(&l, 0).unwrap();
    assert_eq!(
        waiting.state,
        org_host_control::recovery::DeliveryState::Pending
    );
    assert!(waiting.audit.is_empty());
    for n in 1..=33 {
        l = f.db.recovery_delivered(&l, &f.r.account, true, n).unwrap();
    }
    l = f.db.recovery_ack_intent(&l, &f.r.account, 34).unwrap();
    let first = f.db.recovery_status(&l, 0).unwrap();
    assert_eq!(first.audit.len(), 32);
    assert_eq!(first.next_after_ordinal.as_deref(), Some("32"));
    let last = f.db.recovery_status(&l, 32).unwrap();
    assert_eq!(last.audit.len(), 2);
    assert_eq!(last.audit[0].action, "show");
    assert_eq!(last.audit[1].action, "ack_pending");
    assert!(last.audit.iter().all(|a| a.ordinal == "33"));
    assert!(!last.hq_loss_recovery_supported);
    assert!(f.db.recovery_status(&l, -1).is_err());
    f.db.recovery_reconcile(&*f.custody).unwrap();
    assert!(f.db.recovery_status(&l, 32).is_err());
    let committed = f.db.recovery_observed(&l.binding.operation_id).unwrap();
    let last = f.db.recovery_status(&committed, 32).unwrap();
    assert_eq!(last.audit.len(), 3);
    assert_eq!(last.audit[2].action, "acknowledged");
    assert_eq!(last.acknowledged_ms, l.acknowledged_ms);
}
struct DeleteFault {
    inner: Arc<dyn HostSecrets>,
    db: Arc<HostDb>,
    after: bool,
}
impl HostSecrets for DeleteFault {
    fn put(&self, n: &Name, v: &Value) -> Result<(), crate::host_secrets::Error> {
        self.inner.put(n, v)
    }
    fn load(&self, n: &Name) -> Result<Option<Value>, crate::host_secrets::Error> {
        self.inner.load(n)
    }
    fn list(&self) -> Result<Vec<Name>, crate::host_secrets::Error> {
        self.inner.list()
    }
    fn delete(&self, n: &Name) -> Result<(), crate::host_secrets::Error> {
        assert!(
            self.db.connection.try_lock().is_ok(),
            "provider called while DB locked"
        );
        if self.after {
            self.inner.delete(n)?;
        }
        Err(crate::host_secrets::Error::Io(std::io::ErrorKind::Other))
    }
}
#[test]
fn deletion_failures_before_and_after_mutation_resume_only_the_frozen_intent() {
    for after in [false, true] {
        let f = fixture();
        let pending = f.db.recovery_prepare(&f.public).unwrap();
        let shown =
            f.db.recovery_delivered(&pending, &f.r.account, false, 100)
                .unwrap();
        let intent = f.db.recovery_ack_intent(&shown, &f.r.account, 101).unwrap();
        let fault = DeleteFault {
            inner: Arc::clone(&f.custody),
            db: Arc::clone(&f.db),
            after,
        };
        assert!(f.db.recovery_reconcile(&fault).is_err());
        assert_eq!(
            f.db.recovery_observed(&intent.binding.operation_id)
                .unwrap(),
            intent
        );
        assert_eq!(
            f.custody
                .load(&Name::parse(&f.r.recovery_handle).unwrap())
                .unwrap()
                .is_none(),
            after
        );
        assert!(f
            .db
            .recovery_delivered(&intent, &f.r.account, true, 102)
            .is_err());
        f.db.recovery_reconcile(&*f.custody).unwrap();
        f.db.recovery_reconcile(&*f.custody).unwrap();
        let done =
            f.db.recovery_observed(&intent.binding.operation_id)
                .unwrap();
        assert_eq!(done.state, State::Acknowledged);
        assert_eq!(done.acknowledged_by, intent.acknowledged_by);
        assert_eq!(done.acknowledged_ms, intent.acknowledged_ms);
        assert_eq!(f.db.recovery_status(&done, 0).unwrap().audit.len(), 3);
    }
}
#[test]
fn malformed_pending_code_and_duplicate_completed_organization_fail_closed() {
    let f = fixture();
    let r = f.db.recovery_source(&f.r.prepared.organization_id).unwrap();
    let name = Name::parse(&r.recovery_handle).unwrap();
    f.custody.put(&name,&Value::new(serde_json::to_vec(&serde_json::json!({"version":1,"reservation":f.r,"code":"not-a-recovery-code"})).unwrap()).unwrap()).unwrap();
    assert!(crate::hostdb::pending_recovery::pending_recovery(&*f.custody, &r).is_err());
    let db = f.db.connection.lock().unwrap();
    let second = crate::ids::uuid_v7().unwrap();
    db.execute("INSERT INTO selected_provisioning SELECT ?1,account,fingerprint,'different-name','different-slug',prepared,owner_member,?2,key_fingerprint,linked,expires_at_ms FROM selected_provisioning WHERE operation_id=?3",params![second,format!("recovery.{second}"),r.operation_id]).unwrap();
    drop(db);
    assert!(f.db.recovery_source(&r.prepared.organization_id).is_err());
}
