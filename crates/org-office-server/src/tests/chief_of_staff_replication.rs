//! `TASK-80.1` pass 2: an organization created before the built-in Chief of Staff existed gets
//! her once at its HQ (`ensure_chief_of_staff`); she replicates to an enrolled satellite as the
//! same member, and deleting and rebuilding either office's projection never duplicates her.

use std::sync::Arc;
use std::time::Duration;

use org_office::organization_family::records::BuiltinKey;
use org_office::{EngineChoice, Key, Office, TestOffice};

use super::a2a_messaging::{enroll_satellite, sat_custody, wait_until, LiveOfficeNode};

const SAT_SUFFIX: u8 = 61;

fn chiefs_of_staff(office: &Office) -> Vec<(String, String)> {
    office
        .list_employees()
        .unwrap()
        .into_iter()
        .filter(|e| e.builtin_key == Some(BuiltinKey::ChiefOfStaff))
        .map(|e| (e.id.to_string(), e.handle))
        .collect()
}

/// Closes `office`, deletes its projection (the runtime root) and opens it again from its
/// durable files.
fn rebuilt(
    office: Office,
    folder: std::path::PathBuf,
    key: Key,
    roots: org_office::DataRoots,
    custody: org_office::Custody,
) -> Office {
    office.close().unwrap();
    std::fs::remove_dir_all(&roots.runtime).unwrap();
    std::fs::create_dir_all(&roots.runtime).unwrap();
    let instance = custody.identity.instance_id.as_str().to_owned();
    Office::open(folder, Some(key), &instance, roots, custody).unwrap()
}

#[test]
fn an_older_organizations_tess_reaches_its_satellite_once_and_survives_rebuilds() {
    let hq_dir = tempfile::tempdir().unwrap();
    let hq_root = hq_dir.path().canonicalize().unwrap();
    org_office_sqlite::install();
    let (hq_office, hq_roots, hq_custody, hq_key) = Office::for_test_with_real_ca_in(
        &hq_root,
        TestOffice {
            engine: EngineChoice::Sqlite,
        },
    );
    let hq = Arc::new(hq_office);
    let owner = hq.owner_member_id().to_owned();
    let org_id = hq.info().org_id.clone();
    let hq_folder = hq.folder().to_path_buf();
    let expected = chiefs_of_staff(&hq);
    assert_eq!(expected.len(), 1);
    // The organization as an older build created it: no Chief of Staff.
    hq.forget_chief_of_staff_for_test().unwrap();
    assert!(chiefs_of_staff(&hq).is_empty());

    let hq_node = LiveOfficeNode::start(Arc::clone(&hq), true, false);
    let (sat_dir, sat) =
        enroll_satellite(&hq, &owner, &hq_node.hq_fed_addr(), "Tess Sat", SAT_SUFFIX);
    assert!(
        chiefs_of_staff(&sat).is_empty(),
        "the satellite starts without her"
    );
    let sat_id = sat.info().office_id.clone();
    let sat_folder = sat.folder().to_path_buf();
    let sat_node = LiveOfficeNode::start(Arc::clone(&sat), false, true);

    // The HQ-open migration provisions her, once, with her derived ID and default handle.
    let ensured = hq.ensure_chief_of_staff().unwrap().expect("provisioned");
    assert_eq!(
        (ensured.employee_id.to_string(), ensured.handle.clone()),
        expected[0]
    );
    assert_eq!(hq.ensure_chief_of_staff().unwrap(), None);
    sat_node.notify_sat_sync();
    wait_until(
        Duration::from_secs(20),
        "the satellite replicates Tess",
        || {
            sat_node.notify_sat_sync();
            chiefs_of_staff(&sat) == expected
        },
    );
    // On the satellite she is the same member, and no satellite provisions her itself.
    assert_eq!(sat.ensure_chief_of_staff().unwrap(), None);
    assert_eq!(
        sat.chief_of_staff().unwrap().map(|t| t.builtin_key),
        Some(Some(BuiltinKey::ChiefOfStaff))
    );

    // Delete and rebuild both projections: still exactly one Tess on each, the same one.
    drop(sat_node);
    drop(hq_node);
    let hq = Arc::try_unwrap(hq).unwrap_or_else(|_| panic!("the HQ is still shared"));
    let hq = rebuilt(hq, hq_folder, hq_key, hq_roots, hq_custody);
    assert_eq!(chiefs_of_staff(&hq), expected);
    assert_eq!(hq.ensure_chief_of_staff().unwrap(), None);
    hq.close().unwrap();

    let sat = Arc::try_unwrap(sat).unwrap_or_else(|_| panic!("the satellite is still shared"));
    let sat_root = sat_dir.path().canonicalize().unwrap();
    let sat_roots = org_office::DataRoots {
        durable: sat_root.join("data").join(&org_id).join("durable"),
        runtime: sat_root.join("data").join(&org_id).join("runtime"),
    };
    let sat = rebuilt(
        sat,
        sat_folder,
        Key::from_bytes([0x33 + SAT_SUFFIX; 32]),
        sat_roots,
        sat_custody(&org_id, &sat_id, SAT_SUFFIX),
    );
    assert_eq!(chiefs_of_staff(&sat), expected);
    sat.close().unwrap();
}
