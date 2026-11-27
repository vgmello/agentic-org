//! An HQ office for tests (ENG-2): created through the production creation path, in a
//! temporary folder, on the memory engine unless another is chosen, with a fake writer
//! custody that applies the host's rule for every confirmation (F4) and a fixed clock.
//! Its organization is `organization::testkit::creation_command`'s.

use std::{path::Path, sync::Arc};

use org_proto::selected_bootstrap::{CreatorV1, PlanV1, PreparedV1};
use org_store::{
    testkit::{identity, FakeCustody, FixedClock},
    WriterAuthority,
};

use super::{Custody, DataRoots, Office, Options};
use crate::{
    error::OfficeError,
    hq_creation::{Binding, LocalRecord},
    key::Key,
    model::{NewOffice, OfficeRole},
    office_local::testkit::ca_material,
    organization::{testkit::creation_command, testkit::CREATION_AT},
    store::open::EngineChoice,
};

/// The creation request's canonical UUIDv7 operation ID.
pub(crate) const OPERATION: &str = "01563e3a-b5d3-7d67-9931-87bee64c0af5";
/// The CA material's seed.
pub(crate) const CA_SEED: u8 = 1;

/// What [`Office::for_test`] makes.
#[derive(Clone, Copy, Debug)]
pub struct TestOffice {
    pub engine: EngineChoice,
}

impl Default for TestOffice {
    fn default() -> Self {
        Self {
            engine: EngineChoice::Memory,
        }
    }
}

/// The request of the suite's creation: the office and organization of
/// `org_store::testkit::identity`, with `creation_command`'s records.
pub(crate) fn request() -> (NewOffice, Binding) {
    let command = creation_command();
    let prepared = PreparedV1 {
        plan: PlanV1 {
            version: 1,
            name: command.name.clone(),
            slug: command.slug.clone(),
            repository_remote_url: command.repository_remote_url.clone(),
            state_branch: command.state_branch.clone(),
        },
        organization_id: command.organization_id.to_string(),
        office_id: command.office_id.to_string(),
        repository_id: command.repository_id.to_string(),
        home_team_id: command.team_id.to_string(),
        hr_employee_id: command.hr_employee_id.to_string(),
        creator: CreatorV1 {
            employee_id: command.creator.employee_id.to_string(),
            name: command.creator.name.clone(),
            handle: command.creator.handle.clone(),
        },
    };
    let new = NewOffice {
        office_id: prepared.office_id.clone(),
        office_name: command.office_name.clone(),
        role: OfficeRole::Hq,
        org_id: prepared.organization_id.clone(),
        org_name: prepared.plan.name.clone(),
    };
    let binding = Binding {
        operation_id: OPERATION.into(),
        owner_member_id: prepared.creator.employee_id.clone(),
        bootstrap: prepared,
    };
    (new, binding)
}

/// The CA record the suite's creation commits, made from `seed`.
pub(crate) fn record(seed: u8) -> Result<LocalRecord, OfficeError> {
    let here = identity();
    let material = ca_material(&here.organization_id, seed);
    LocalRecord::new(
        here.organization_id.to_string(),
        OPERATION.into(),
        material.public_key,
        material.package,
        material.wrapped_ca_key,
    )
}

/// The office folder and data roots under `root`. The roots are made, as the host makes
/// them when it reserves the office's custody.
pub(crate) fn places(root: &Path) -> (std::path::PathBuf, DataRoots) {
    let here = identity();
    let folder = root.join("home/offices").join(here.office_id.as_str());
    let data = root.join("data").join(here.organization_id.as_str());
    let roots = DataRoots {
        durable: data.join("durable"),
        runtime: data.join("runtime"),
    };
    for made in [&roots.durable, &roots.runtime] {
        std::fs::create_dir_all(made).expect("a data root");
    }
    (folder, roots)
}

pub(crate) fn custody(authority: Arc<FakeCustody>) -> Custody {
    Custody {
        identity: identity(),
        authority: authority as Arc<dyn WriterAuthority>,
    }
}

pub(crate) fn options(engine: EngineChoice) -> Options {
    Options {
        engine,
        clock: Arc::new(FixedClock(CREATION_AT)),
        testing: Default::default(),
        #[cfg(test)]
        before_final_checks: None,
    }
}

impl Office {
    /// A new HQ in a temporary folder that lives as long as the office.
    pub fn for_test(spec: TestOffice) -> Office {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path().canonicalize().expect("a real path");
        let (folder, roots) = places(&root);
        let (new, binding) = request();
        let custody = custody(Arc::new(FakeCustody::new()));
        let key = Key::from_bytes([7; 32]);
        let mut office = Office::provision_with(
            folder,
            &new,
            Some(key),
            &binding,
            roots,
            custody,
            |prior| prior.map_or_else(|| record(CA_SEED), Ok),
            options(spec.engine),
        )
        .expect("a test office");
        office._test_root = Some(dir);
        office
    }

    /// Like [`Self::for_test`], but provisions a real P-256 Org CA signing key wrapped under the
    /// office secrets key (matching production `hq_creation::prepare`) for mTLS/federation tests.
    pub fn for_test_with_real_ca(spec: TestOffice) -> Office {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path().canonicalize().expect("a real path");
        let (mut office, _, _, _) = Self::for_test_with_real_ca_in(&root, spec);
        office._test_root = Some(dir);
        office
    }

    /// Like [`Self::for_test_with_real_ca`], rooted in a caller-owned directory and returning the
    /// `(DataRoots, Custody, Key)` needed to reopen the HQ via `Office::open_as_owner` in tests.
    pub fn for_test_with_real_ca_in(
        root: &Path,
        spec: TestOffice,
    ) -> (Office, DataRoots, Custody, Key) {
        let (folder, roots) = places(root);
        let (new, binding) = request();
        let custody = custody(Arc::new(FakeCustody::new()));
        let key = Key::from_bytes([7; 32]);
        let key_for_wrap = key.clone();
        let org_id = new.org_id.clone();
        let office_id = new.office_id.clone();
        let mut opts = options(spec.engine);
        opts.clock = Arc::new(org_store::SystemClock);
        let office = Office::provision_with(
            folder,
            &new,
            Some(key.clone()),
            &binding,
            roots.clone(),
            custody.clone(),
            move |prior| {
                if let Some(p) = prior {
                    return Ok(p);
                }
                let ca = org_pki::SigningKey::generate().expect("generate CA key");
                let pub_key = ca.public_key();
                let pkg = crate::office_local::testkit::package(
                    &org_store::Id::parse(&org_id).unwrap(),
                    &pub_key,
                    CA_SEED,
                );
                let wrapped = org_pki::wrap::wrap(
                    key_for_wrap.expose_bytes(),
                    &org_pki::wrap::WrapBinding {
                        organization_id: &org_id,
                        office_id: &office_id,
                        purpose: org_pki::wrap::WrapPurpose::OrganizationCa,
                    },
                    ca.local_der(),
                )
                .expect("wrap CA key");
                LocalRecord::new(org_id, OPERATION.into(), pub_key, pkg, wrapped)
            },
            opts,
        )
        .expect("a real-CA test office");
        (office, roots, custody, key)
    }
}

/// A satellite office for tests outside this crate (`TASK-203`): provisioned on its own
/// temporary roots with a fresh Org CA and a certificate issued from it (no HQ runs), plus
/// what reopening it needs. Its organization state is empty until a test replicates some.
pub struct TestSatellite {
    pub office: Office,
    /// The HQ office the satellite is bound to.
    pub hq_office_id: String,
    folder: std::path::PathBuf,
    roots: DataRoots,
    custody: Custody,
    key: Key,
    _dir: tempfile::TempDir,
}

impl TestSatellite {
    pub fn new() -> Self {
        use org_pki::cert::{
            build_org_ca_cert, issue_satellite_cert_from_pubkey, sign_satellite_key_pop,
            IssueSatelliteCertParams,
        };
        const ORG: &str = "0195a000-0000-7000-8000-0000000000e1";
        const SATELLITE: &str = "0195a000-0000-7000-8000-0000000000e2";
        const HQ: &str = "0195a000-0000-7000-8000-0000000000e3";
        const INVITATION: &str = "0195a000-0000-7000-8000-0000000000e4";
        let id = |text: &str| org_store::Id::parse(text).expect("a test id");
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = dir.path().canonicalize().expect("a real path");
        let folder = root.join("home/offices").join(SATELLITE);
        let data = root.join("data").join(ORG);
        let roots = DataRoots {
            durable: data.join("durable"),
            runtime: data.join("runtime"),
        };
        for made in [&roots.durable, &roots.runtime] {
            std::fs::create_dir_all(made).expect("a data root");
        }
        let custody = Custody {
            identity: org_store::OfficeIdentity {
                instance_id: id("0195a000-0000-7000-8000-0000000000e5"),
                organization_id: id(ORG),
                office_id: id(SATELLITE),
                journal_incarnation: id("0195a000-0000-7000-8000-0000000000e6"),
                writer_epoch: 1,
                machine_id: id("0195a000-0000-7000-8000-0000000000e7"),
            },
            authority: Arc::new(FakeCustody::new()) as Arc<dyn WriterAuthority>,
        };
        let now_unix = (org_store::Clock::now(&org_store::SystemClock).0 / 1000) as i64;
        let ca_key = org_pki::SigningKey::generate().expect("a CA key");
        let ca = build_org_ca_cert(&ca_key, ORG, now_unix).expect("a CA certificate");
        let tls = org_pki::SigningKey::generate().expect("a satellite key");
        let pop = sign_satellite_key_pop(&tls, ORG, SATELLITE, INVITATION).expect("a proof");
        let cert = issue_satellite_cert_from_pubkey(
            &ca_key,
            &ca.cert_der,
            &IssueSatelliteCertParams {
                org_id: ORG,
                office_id: SATELLITE,
                invitation_id: INVITATION,
                subject_pub_key_uncompressed: &tls.public_key(),
                pop_signature_der: &pop,
                now_unix,
                validity_days: 365,
            },
        )
        .expect("a satellite certificate");
        let key = Key::from_bytes([0x5a; 32]);
        let office = Office::provision_satellite(
            folder.clone(),
            super::SatelliteProvisionSpec {
                new: NewOffice {
                    office_id: SATELLITE.into(),
                    office_name: "Test Satellite".into(),
                    role: OfficeRole::Satellite,
                    org_id: ORG.into(),
                    org_name: "Acme".into(),
                },
                operation_id: "0195a000-0000-7000-8000-0000000000e8".into(),
                hq_office_id: HQ.into(),
                hq_addresses: vec!["127.0.0.1:9443".into()],
                epoch: 1,
                ca_cert_der: ca.cert_der,
                office_cert_der: cert.cert_der,
                office_key_der: org_pki::Zeroizing::new(tls.local_der().to_vec()),
            },
            key.clone(),
            roots.clone(),
            custody.clone(),
        )
        .expect("a test satellite");
        Self {
            office,
            hq_office_id: HQ.into(),
            folder,
            roots,
            custody,
            key,
            _dir: dir,
        }
    }

    /// Closes the satellite, deletes its runtime root and opens it again, so every
    /// projection is rebuilt from the durable root.
    pub fn rebuilt(self) -> Self {
        let Self {
            office,
            hq_office_id,
            folder,
            roots,
            custody,
            key,
            _dir,
        } = self;
        office.close().expect("the satellite closes");
        std::fs::remove_dir_all(&roots.runtime).expect("the runtime root goes");
        std::fs::create_dir_all(&roots.runtime).expect("a fresh runtime root");
        let instance = custody.identity.instance_id.to_string();
        let office = Office::open(
            folder.clone(),
            Some(key.clone()),
            &instance,
            roots.clone(),
            custody.clone(),
        )
        .expect("the satellite reopens");
        Self {
            office,
            hq_office_id,
            folder,
            roots,
            custody,
            key,
            _dir,
        }
    }
}

impl Default for TestSatellite {
    fn default() -> Self {
        Self::new()
    }
}

/// Removes the built-in Chief of Staff's profile, as an organization created before
/// `TASK-80.1` lacks it (the shape the HQ-open bootstrap migrates).
struct ForgetChiefOfStaff {
    path: org_store::DurablePath,
}

impl crate::store::open::StoreOp for ForgetChiefOfStaff {
    type Out = Result<(), org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        store
            .submit(None, true, move |uow| {
                uow.next(org_store::Stream::Organization)?;
                Ok(org_store::Change::new(org_store::Ack::Durable)
                    .remove(self.path.clone())
                    .reply(b"{}".to_vec()))
            })
            .map(|_| ())
    }
}

/// Reads the chain of parents of a team, from its top-level team.
struct TeamChainOf(org_store::Id);

impl crate::store::open::ReadOp for TeamChainOf {
    type Out = Option<Vec<org_store::Id>>;

    fn read<S: crate::tasks::ports::TasksRead>(
        self,
        read: &S,
    ) -> Result<Self::Out, org_store::StoreError> {
        Ok(read.team_chain(&self.0)?)
    }
}

impl Office {
    /// Test-only: forgets the organization's built-in Chief of Staff, as an organization
    /// created before `TASK-80.1` never had her. The open-time bootstrap
    /// ([`Office::ensure_chief_of_staff`]) provisions her again.
    pub fn forget_chief_of_staff_for_test(&self) -> Result<(), OfficeError> {
        let Some(tess) = self.chief_of_staff()? else {
            return Ok(());
        };
        let chain = self
            .store
            .read(TeamChainOf(tess.home_team_id.clone()))
            .map_err(super::storage)?
            .0
            .ok_or(OfficeError::Invalid("her team has no chain"))?;
        let path = crate::organization::records::employee_path(&chain, tess.kind, &tess.id)
            .map_err(super::storage)?;
        self.store
            .run(ForgetChiefOfStaff { path })
            .map_err(super::storage)
    }
}
