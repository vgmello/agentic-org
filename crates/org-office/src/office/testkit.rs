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
    organization::{operation_id, testkit::creation_command, testkit::CREATION_AT},
    store::open::EngineChoice,
};

/// The creation request's ULID, as the launch binding carries it.
pub(crate) const OPERATION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
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
    let operation = operation_id::from_wire_ulid(OPERATION).expect("a ULID");
    LocalRecord::new(
        here.organization_id.to_string(),
        operation.to_string(),
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
}
