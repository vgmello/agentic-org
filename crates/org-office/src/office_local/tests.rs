use org_store::testkit::{id, identity};

use super::*;
use crate::office_local::testkit::ca_material;

/// The suite's CA records: material of the identity's organization, made by operation 1.
fn records() -> CaRecords {
    let material = ca_material(&identity().organization_id, 1);
    CaRecords {
        key: material.record(&identity(), &id(1)),
        package: material.package,
    }
}

fn check(records: &CaRecords) -> Result<(), CaError> {
    let here = identity();
    records.check(&here.organization_id, &here.office_id, &id(1))
}

#[test]
fn records_that_match_pass_and_each_mismatch_is_refused() {
    assert_eq!(check(&records()), Ok(()));
    let here = identity();
    let mismatch = |result: Result<(), CaError>| matches!(result, Err(CaError::Mismatch(_)));
    let ca = records();
    assert!(
        mismatch(ca.check(&id(0xB2), &here.office_id, &id(1))),
        "organization"
    );
    assert!(
        mismatch(ca.check(&here.organization_id, &id(0xB3), &id(1))),
        "office"
    );
    assert!(
        mismatch(ca.check(&here.organization_id, &here.office_id, &id(2))),
        "operation"
    );

    // A package whose hash is not the recorded one.
    let mut other_package = records();
    other_package.package = ca_material(&here.organization_id, 2).package;
    assert!(mismatch(check(&other_package)), "package hash");
    // The recorded hash matches, but the package carries another public key.
    let mut other_key = records();
    other_key.key.public_key = ca_material(&here.organization_id, 3).public_key;
    assert!(mismatch(check(&other_key)), "public key");
    // A package sealed for another organization, recorded faithfully.
    let foreign = ca_material(&id(0xB2), 1);
    let mut sealed_elsewhere = records();
    sealed_elsewhere.key.package_sha256 = Sha256::of(&foreign.package);
    sealed_elsewhere.package = foreign.package;
    assert!(
        mismatch(check(&sealed_elsewhere)),
        "sealed for another organization"
    );
}

/// The two files under a temporary durable root, either of them left out.
fn files_with(key: bool, package: bool) -> (tempfile::TempDir, Files) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let ca = records();
    let write = |path: &DurablePath, bytes: &[u8]| {
        let full = root.join(path.as_str());
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, bytes).unwrap();
    };
    if key {
        write(
            &records::ca_key_path(&identity().office_id),
            &records::encode_ca_key(&ca.key),
        );
    }
    if package {
        write(&organization::ca_escrow_path(), &ca.package);
    }
    let files = Files::open(&root).unwrap();
    (dir, files)
}

#[test]
fn read_ca_reads_both_files_none_or_names_the_missing_half() {
    let office = identity().office_id;
    let (_dir, files) = files_with(true, true);
    assert_eq!(read_ca(&files, &office), Ok(Some(records())));
    let (_dir, files) = files_with(false, false);
    assert_eq!(read_ca(&files, &office), Ok(None));
    let (_dir, files) = files_with(true, false);
    let missing = Err(CaError::Missing(organization::ca_escrow_path()));
    assert_eq!(read_ca(&files, &office), missing);
    let (_dir, files) = files_with(false, true);
    let missing = Err(CaError::Missing(records::ca_key_path(&office)));
    assert_eq!(read_ca(&files, &office), missing);
}

#[test]
fn a_key_record_out_of_its_form_is_named_by_its_path() {
    let office = identity().office_id;
    let (dir, files) = files_with(true, true);
    let path = records::ca_key_path(&office);
    let full = dir.path().canonicalize().unwrap().join(path.as_str());
    let mut bytes = std::fs::read(&full).unwrap();
    bytes.pop();
    std::fs::write(&full, bytes).unwrap();
    let read = read_ca(&files, &office);
    assert!(
        matches!(read, Err(CaError::Malformed(ref at, _)) if *at == path),
        "{read:?}"
    );
}

#[test]
fn a_ca_file_that_cannot_be_read_is_a_files_error() {
    let office = identity().office_id;
    // The key record's place holds a folder; then the escrow package's holds a link.
    let (dir, files) = files_with(false, true);
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(records::ca_key_path(&office).as_str())).unwrap();
    let read = read_ca(&files, &office);
    assert!(matches!(read, Err(CaError::Files(_))), "{read:?}");

    let (dir, files) = files_with(true, false);
    let root = dir.path().canonicalize().unwrap();
    let outside = root.join("outside");
    std::fs::write(&outside, records().package).unwrap();
    let escrow = root.join(organization::ca_escrow_path().as_str());
    std::fs::create_dir_all(escrow.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, &escrow).unwrap();
    let read = read_ca(&files, &office);
    assert!(matches!(read, Err(CaError::Files(_))), "{read:?}");
}
