//! Shared engine-independent `HostStore` conformance cases (ADR-0018 ENG-2 / ENG-3).

mod core_cases;
mod multi_person_and_journal;
mod subsystems_e2e;

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use org_office_launch::recovery::PublicRecord;
use org_proto::selected_bootstrap::{CreatorV1, PlanV1, PreparedV1};
use sha2::{Digest, Sha256};

use crate::accounts::identity::AccountId;
use crate::host_secrets::{HostSecrets, Name, Value};
use crate::hostdb::journal::PruneOutcome;
use crate::hostdb::provisioning::Reservation;
use crate::hostdb::{Error, HostStore};

pub use core_cases::{
    account_preferences_and_local_authless_lifecycle, credentials_lookup_and_admin_guarded_links,
    instance_verification_and_organization_ownership, login_lockout_backoff_and_pruning,
    provisioning_lifecycle, recovery_ledger_and_reconciliation, web_tls_pair_and_journal_replay,
};
pub use multi_person_and_journal::{
    journal_prune_batch_cap_and_startup_wiring,
    multi_person_invites_resets_handovers_and_access_removal, operation_journal_lookup_and_commit,
    operation_journal_prune_expired_and_rollback,
};
pub use subsystems_e2e::live_host_subsystems_run_end_to_end;

pub const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
pub const OTHER_INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b1";
pub(super) const ADMIN_ID: &str = "01J0000000000000000000000A";
pub(super) const MEMBER_ACC_ID: &str = "01J0000000000000000000000B";
pub(super) const ORG_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";
pub(super) const OFFICE_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c1";
pub(super) const MEMBER_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c2";
pub const OP_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607110";
pub(super) const OP_ID_2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607111";
pub const PHC: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

pub fn default_now_ms() -> i64 {
    org_proto::uuid_v7_time_ms(OP_ID).unwrap() as i64
}

pub struct HostStoreHarness<'a> {
    pub store: &'a dyn HostStore,
    pub seed: &'a dyn Fn(&str, &str, bool),
    pub revoke: &'a dyn Fn(&AccountId),
    pub seed_exempt_journal: &'a dyn Fn(&str, &str, &str, i64),
}

pub type PruneCappedFn = Box<dyn Fn(i64, usize) -> Result<PruneOutcome, Error>>;
pub type SeedExemptJournalFn = Box<dyn Fn(&str, &str, &str, i64)>;
pub type OpVerifierAccountsFn = Box<dyn Fn() -> Vec<String>>;

pub struct JournalPruneHarness {
    pub store: Arc<dyn HostStore>,
    pub prune_capped: PruneCappedFn,
    pub seed_exempt_journal: SeedExemptJournalFn,
    pub op_verifier_accounts: OpVerifierAccountsFn,
    pub keep_alive: Box<dyn Any>,
}

pub(super) fn uuid_v7_at(at: i64, sequence: u64) -> String {
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

pub(super) fn sample_reservation(
    op: &str,
    name: &str,
    slug: &str,
    org: &str,
    office: &str,
) -> Reservation {
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

#[derive(Default)]
pub(super) struct MemorySecrets(Mutex<BTreeMap<Name, Vec<u8>>>);

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

pub(super) fn sample_public_record(r: &Reservation) -> PublicRecord {
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
