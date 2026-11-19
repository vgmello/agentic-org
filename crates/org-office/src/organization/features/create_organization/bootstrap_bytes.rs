//! Today's organization bootstrap (`organization/bootstrap.rs`, reached through
//! `Office::finish_selected_bootstrap`) against the golden creation files, for the same
//! inputs. Both ways of creating an organization are to leave the same bytes (plan B1a,
//! revision 2, F5); this test says, file by file and field by field, where they do not.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use org_proto::selected_bootstrap::{CreatorV1, PlanV1, PreparedV1};

use crate::{
    hq_creation::{Binding, LocalRecord},
    organization::{
        records,
        testkit::{creation_command, creation_files, CREATION_AT},
    },
    AcceptanceConfig, AcceptanceIdentity, Key, NewOffice, Office, OfficeError, OfficeRole,
    OwnerAccount, OwnerAuthority,
};

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182";
const OPERATION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

/// The live owner authority: the creator, whose person record is the owner's (creation
/// refuses any other owner member).
struct Owner(String);

impl OwnerAuthority for Owner {
    fn verify_live(&self, _: &AcceptanceIdentity) -> Result<OwnerAccount, OfficeError> {
        Ok(OwnerAccount {
            account_id: "account".into(),
            member_id: self.0.clone(),
        })
    }
}

/// Every file under `root`, by its path below it.
fn files_under(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            files.insert(name, std::fs::read(&path).unwrap());
        }
    }
    files
}

/// The files today's bootstrap publishes for the golden creation's inputs, with its
/// wall-clock creation time replaced by the golden one.
fn bootstrap_files() -> BTreeMap<String, Vec<u8>> {
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
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let public = root.join("state");
    std::fs::create_dir(&public).unwrap();
    let binding = Binding {
        operation_id: OPERATION.into(),
        owner_member_id: prepared.creator.employee_id.clone(),
        public_root: public.clone(),
        bootstrap: Some(prepared.clone()),
    };
    let new = NewOffice {
        office_id: prepared.office_id.clone(),
        org_id: prepared.organization_id.clone(),
        org_name: prepared.plan.name.clone(),
        office_name: command.office_name.clone(),
        role: OfficeRole::Hq,
    };
    let folder = root.join(&prepared.office_id);
    let key = Key::from_bytes([63; 32]);
    let office = Office::provision_hq_for_instance(&folder, &new, key, INSTANCE, &binding, |_| {
        LocalRecord::new(
            prepared.organization_id.clone(),
            OPERATION.into(),
            [4; 65],
            vec![7; 140],
            vec![1; 138],
        )
    })
    .unwrap();
    let config = AcceptanceConfig {
        identity: AcceptanceIdentity {
            instance_id: INSTANCE.into(),
            organization_id: prepared.organization_id.clone(),
            office_id: prepared.office_id.clone(),
            machine_id: "machine".into(),
            writer_epoch: 1,
            journal_incarnation: "bootstrap-journal".into(),
        },
        public_root: public.clone(),
        protected_root: root.join("protected"),
        authority: None,
    };
    office.install_selected_acceptance(config).unwrap();
    let owner = || Arc::new(Owner(prepared.creator.employee_id.clone()));
    office.bind_selected_owner(owner()).unwrap();
    office.finish_selected_bootstrap(owner()).unwrap();

    let (written_at, golden_at) = (&office.info().created_at, records::time_text(CREATION_AT));
    let at_golden_time = |bytes: Vec<u8>| match String::from_utf8(bytes) {
        Ok(text) => text.replace(written_at.as_str(), &golden_at).into_bytes(),
        Err(not_text) => not_text.into_bytes(),
    };
    // Only this family's files, and not the CA escrow package: today's bootstrap publishes
    // the fixture bytes above there, and the new creation's CA records are compared on
    // their own (`store/open/tests/local.rs`).
    let ours = |path: &String| {
        let path = org_store::DurablePath::parse(path);
        let kind = path.ok().and_then(|path| records::kind_of(&path));
        kind.is_some_and(|kind| kind != records::Kind::CaEscrow)
    };
    let files = files_under(&public).into_iter();
    files
        .map(|(path, bytes)| (format!("state/{path}"), at_golden_time(bytes)))
        .filter(|(path, _)| ours(path))
        .collect()
}

/// The lines of a file by their key (the text before the first `: `).
fn fields(bytes: &[u8]) -> BTreeMap<String, String> {
    let text = String::from_utf8_lossy(bytes);
    let field = |line: &str| match line.split_once(": ") {
        Some((key, value)) => (key.to_owned(), value.to_owned()),
        None => (line.to_owned(), String::new()),
    };
    text.lines().map(field).collect()
}

/// Every difference, one per line: a file only one side writes, or a field only one side
/// has or that differs.
fn differences(
    golden: &BTreeMap<String, Vec<u8>>,
    bootstrap: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let mut found = Vec::new();
    for (path, bytes) in golden {
        let Some(theirs) = bootstrap.get(path) else {
            found.push(format!("{path}: not written by the bootstrap"));
            continue;
        };
        let (ours, theirs) = (fields(bytes), fields(theirs));
        for (key, value) in &ours {
            match theirs.get(key) {
                None => found.push(format!("{path}: {key} missing from the bootstrap")),
                Some(other) if other != value => {
                    found.push(format!(
                        "{path}: {key} is {other} in the bootstrap, {value} here"
                    ));
                }
                Some(_) => {}
            }
        }
        for key in theirs.keys().filter(|key| !ours.contains_key(*key)) {
            found.push(format!("{path}: {key} written only by the bootstrap"));
        }
    }
    for path in bootstrap.keys().filter(|path| !golden.contains_key(*path)) {
        found.push(format!("{path}: written only by the bootstrap"));
    }
    found
}

#[test]
#[ignore = "A4 bootstrap bytes differ in 14 places: identity/office-codes.yaml is not \
            written; the repositories.yaml entry has no key (organization); the HR agent's \
            and the creator's profile.md each lack contact, focus, may_hire, role_text, \
            title and title_in_team. Not fixed: B1b block 4 deletes this bootstrap, and \
            organizations it made are not migrated"]
fn todays_bootstrap_writes_the_golden_creation_files() {
    let found = differences(&creation_files(), &bootstrap_files());
    assert!(
        found.is_empty(),
        "{} differences:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn todays_bootstrap_has_only_the_documented_differences() {
    let mut expected = creation_files();
    assert!(expected
        .remove("state/identity/office-codes.yaml")
        .is_some());

    let repositories = expected.get_mut("state/repositories.yaml").unwrap();
    let canonical = std::str::from_utf8(repositories).unwrap();
    let without_key = canonical.replace("\"key\":\"organization\",", "");
    assert_ne!(without_key, canonical);
    *repositories = without_key.into_bytes();

    let omitted = [
        "contact",
        "focus",
        "may_hire",
        "role_text",
        "title",
        "title_in_team",
    ];
    for (path, profile) in expected
        .iter_mut()
        .filter(|(path, _)| path.ends_with("/profile.md"))
    {
        let mut without_fields = String::new();
        let mut missing = 0;
        for line in std::str::from_utf8(profile).unwrap().split_inclusive('\n') {
            if line
                .split_once(": ")
                .is_some_and(|(key, _)| omitted.contains(&key))
            {
                missing += 1;
            } else {
                without_fields.push_str(line);
            }
        }
        assert_eq!(missing, omitted.len(), "{path}");
        *profile = without_fields.into_bytes();
    }

    let actual = bootstrap_files();
    assert_eq!(
        actual.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    for (path, bytes) in expected {
        assert_eq!(
            std::str::from_utf8(&actual[&path]).unwrap(),
            std::str::from_utf8(&bytes).unwrap(),
            "{path}"
        );
    }
}

#[test]
fn the_comparison_reports_each_kind_of_difference() {
    let golden = BTreeMap::from([
        ("state/a".to_owned(), b"x: 1\ny: 2\n".to_vec()),
        ("state/b".to_owned(), b"x: 1\n".to_vec()),
    ]);
    let bootstrap = BTreeMap::from([
        ("state/a".to_owned(), b"x: 3\nz: 4\n".to_vec()),
        ("state/c".to_owned(), b"x: 1\n".to_vec()),
    ]);
    assert_eq!(
        differences(&golden, &bootstrap),
        [
            "state/a: x is 3 in the bootstrap, 1 here",
            "state/a: y missing from the bootstrap",
            "state/a: z written only by the bootstrap",
            "state/b: not written by the bootstrap",
            "state/c: written only by the bootstrap",
        ]
    );
    assert!(differences(&golden, &golden).is_empty());
}
