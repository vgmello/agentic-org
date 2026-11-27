//! Co-hosted neighbours (`TASK-204`), on folders laid out as `org-host` lays them out, without an
//! office: where an office's roots sit ([`Placement::locate`]), which neighbours it masks, and
//! that nothing else beside the roots is ever swept. Links and linked prefixes are in
//! `office_masks_link_tests.rs`, which shares this file's host and assertions.

use std::path::{Path, PathBuf};

use super::*;

pub(super) const ORG_A: &str = "0195a000-0000-7000-8000-00000000a001";
pub(super) const ORG_B: &str = "0195a000-0000-7000-8000-00000000b001";
pub(super) const HQ: &str = "0195a000-0000-7000-8000-00000000a0a0";
pub(super) const SAT_1: &str = "0195a000-0000-7000-8000-00000000a0a1";
pub(super) const SAT_2: &str = "0195a000-0000-7000-8000-00000000a0a2";

/// A host instance's data root in a temporary folder.
pub(super) struct Host {
    _dir: tempfile::TempDir,
    pub(super) data: PathBuf,
}

impl Host {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().canonicalize().unwrap().join("data");
        std::fs::create_dir_all(&data).unwrap();
        Self { _dir: dir, data }
    }

    /// Short `/tmp` path so Unix domain sockets stay below macOS `SUN_LEN` (104 bytes).
    fn new_short() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("om")
            .tempdir_in("/tmp")
            .unwrap();
        let data = dir.path().canonicalize().unwrap().join("d");
        std::fs::create_dir_all(&data).unwrap();
        Self { _dir: dir, data }
    }

    /// Makes `rel` (and its parents) below the data root.
    pub(super) fn mk(&self, rel: &str) -> PathBuf {
        let path = self.data.join(rel);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    pub(super) fn file(&self, rel: &str) -> PathBuf {
        let path = self.data.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"secret").unwrap();
        path
    }

    /// The host's own folders beside the organizations: the office home with the office
    /// sockets and agent temporary folders, the instance's `agents/` and its configuration.
    fn host_folders(&self) -> Vec<PathBuf> {
        vec![
            self.mk("office-state/run/o/00000000a0a1"),
            self.mk("office-state/run/t/agent-slug"),
            self.mk("office-state/offices"),
            self.mk("agents"),
            self.file("config.v1.json"),
        ]
    }
}

/// What a sandbox masks of `durable`'s neighbours, on roots where the host puts them.
pub(super) fn placed_masks(durable: &Path, runtime: &Path, org: &str, office: &str) -> OfficeMasks {
    let placement = Placement::locate(durable, runtime, org, office).expect("a host layout");
    let (mut folders, mut files, mut neighbours, mut sweeps) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    placement.add_masks(&mut folders, &mut files, &mut neighbours, &mut sweeps);
    OfficeMasks {
        folders,
        files,
        neighbours,
        sweeps,
        own_roots: own_root_forms([durable, runtime]),
    }
}

/// [`placed_masks`] as deny patterns, and what it pre-creates.
fn neighbour_masks(
    durable: &Path,
    runtime: &Path,
    org: &str,
    office: &str,
) -> (Vec<String>, Vec<PathBuf>) {
    let masks = placed_masks(durable, runtime, org, office);
    (
        denies(&masks),
        masks.pre_created().map(Path::to_path_buf).collect(),
    )
}

pub(super) fn denies(masks: &OfficeMasks) -> Vec<String> {
    let mut out = Vec::new();
    masks.push_denies(&mut out);
    out.iter().map(|p| p.as_str().to_owned()).collect()
}

pub(super) fn assert_masked_whole(denied: &[String], path: &Path) {
    for needle in [path.display().to_string(), format!("{}/**", path.display())] {
        assert!(denied.contains(&needle), "{needle} missing: {denied:?}");
    }
}

/// No deny pattern is `path` or a folder holding it.
pub(super) fn assert_reachable(denied: &[String], path: &Path) {
    for pattern in denied {
        let base = pattern.strip_suffix("/**").unwrap_or(pattern);
        assert!(
            !path.starts_with(base),
            "{} is masked by {pattern}",
            path.display()
        );
    }
}

#[test]
fn a_satellite_masks_every_other_organization_its_hq_and_every_other_satellite_never_its_own() {
    let host = Host::new();
    let host_folders = host.host_folders();
    host.file(&format!("{ORG_A}/durable/private/task-notes.db"));
    host.file(&format!("{ORG_A}/runtime/{HQ}/protected/blob-cache/x"));
    host.mk(&format!("{ORG_A}/restore-backups"));
    host.mk(&format!("{ORG_A}/.restore-0011223344556677"));
    let own = host.mk(&format!("{ORG_A}/offices/{SAT_1}"));
    let (durable, runtime) = (own.join("durable"), own.join("runtime"));
    let home = durable.join(format!("local/offices/{SAT_1}/agents/me/home"));
    let worktree = runtime.join(format!("{SAT_1}/teams/t1/agents/me/wt"));
    for dir in [&home, &worktree] {
        std::fs::create_dir_all(dir).unwrap();
    }
    host.file(&format!(
        "{ORG_A}/offices/{SAT_2}/durable/local/offices/{SAT_2}/auth/token"
    ));
    host.file(&format!(
        "{ORG_A}/offices/{SAT_2}/runtime/{SAT_2}/protected/x"
    ));
    host.file(&format!("{ORG_B}/durable/private/task-notes.db"));
    host.file(&format!("{ORG_B}/runtime/{HQ}/protected/blob-cache/x"));
    // Another organization's folder named in capitals, and one a removal left behind.
    let upper = host.mk("0195A000-0000-7000-8000-00000000C0DE");
    let leftover = host.mk(&format!("{ORG_B}.before-delete"));

    let (denied, pre_created) = neighbour_masks(&durable, &runtime, ORG_A, SAT_1);
    for neighbour in [
        host.data.join(ORG_B),
        upper,
        leftover,
        host.data.join(ORG_A).join("durable"),
        host.data.join(ORG_A).join("runtime"),
        host.data.join(ORG_A).join("restore-backups"),
        host.data.join(ORG_A).join(".restore-0011223344556677"),
        host.data.join(ORG_A).join("offices").join(SAT_2),
    ] {
        assert_masked_whole(&denied, &neighbour);
    }
    for path in [&durable, &runtime, &home, &worktree] {
        assert_reachable(&denied, path);
    }
    for path in &host_folders {
        assert_reachable(&denied, path);
    }
    assert!(
        pre_created.is_empty(),
        "nothing is made outside the office's own roots"
    );
}

#[test]
fn a_satellite_masks_its_hq_roots_before_they_exist_and_never_makes_them() {
    let host = Host::new();
    let own = host.mk(&format!("{ORG_A}/offices/{SAT_1}"));
    let (durable, runtime) = (own.join("durable"), own.join("runtime"));
    let (denied, pre_created) = neighbour_masks(&durable, &runtime, ORG_A, SAT_1);
    for root in ["durable", "runtime"] {
        let hq_root = host.data.join(ORG_A).join(root);
        assert_masked_whole(&denied, &hq_root);
        assert!(!hq_root.exists());
    }
    assert!(pre_created.is_empty());
}

#[test]
fn an_hq_masks_every_other_organization_and_its_satellites_folder_which_bwrap_makes() {
    let host = Host::new();
    let host_folders = host.host_folders();
    let org = host.mk(ORG_A);
    let (durable, runtime) = (org.join("durable"), org.join("runtime"));
    let home = durable.join(format!("local/offices/{HQ}/agents/me/home"));
    std::fs::create_dir_all(&home).unwrap();
    host.file(&format!("{ORG_B}/durable/state/x"));

    let (denied, pre_created) = neighbour_masks(&durable, &runtime, ORG_A, HQ);
    assert_masked_whole(&denied, &host.data.join(ORG_B));
    let satellites = org.join("offices");
    assert!(
        denied.contains(&format!("{}/**", satellites.display())),
        "{denied:?}"
    );
    assert_eq!(pre_created, vec![satellites.clone()]);
    assert!(!satellites.exists(), "denying never makes the folder");
    for path in [&durable, &runtime, &home] {
        assert_reachable(&denied, path);
    }
    for path in &host_folders {
        assert_reachable(&denied, path);
    }
}

#[test]
fn roots_laid_out_any_other_way_have_no_neighbours() {
    let dir = tempfile::tempdir().unwrap();
    let tmp = dir.path().canonicalize().unwrap();
    let sat = tmp.join(ORG_A).join("offices").join(SAT_1);
    let hq = tmp.join(ORG_A);
    let upper_org = ORG_A.to_uppercase();
    for (durable, runtime, org, office) in [
        // A test's bare temporary folder: its parent is a shared folder such as `/tmp`.
        (tmp.join("durable"), tmp.join("runtime"), ORG_A, HQ),
        (hq.join("durable"), hq.join("runtime"), ORG_B, HQ),
        (hq.join("durable"), tmp.join("runtime"), ORG_A, HQ),
        (hq.join("data"), hq.join("runtime"), ORG_A, HQ),
        (hq.join("durable"), hq.join("cache"), ORG_A, HQ),
        (sat.join("durable"), sat.join("runtime"), ORG_A, SAT_2),
        (sat.join("durable"), sat.join("runtime"), ORG_B, SAT_1),
        (
            tmp.join(ORG_A).join("sites").join(SAT_1).join("durable"),
            tmp.join(ORG_A).join("sites").join(SAT_1).join("runtime"),
            ORG_A,
            SAT_1,
        ),
        // Ids that aren't canonical UUIDv7s are never taken for folder names.
        (hq.join("durable"), hq.join("runtime"), "default", HQ),
        (
            tmp.join(&upper_org).join("durable"),
            tmp.join(&upper_org).join("runtime"),
            upper_org.as_str(),
            HQ,
        ),
        (sat.join("durable"), sat.join("runtime"), ORG_A, ".."),
        (
            PathBuf::from("/durable"),
            PathBuf::from("/runtime"),
            ORG_A,
            HQ,
        ),
    ] {
        assert_eq!(
            Placement::locate(&durable, &runtime, org, office),
            None,
            "{} {org} {office}",
            durable.display()
        );
    }
    assert_eq!(
        Placement::locate(
            &PathBuf::from(format!("/{ORG_A}/durable")),
            &PathBuf::from(format!("/{ORG_A}/runtime")),
            ORG_A,
            HQ,
        ),
        Some(Placement {
            data_root: PathBuf::from("/"),
            org_root: PathBuf::from(format!("/{ORG_A}")),
            org_id: ORG_A.to_owned(),
            satellite: None,
        })
    );
    assert_eq!(
        Placement::locate(&sat.join("durable"), &sat.join("runtime"), ORG_A, SAT_1),
        Some(Placement {
            data_root: tmp.clone(),
            org_root: hq,
            org_id: ORG_A.to_owned(),
            satellite: Some(SAT_1.to_owned()),
        })
    );
}

#[test]
fn roots_that_read_as_both_layouts_are_a_satellites() {
    // An office id equal to its organization's: the satellite layout, checked first, wins.
    let sat = PathBuf::from(format!("/data/{ORG_A}/offices/{ORG_A}"));
    assert_eq!(
        Placement::locate(&sat.join("durable"), &sat.join("runtime"), ORG_A, ORG_A),
        Some(Placement {
            data_root: PathBuf::from("/data"),
            org_root: PathBuf::from(format!("/data/{ORG_A}")),
            org_id: ORG_A.to_owned(),
            satellite: Some(ORG_A.to_owned()),
        })
    );
}

#[test]
fn a_shared_data_root_has_only_its_organization_folders_masked() {
    // A data root that is also a shared folder (`/tmp`): its other entries are never masked.
    let host = Host::new();
    let mut unrelated = host.host_folders();
    for name in [
        ".X11-unix",
        "systemd-private-0195a000000070008000-chronyd.service-x",
        "tmp.0195a000-0000-7000-8000-00000000b00",
        "0195a000-0000-7000-8000_00000000b001",
        "0195a000-0000-7000-8000-00000000b00g",
        "ssh-XXXXabcdef",
    ] {
        unrelated.push(host.mk(name));
    }
    unrelated.push(host.file("0195a000.lock"));
    let org = host.mk(ORG_A);
    let (durable, runtime) = (org.join("durable"), org.join("runtime"));
    let (denied, _) = neighbour_masks(&durable, &runtime, ORG_A, HQ);
    for path in &unrelated {
        assert_reachable(&denied, path);
        assert!(
            !denied
                .iter()
                .any(|d| d.contains(&*path.display().to_string())),
            "{} is masked: {denied:?}",
            path.display()
        );
    }
    let office_state_prefix = host
        .data
        .join(OFFICE_STATE_DIR_NAME)
        .join(HOST_REGISTRY_PREFIX)
        .display()
        .to_string();
    assert!(
        !denied
            .iter()
            .any(|d| d.starts_with(&*host.data.display().to_string())
                && !d.starts_with(&*org.display().to_string())
                && !d.starts_with(&office_state_prefix)),
        "only the organization's own satellites folder and office-state/host-registry.* are masked: {denied:?}"
    );
    for name in HOST_REGISTRY_FILES {
        let needle = host
            .data
            .join(OFFICE_STATE_DIR_NAME)
            .join(name)
            .display()
            .to_string();
        assert!(denied.contains(&needle), "{needle} missing: {denied:?}");
    }
}

#[test]
fn links_and_special_files_named_like_an_organization_are_masked_as_themselves() {
    let host = Host::new_short();
    let org = host.mk(ORG_A);
    let (durable, runtime) = (org.join("durable"), org.join("runtime"));
    let own_home = host.mk(&format!(
        "{ORG_A}/durable/local/offices/{HQ}/agents/me/home"
    ));
    // A link to the office's own organization, a dangling one, and a loop.
    let to_own = host.data.join(ORG_B);
    std::os::unix::fs::symlink(&org, &to_own).unwrap();
    let dangling = host.data.join("0195a000-0000-7000-8000-00000000c001");
    std::os::unix::fs::symlink(host.data.join("gone"), &dangling).unwrap();
    let (loop_a, loop_b) = (
        host.data.join("0195a000-0000-7000-8000-00000000c002"),
        host.data.join("0195a000-0000-7000-8000-00000000c003"),
    );
    std::os::unix::fs::symlink(&loop_b, &loop_a).unwrap();
    std::os::unix::fs::symlink(&loop_a, &loop_b).unwrap();
    // A socket where an organization's folder was expected.
    let socket = host.data.join("0195a000-0000-7000-8000-00000000c004");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();

    let (denied, _) = neighbour_masks(&durable, &runtime, ORG_A, HQ);
    // A link targeting the office's own organization is omitted so sandboxes that dereference
    // deny entries never wipe own roots; dangling outside links and loops stay masked as links.
    let to_own_raw = to_own.display().to_string();
    assert!(
        !denied.contains(&to_own_raw) && !denied.contains(&format!("{to_own_raw}/**")),
        "link into own org must be omitted: {denied:?}"
    );
    for link in [&dangling, &loop_a, &loop_b] {
        let raw = link.display().to_string();
        assert!(denied.contains(&raw), "{raw} missing: {denied:?}");
        assert!(
            !denied.contains(&format!("{raw}/**")),
            "a link is never masked through: {denied:?}"
        );
    }
    assert!(denied.contains(&socket.display().to_string()), "{denied:?}");
    assert!(!denied.contains(&format!("{}/**", socket.display())));
    // What the link to the office's own organization resolves to is never masked.
    for path in [&org, &durable, &own_home] {
        assert_reachable(&denied, path);
    }
}

#[test]
fn a_neighbour_or_swept_entry_holding_the_offices_own_roots_is_never_masked() {
    let host = Host::new();
    let org = host.mk(ORG_A);
    let (durable, runtime) = (org.join("durable"), org.join("runtime"));
    host.mk(ORG_B);
    let masks = OfficeMasks {
        folders: Vec::new(),
        files: Vec::new(),
        neighbours: vec![host.data.clone(), org.clone(), durable.clone()],
        // A sweep that keeps nothing reaches the organization's own folder.
        sweeps: vec![Sweep::all_but(&host.data, std::iter::empty())],
        own_roots: own_root_forms([&durable, &runtime]),
    };
    let denied = denies(&masks);
    assert_masked_whole(&denied, &host.data.join(ORG_B));
    for path in [&durable, &runtime] {
        assert_reachable(&denied, path);
    }
}

#[test]
fn the_masks_are_the_same_on_every_launch_and_in_name_order() {
    let host = Host::new();
    let org = host.mk(ORG_A);
    let (durable, runtime) = (org.join("durable"), org.join("runtime"));
    let names = [
        "0195a000-0000-7000-8000-00000000f003",
        "0195a000-0000-7000-8000-00000000f001",
        "0195a000-0000-7000-8000-00000000f002",
    ];
    for name in names {
        host.mk(name);
    }
    let (first, _) = neighbour_masks(&durable, &runtime, ORG_A, HQ);
    let (second, _) = neighbour_masks(&durable, &runtime, ORG_A, HQ);
    assert_eq!(first, second);
    let order: Vec<&str> = first
        .iter()
        .filter(|d| !d.ends_with("/**"))
        .filter_map(|d| Path::new(d).file_name()?.to_str())
        .filter(|name| names_an_organization(name))
        .collect();
    let mut sorted = names.to_vec();
    sorted.sort_unstable();
    assert_eq!(order, sorted);

    // An organization made after a launch is masked from the next one.
    host.mk(ORG_B);
    let (third, _) = neighbour_masks(&durable, &runtime, ORG_A, HQ);
    assert!(!first.contains(&host.data.join(ORG_B).display().to_string()));
    assert_masked_whole(&third, &host.data.join(ORG_B));
}

#[test]
fn only_a_uuid_names_an_organization_and_a_multibyte_name_never_panics() {
    for name in [
        ORG_A,
        "0195A000-0000-7000-8000-00000000A001",
        "00000000-0000-0000-0000-000000000000",
        "0195a000-0000-7000-8000-00000000a001.before-delete",
        "0195a000-0000-7000-8000-00000000a001é",
    ] {
        assert!(names_an_organization(name), "{name}");
    }
    for name in [
        "",
        "office-state",
        "0195a000-0000-7000-8000-00000000a00",
        "0195a0000-000-7000-8000-00000000a001",
        "0195a000-0000-7000-8000-00000000a00g",
        "0195a000-0000-7000-8000-00000000a00é",
        "ééééééééééééééééééééééééééééééééééé",
        "x0195a000-0000-7000-8000-00000000a001",
    ] {
        assert!(!names_an_organization(name), "{name}");
    }
    let others = Keep::AllButOtherOrganizations {
        own: ORG_A.to_owned(),
    };
    assert!(others.masks(Some(ORG_B)));
    assert!(!others.masks(Some(ORG_A)));
    assert!(!others.masks(Some("office-state")));
    assert!(
        !others.masks(None),
        "a name that isn't UTF-8 names no organization"
    );
    let named = Keep::Named(vec!["agents".to_owned()]);
    assert!(!named.masks(Some("agents")));
    assert!(named.masks(Some("auth")));
    assert!(
        named.masks(None),
        "an entry whose name isn't UTF-8 is masked"
    );
}

#[test]
fn host_registry_files_in_office_state_and_custom_office_home_are_masked_while_run_stays_reachable()
{
    let host = Host::new();
    let host_folders = host.host_folders();
    let org = host.mk(ORG_A);
    let (durable, runtime) = (org.join("durable"), org.join("runtime"));
    let extra_backup = host.file("office-state/host-registry.db.before-restore");
    let existing_db = host.file("office-state/host-registry.db");

    let (denied, _) = neighbour_masks(&durable, &runtime, ORG_A, HQ);
    for name in HOST_REGISTRY_FILES {
        let path = host.data.join("office-state").join(name);
        let raw = path.display().to_string();
        assert!(denied.contains(&raw), "{raw} missing: {denied:?}");
        assert!(
            !denied.contains(&format!("{raw}/**")),
            "host-registry file is masked exact, not as folder glob: {denied:?}"
        );
    }
    assert!(
        denied.contains(&extra_backup.display().to_string()),
        "swept host-registry backup missing: {denied:?}"
    );
    assert!(denied.contains(&existing_db.display().to_string()));
    for path in &host_folders {
        assert_reachable(&denied, path);
    }

    // Custom `<office-home>/offices/<office-id>` outside `<data-root>/office-state/` also masks
    // its own `host-registry.*` files without masking `<office-home>/run/`.
    let custom_home = host.mk("custom-office-home");
    let custom_office_folder = custom_home.join("offices").join(HQ);
    std::fs::create_dir_all(&custom_office_folder).unwrap();
    let custom_run = custom_home.join("run/o/short");
    std::fs::create_dir_all(&custom_run).unwrap();
    let custom_bak = custom_home.join("host-registry.db.bak");
    std::fs::write(&custom_bak, b"secret").unwrap();
    assert_eq!(
        office_home(&custom_office_folder, HQ),
        Some(custom_home.clone())
    );
    let mut files = Vec::new();
    let mut sweeps = Vec::new();
    push_host_registry_masks(&custom_home, &mut files, &mut sweeps);
    // Calling twice deduplicates cleanly.
    push_host_registry_masks(&custom_home, &mut files, &mut sweeps);
    assert_eq!(files.len(), HOST_REGISTRY_FILES.len());
    assert_eq!(sweeps.len(), 1);
    let custom_masks = OfficeMasks {
        folders: Vec::new(),
        files,
        neighbours: Vec::new(),
        sweeps,
        own_roots: own_root_forms([&durable, &runtime]),
    };
    let custom_denied = denies(&custom_masks);
    assert!(custom_denied.contains(&custom_home.join("host-registry.db").display().to_string()));
    assert!(custom_denied.contains(&custom_bak.display().to_string()));
    assert_reachable(&custom_denied, &custom_run);
}

#[test]
fn topology_digest_tracks_cohosted_org_and_satellite_changes_without_companion_churn() {
    let host = Host::new();
    let org = host.mk(ORG_A);
    let (hq_durable, hq_runtime) = (org.join("durable"), org.join("runtime"));
    std::fs::create_dir_all(&hq_durable).unwrap();
    std::fs::create_dir_all(&hq_runtime).unwrap();

    // 1. HQ digest detects a newly created sibling organization (`ORG_B`), while ignoring
    //    satellites below statically masked `<ORG_A>/offices/**` and known companion folders.
    let mut hq_masks = placed_masks(&hq_durable, &hq_runtime, ORG_A, HQ);
    let hq_local = hq_durable.join("local/offices").join(HQ);
    hq_masks.folders.push(MaskedFolder {
        path: hq_local.join("sessions"),
        pre_create: true,
    });
    hq_masks
        .sweeps
        .push(Sweep::all_but(&hq_local, [AGENTS_DIR_NAME]));
    let d0 = hq_masks.topology_digest(false).unwrap();
    std::fs::create_dir_all(hq_local.join("sessions")).unwrap();
    assert_eq!(
        hq_masks.topology_digest(false).unwrap(),
        d0,
        "creating a statically declared companion folder must not churn digest"
    );
    host.mk(&format!("{ORG_A}/offices/{SAT_1}"));
    assert_eq!(
        hq_masks.topology_digest(false).unwrap(),
        d0,
        "HQ statically masks `<org>/offices/**`, so new satellites need not churn host-native HQ"
    );
    host.mk(ORG_B);
    let d1 = hq_masks.topology_digest(false).unwrap();
    assert_ne!(
        d1, d0,
        "new co-hosted sibling org must rotate topology digest"
    );

    // 2. Satellite digest detects both a new sibling satellite (`SAT_2`) and (when tracking
    //    absent static masks for Linux bwrap) newly materialized HQ roots / host-registry.db.
    let sat_host = Host::new();
    let sat_own = sat_host.mk(&format!("{ORG_A}/offices/{SAT_1}"));
    let (sat_durable, sat_runtime) = (sat_own.join("durable"), sat_own.join("runtime"));
    std::fs::create_dir_all(&sat_durable).unwrap();
    std::fs::create_dir_all(&sat_runtime).unwrap();
    let sat_masks = placed_masks(&sat_durable, &sat_runtime, ORG_A, SAT_1);
    let sat_native_0 = sat_masks.topology_digest(false).unwrap();
    let sat_bwrap_0 = sat_masks.topology_digest(true).unwrap();

    // Creating HQ's `durable` root flips Linux bwrap static-presence digest, not host-native.
    sat_host.mk(&format!("{ORG_A}/durable"));
    assert_eq!(sat_masks.topology_digest(false).unwrap(), sat_native_0);
    let sat_bwrap_1 = sat_masks.topology_digest(true).unwrap();
    assert_ne!(sat_bwrap_1, sat_bwrap_0);

    // Creating a sibling satellite `SAT_2` flips BOTH host-native and Linux bwrap digests.
    sat_host.mk(&format!("{ORG_A}/offices/{SAT_2}"));
    assert_ne!(sat_masks.topology_digest(false).unwrap(), sat_native_0);
    assert_ne!(sat_masks.topology_digest(true).unwrap(), sat_bwrap_1);
}
