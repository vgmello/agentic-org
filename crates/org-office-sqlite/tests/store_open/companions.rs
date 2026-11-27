//! TASK-172: the companion stores' durable folders are the office's, laid out the same way
//! on every engine, for HQ and for a satellite alike.

use std::collections::BTreeSet;
use std::path::Path;

use super::engines;
use org_office::testkit_support::store_open::{
    open_accounting, open_budgets, open_private_notes, open_providers, open_replication,
    EngineChoice, ReplicationOpenSpec,
};

const ORG: &str = "0195a000-0000-7000-8000-000000000001";
const HQ: &str = "0195a000-0000-7000-8000-000000000002";
const SATELLITE: &str = "0195a000-0000-7000-8000-000000000003";
const INCARNATION: &str = "0195a000-0000-7000-8000-000000000004";

/// Opens every companion store of `office` on `engine` and returns the folders left below
/// the durable root, by their paths under it.
fn companion_folders(engine: &EngineChoice, office: &str) -> BTreeSet<String> {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    let runtime = dir.path().join("runtime");
    std::fs::create_dir_all(&durable).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let is_hq = office == HQ;
    open_private_notes(engine, &durable, &runtime, office, ORG, is_hq).unwrap();
    open_accounting(engine, &durable, &runtime, office, ORG).unwrap();
    open_budgets(engine, &durable, &runtime, office, ORG, is_hq).unwrap();
    open_providers(engine, &durable, &runtime, office, ORG, is_hq).unwrap();
    open_replication(ReplicationOpenSpec {
        engine,
        durable_root: &durable,
        runtime_root: &runtime,
        org_id: ORG,
        local_office: office,
        hq_office: HQ,
        ca_digest: org_sync_proto::Digest::from_bytes([0x42; 32]),
        epoch: 1,
        incarnation: INCARNATION,
        initial_snapshot_required: !is_hq,
    })
    .unwrap();
    folders(&durable)
}

fn folders(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                let name = path.strip_prefix(root).unwrap().to_str().unwrap();
                found.insert(name.to_owned());
                pending.push(path);
            }
        }
    }
    found
}

#[test]
fn hq_and_satellite_companion_stores_lay_out_the_same_folders_on_every_engine() {
    for office in [HQ, SATELLITE] {
        let [sqlite, memory] = engines().map(|engine| companion_folders(&engine, office));
        assert_eq!(sqlite, memory, "{office}");

        let own = |folder: &str| format!("local/offices/{office}/{folder}");
        for folder in ["accounting", "budgets"] {
            assert!(sqlite.contains(folder), "{office}: {folder} in {sqlite:?}");
        }
        for folder in ["connections", "replication"] {
            assert!(
                sqlite.contains(&own(folder)),
                "{office}: {folder} in {sqlite:?}"
            );
        }
        let hq_only = ["authority", "authority/agent-execution", "private"];
        if office == HQ {
            assert!(
                hq_only.iter().all(|folder| sqlite.contains(*folder)),
                "{sqlite:?}"
            );
            assert!(!sqlite.contains(&own("local-execution")), "{sqlite:?}");
        } else {
            // TASK-52 ARCH-02: a satellite keeps its permits under its own office-local
            // folder and holds no private notes.
            assert!(sqlite.contains(&own("local-execution")), "{sqlite:?}");
            assert!(
                hq_only.iter().all(|folder| !sqlite.contains(*folder)),
                "{sqlite:?}"
            );
        }
    }
}
