use super::*;
use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct Fixture {
    temporary: tempfile::TempDir,
    registry: Registry,
}

impl Fixture {
    fn new() -> Self {
        // Canonicalize only this test-owned OS temporary base, before composing owned paths.
        #[cfg(target_os = "macos")]
        let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let temporary =
            {
                let base = std::env::var_os("ORG_INSTANCE_TEST_DISK_BASE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/var/tmp"));
                tempfile::tempdir_in(&base).unwrap_or_else(|error|
                panic!("disk fixture base {} unavailable: {error}; set ORG_INSTANCE_TEST_DISK_BASE",
                       base.display()))
            };
        let base = temporary.path().canonicalize().unwrap();
        let user_temp = base.join("temp");
        fs::create_dir(&user_temp).unwrap();
        fs::set_permissions(&user_temp, fs::Permissions::from_mode(0o700)).unwrap();
        let registry = Registry::open(&base.join("locator"), &user_temp).unwrap_or_else(|error|
            panic!("instance fixture needs a short disk-backed base at {}: {error}; set ORG_INSTANCE_TEST_DISK_BASE",
                   base.display()));
        Self {
            temporary,
            registry,
        }
    }

    fn base(&self) -> PathBuf {
        self.temporary.path().canonicalize().unwrap()
    }
    fn root(&self, name: &str) -> PathBuf {
        self.base().join(name)
    }
    fn locator(&self) -> PathBuf {
        self.base().join("locator").join(REGISTRY)
    }

    fn reserve(&self, name: &str) -> Instance {
        self.registry.reserve(Some(name), &self.root(name)).unwrap()
    }
}

#[test]
fn names_have_one_portable_spelling_and_default_is_explicit() {
    assert_eq!(Name::parse(None).unwrap().as_str(), "default");
    for name in ["default", "lab", "a-1", "0", &"a".repeat(32)] {
        assert_eq!(Name::parse(Some(name)).unwrap().as_str(), name);
    }
    for name in [
        "",
        ".",
        "..",
        "../lab",
        "a/b",
        "Lab",
        " lab",
        "é",
        "-lab",
        &"a".repeat(33),
    ] {
        assert!(Name::parse(Some(name)).is_err(), "{name}");
    }
}

#[test]
fn reservation_is_private_durable_and_reopens_without_changing_identity() {
    let fixture = Fixture::new();
    let instance = fixture
        .registry
        .reserve(None, &fixture.root("default"))
        .unwrap();
    assert_eq!(instance.name(), "default");
    assert!(org_proto::ulid_time_ms(instance.id()).is_some());
    assert_eq!(instance.root(), fixture.root("default"));
    let id = instance.id().to_owned();
    let config = fs::read(instance.root().join(CONFIG)).unwrap();
    let locator = fs::read(fixture.locator()).unwrap();
    for file in [fixture.locator(), instance.root().join(CONFIG)] {
        assert_eq!(fs::metadata(file).unwrap().mode() & 0o7777, 0o600);
    }
    assert_eq!(
        fs::metadata(instance.root()).unwrap().mode() & 0o7777,
        0o700
    );
    drop(instance);
    let reopened = fixture
        .registry
        .reserve(None, &fixture.root("default"))
        .unwrap();
    assert_eq!(reopened.id(), id);
    assert_eq!(fs::read(reopened.root().join(CONFIG)).unwrap(), config);
    assert_eq!(fs::read(fixture.locator()).unwrap(), locator);
}

#[test]
fn separate_instances_can_hold_independent_roots_concurrently() {
    let fixture = Fixture::new();
    let first = fixture.reserve("default");
    let second = fixture.reserve("lab");
    assert_ne!(first.id(), second.id());
    assert_ne!(first.root(), second.root());
    first.verify().unwrap();
    second.verify().unwrap();
}

#[test]
fn same_root_cannot_be_reserved_under_another_name() {
    let fixture = Fixture::new();
    let first = fixture.reserve("default");
    let original = fs::read(first.root().join(CONFIG)).unwrap();
    let locator = fs::read(fixture.locator()).unwrap();
    assert!(matches!(
        fixture.registry.reserve(Some("lab"), first.root()),
        Err(Error::ReservedRoot { .. })
    ));
    assert_eq!(fs::read(first.root().join(CONFIG)).unwrap(), original);
    assert_eq!(fs::read(fixture.locator()).unwrap(), locator);
}

#[test]
fn an_unavailable_other_instance_does_not_block_default_or_release_its_reservations() {
    for replaced in [false, true] {
        let fixture = Fixture::new();
        let default = fixture.reserve("default");
        let id = default.id().to_owned();
        drop(default);
        let lab = fixture.reserve("lab");
        drop(lab);
        let lab_path = fixture.root("lab");
        let moved = fixture.root("moved-lab");
        fs::rename(&lab_path, &moved).unwrap();
        if replaced {
            fs::create_dir(&lab_path).unwrap();
            fs::set_permissions(&lab_path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let reopened = fixture
            .registry
            .reserve(None, &fixture.root("default"))
            .unwrap_or_else(|error| panic!("unrelated lab must not block default: {error}"));
        assert_eq!(reopened.id(), id);
        assert!(fixture
            .registry
            .reserve(Some("takeover"), &lab_path)
            .is_err());
        assert!(fixture.registry.reserve(Some("takeover"), &moved).is_err());
        let error = match fixture.registry.reserve(Some("lab"), &lab_path) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("changed lab root must not be adopted"),
        };
        assert!(
            error.contains("lab") && error.contains(lab_path.to_str().unwrap()),
            "{error}"
        );
        assert!(!error.contains("repair its reservation; repair"), "{error}");
        assert!(error.contains("agentic-org instance rebind lab"), "{error}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn an_unavailable_root_reserves_unicode_normalization_aliases() {
    let fixture = Fixture::new();
    let composed = fixture.root("caf\u{e9}");
    let decomposed = fixture.root("cafe\u{301}");
    let held = fixture.registry.reserve(Some("lab"), &composed).unwrap();
    drop(held);
    fs::rename(&composed, fixture.root("moved-lab")).unwrap();
    assert!(
        fixture
            .registry
            .reserve(Some("other"), &decomposed)
            .is_err(),
        "an unavailable NFC path must not be taken over through its NFD spelling"
    );
    assert!(!composed.exists() && !decomposed.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn unavailable_path_comparison_preserves_component_and_accent_boundaries() {
    assert!(paths_overlap(
        Path::new("/data/CAF\u{c9}"),
        Path::new("/data/cafe\u{301}/child"),
        true
    ));
    assert!(!paths_overlap(
        Path::new("/data/caf\u{e9}"),
        Path::new("/data/cafe"),
        true
    ));
    assert!(!paths_overlap(
        Path::new("/data/caf\u{e9}"),
        Path::new("/data/caf\u{e9}2"),
        true
    ));
}

#[test]
fn every_control_symlink_is_refused_without_changing_its_valid_target() {
    for control in ["instance.json", "instances.json", "registry.lock"] {
        let fixture = Fixture::new();
        let instance = fixture.reserve("lab");
        drop(instance);
        fixture.reserve("lab").verify().unwrap();
        let path = if control == CONFIG {
            fixture.root("lab").join(control)
        } else {
            fixture.root("locator").join(control)
        };
        let target = fixture.root("target");
        let before = fs::read(&path).unwrap();
        fs::rename(&path, &target).unwrap();
        let inode = fs::metadata(&target).unwrap().ino();
        symlink(&target, &path).unwrap();
        assert!(
            fixture
                .registry
                .reserve(Some("lab"), &fixture.root("lab"))
                .is_err(),
            "{control}"
        );
        assert_eq!(fs::read(&target).unwrap(), before, "{control}");
        assert_eq!(fs::metadata(&target).unwrap().ino(), inode);
    }
}

#[test]
fn an_empty_locator_still_refuses_a_changed_user_temp_base_with_its_own_diagnostic() {
    let fixture = Fixture::new();
    let locator = Locator {
        version: 1,
        user_temp: fixture.root("different"),
        entries: Vec::new(),
    };
    let lock = fixture.registry.directory.lock_file(LOCK).unwrap();
    fixture.registry.save(&locator, &lock).unwrap();
    drop(lock);
    let message = match fixture.registry.restore_temp_folders() {
        Err(error) => error.to_string(),
        Ok(_) => panic!("changed user temp base accepted"),
    };
    assert!(message.contains("user temp base"), "{message}");
    assert!(!message.contains("version or entry count"), "{message}");
}

#[test]
fn same_name_cannot_move_to_a_new_root() {
    let fixture = Fixture::new();
    let first = fixture.reserve("lab");
    let other = fixture.root("elsewhere");
    match fixture.registry.reserve(Some("lab"), &other) {
        Err(error) => assert!(matches!(error, Error::Conflict(_)), "{error:?}"),
        Ok(_) => panic!("name unexpectedly rebound"),
    }
    assert!(!other.exists());
    first.verify().unwrap();
}

#[test]
fn nested_roots_are_refused_before_creating_or_mutating_them() {
    let fixture = Fixture::new();
    let first = fixture.reserve("parent");
    let child = first.root().join("child");
    assert!(fixture.registry.reserve(Some("child"), &child).is_err());
    assert!(!child.exists());
    let parent = fixture.base();
    let before: Vec<_> = fs::read_dir(&parent)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(fixture.registry.reserve(Some("outer"), &parent).is_err());
    let after: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(before, after);
}

#[test]
fn unknown_nonempty_or_copied_roots_are_never_adopted() {
    let fixture = Fixture::new();
    let root = fixture.root("unknown");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.join("sentinel"), b"existing data").unwrap();
    assert!(matches!(
        fixture.registry.reserve(Some("unknown"), &root),
        Err(Error::Conflict(_))
    ));
    assert_eq!(fs::read(root.join("sentinel")).unwrap(), b"existing data");
    assert!(!root.join(CONFIG).exists());
    let original = fixture.reserve("original");
    let copy = fixture.root("copy");
    fs::create_dir(&copy).unwrap();
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o700)).unwrap();
    fs::copy(original.root().join(CONFIG), copy.join(CONFIG)).unwrap();
    assert!(matches!(
        fixture.registry.reserve(Some("copy"), &copy),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn a_ready_instance_never_regenerates_missing_or_corrupt_configuration() {
    let fixture = Fixture::new();
    let instance = fixture.reserve("lab");
    let config = instance.root().join(CONFIG);
    let locator = fs::read(fixture.locator()).unwrap();
    drop(instance);
    fs::remove_file(&config).unwrap();
    assert!(matches!(
        fixture.registry.reserve(Some("lab"), &fixture.root("lab")),
        Err(Error::Unsafe(_))
    ));
    assert!(!config.exists());
    for bytes in [b"not JSON".to_vec(), vec![b'x'; MAX_BYTES + 1]] {
        fs::write(&config, &bytes).unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(fixture
            .registry
            .reserve(Some("lab"), &fixture.root("lab"))
            .is_err());
        assert_eq!(fs::read(&config).unwrap(), bytes);
    }
    assert_eq!(fs::read(fixture.locator()).unwrap(), locator);
}

#[test]
fn an_interrupted_reservation_reuses_its_committed_id() {
    let fixture = Fixture::new();
    let instance = fixture.reserve("lab");
    let id = instance.id().to_owned();
    let config_path = instance.root().join(CONFIG);
    drop(instance);
    // Crash fixtures at the two publication boundaries: reserved before config, and
    // config durably present before the ready marker. Neither may mint another ID.
    for remove_config in [true, false] {
        let lock = fixture.registry.directory.lock_file(LOCK).unwrap();
        let mut locator = fixture.registry.load().unwrap();
        locator.entries[0].phase = Phase::Reserved;
        fixture.registry.save(&locator, &lock).unwrap();
        drop(lock);
        if remove_config {
            fs::remove_file(&config_path).unwrap();
        }
        let resumed = fixture.reserve("lab");
        assert_eq!(resumed.id(), id);
        resumed.verify().unwrap();
        drop(resumed);
    }
}

#[test]
fn replaced_or_relocated_root_is_refused_without_writing_to_replacement() {
    let fixture = Fixture::new();
    let instance = fixture.reserve("lab");
    let path = fixture.root("lab");
    let moved = fixture.root("moved");
    fs::rename(&path, &moved).unwrap();
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(instance.verify().is_err());
    assert!(fixture.registry.reserve(Some("lab"), &path).is_err());
    assert!(!path.join(CONFIG).exists());
    let mut actual = Directory::open(&moved, false).unwrap();
    assert!(matches!(actual.lock_root(), Err(Error::Busy(_))));
}

#[test]
fn symlinks_in_final_and_intermediate_components_are_refused() {
    let fixture = Fixture::new();
    let instance = fixture.reserve("lab");
    let alias = fixture.root("alias");
    symlink(instance.root(), &alias).unwrap();
    assert!(fixture.registry.reserve(Some("alias"), &alias).is_err());
    assert!(fixture
        .registry
        .reserve(Some("nested"), &alias.join("nested"))
        .is_err());
    assert!(!instance.root().join("nested").exists());
    assert!(Registry::open(&alias.join("locator"), &fixture.root("temp")).is_err());
    assert!(!instance.root().join("locator").exists());
}

#[test]
fn hardlinked_world_readable_and_special_control_files_are_refused() {
    let fixture = Fixture::new();
    let instance = fixture.reserve("lab");
    let config = instance.root().join(CONFIG);
    let original = fs::read(&config).unwrap();
    drop(instance);
    let alias = fixture.root("linked-config");
    fs::hard_link(&config, &alias).unwrap();
    assert!(fixture
        .registry
        .reserve(Some("lab"), &fixture.root("lab"))
        .is_err());
    assert_eq!(fs::read(&alias).unwrap(), original);
    fs::remove_file(&alias).unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(fixture
        .registry
        .reserve(Some("lab"), &fixture.root("lab"))
        .is_err());
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(&config).unwrap();
    let fifo = std::ffi::CString::new(config.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: creates only a test-owned temporary FIFO with a valid path.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(fixture
        .registry
        .reserve(Some("lab"), &fixture.root("lab"))
        .is_err());
}

#[test]
fn unknown_versions_fields_duplicate_bindings_and_oversize_locators_fail_closed() {
    let fixture = Fixture::new();
    let instance = fixture.reserve("lab");
    let config = fs::read(instance.root().join(CONFIG)).unwrap();
    drop(instance);
    let original = fs::read(fixture.locator()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let mut version = value.clone();
    version["version"] = 2.into();
    let mut unknown = value.clone();
    unknown["unexpected"] = true.into();
    let mut duplicate = value.clone();
    duplicate["entries"]
        .as_array_mut()
        .unwrap()
        .push(value["entries"][0].clone());
    let mut invalid_id = value.clone();
    invalid_id["entries"][0]["binding"]["id"] = "not-an-id".into();
    for bytes in [
        serde_json::to_vec(&version).unwrap(),
        serde_json::to_vec(&unknown).unwrap(),
        serde_json::to_vec(&duplicate).unwrap(),
        serde_json::to_vec(&invalid_id).unwrap(),
        vec![b'x'; MAX_BYTES + 1],
    ] {
        fs::write(fixture.locator(), &bytes).unwrap();
        assert!(fixture
            .registry
            .reserve(Some("lab"), &fixture.root("lab"))
            .is_err());
        assert_eq!(fs::read(fixture.locator()).unwrap(), bytes);
        assert_eq!(fs::read(fixture.root("lab").join(CONFIG)).unwrap(), config);
    }
}

#[test]
fn same_office_id_in_two_instances_gets_distinct_persistent_temp_folders() {
    let fixture = Fixture::new();
    let mut first = fixture.reserve("default");
    let mut second = fixture.reserve("lab");
    let a = first.allocate_office_temp("01OFFICE").unwrap();
    let b = second.allocate_office_temp("01OFFICE").unwrap();
    assert_ne!(a, b);
    assert_eq!(a.parent(), Some(first.temp_folder()));
    assert_eq!(b.parent(), Some(second.temp_folder()));
    assert!(temp::valid(a.file_name().unwrap().to_str().unwrap()));
    assert!(temp::valid(b.file_name().unwrap().to_str().unwrap()));
    assert_eq!(fs::metadata(&a).unwrap().mode() & 0o7777, 0o700);
    assert_eq!(first.allocate_office_temp("01OFFICE").unwrap(), a);
    fs::remove_dir(&a).unwrap();
    drop(first);
    let mut reopened = fixture.reserve("default");
    assert_eq!(reopened.allocate_office_temp("01OFFICE").unwrap(), a);
    assert!(
        a.is_dir(),
        "recorded handle is restored instead of replaced"
    );
    let other = reopened.allocate_office_temp("02OFFICE").unwrap();
    assert_ne!(other, a);
    assert!(b.is_dir());
}

#[test]
fn startup_restores_all_registered_instance_temp_folders_without_reallocating() {
    let fixture = Fixture::new();
    let first = fixture.reserve("default");
    let second = fixture.reserve("lab");
    let a = first.temp_folder().to_owned();
    let b = second.temp_folder().to_owned();
    assert_ne!(a, b);
    let original = fs::read(fixture.locator()).unwrap();
    fs::remove_dir(&a).unwrap();
    fs::remove_dir(&b).unwrap();
    fs::remove_dir(a.parent().unwrap()).unwrap();
    let mut restored = fixture.registry.restore_temp_folders().unwrap();
    let mut expected = vec![a.clone(), b.clone()];
    restored.sort();
    expected.sort();
    assert_eq!(restored, expected);
    assert!(a.is_dir() && b.is_dir());
    assert_eq!(fs::read(fixture.locator()).unwrap(), original);
}

#[test]
fn user_temp_base_cannot_change_or_overlap_control_roots() {
    let fixture = Fixture::new();
    let instance = fixture.reserve("lab");
    let other = fixture.root("other-temp");
    fs::create_dir(&other).unwrap();
    fs::set_permissions(&other, fs::Permissions::from_mode(0o700)).unwrap();
    let changed = Registry::open(&fixture.root("locator"), &other).unwrap();
    assert!(changed.restore_temp_folders().is_err());
    assert!(fixture
        .registry
        .reserve(Some("nested"), &fixture.root("temp").join("nested"))
        .is_err());
    assert!(!fixture.root("temp").join("nested").exists());
    assert!(Registry::open(&fixture.root("temp").join("locator"), &fixture.root("temp")).is_err());
    instance.verify().unwrap();
}

#[test]
fn invalid_office_handles_and_symlink_temp_targets_fail_closed() {
    let fixture = Fixture::new();
    let mut instance = fixture.reserve("lab");
    let office = instance.allocate_office_temp("01OFFICE").unwrap();
    let config = instance.root().join(CONFIG);
    let original = fs::read(&config).unwrap();
    let mut bad: serde_json::Value = serde_json::from_slice(&original).unwrap();
    bad["office_temp_handles"]["02OFFICE"] = bad["office_temp_handles"]["01OFFICE"].clone();
    fs::write(&config, serde_json::to_vec(&bad).unwrap()).unwrap();
    assert!(instance.allocate_office_temp("03OFFICE").is_err());
    fs::write(&config, &original).unwrap();
    let text = String::from_utf8(original.clone()).unwrap();
    let handle = office.file_name().unwrap().to_str().unwrap();
    let one = format!("\"01OFFICE\":\"{handle}\"");
    let duplicated = text.replace(&one, &format!("{one},{one}"));
    assert_ne!(duplicated, text);
    fs::write(&config, duplicated).unwrap();
    assert!(
        instance.verify().is_err(),
        "duplicate JSON map keys must not overwrite a record"
    );
    fs::write(&config, original).unwrap();
    fs::remove_dir(&office).unwrap();
    let elsewhere = fixture.root("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, &office).unwrap();
    assert!(instance.allocate_office_temp("01OFFICE").is_err());
    assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());
}

#[test]
fn temp_base_validation_precedes_locator_mutation() {
    let fixture = Fixture::new();
    let base = fixture.root("a-very-long-user-temp-base-that-exceeds-the-session-path-budget");
    fs::create_dir(&base).unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    let locator = fixture.root("other-locator");
    assert!(Registry::open(&locator, &base).is_err());
    assert!(!locator.exists());
}

#[test]
fn every_owned_temp_level_is_revalidated_before_allocating() {
    let fixture = Fixture::new();
    let mut instance = fixture.reserve("lab");
    let ancestor = fixture.root("temp").join("ao");
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(instance.allocate_office_temp("01OFFICE").is_err());
    assert!(instance.verify().is_err());
    assert!(fixture.registry.restore_temp_folders().is_err());
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o700)).unwrap();
    instance.allocate_office_temp("01OFFICE").unwrap();
}

// Child-only environment is set on Command; tests never mutate the parent's environment.
const CHILD: &str = "ORG_INSTANCE_TEST_CHILD";
const CHILD_TEST: &str = "instance::registry::tests::reservation_child";

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn child(fixture: &Fixture, mode: &str, output: &Path) -> Process {
    // Match the host's macOS two-step pipe/socket close-on-exec discipline.
    let _spawn = crate::cloexec::SPAWN_LOCK.lock().unwrap();
    Process(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD_TEST, "--nocapture"])
            .env(CHILD, mode)
            .env("ORG_INSTANCE_TEST_BASE", fixture.base())
            .env("ORG_INSTANCE_TEST_OUTPUT", output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    )
}

fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "child fixture timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn reservation_child() {
    let Some(mode) = std::env::var_os(CHILD) else {
        return;
    };
    let base = PathBuf::from(std::env::var_os("ORG_INSTANCE_TEST_BASE").unwrap());
    let output = PathBuf::from(std::env::var_os("ORG_INSTANCE_TEST_OUTPUT").unwrap());
    let registry = Registry::open(&base.join("locator"), &base.join("temp")).unwrap();
    if mode == "busy" {
        assert!(matches!(
            registry.reserve(Some("lab"), &base.join("lab")),
            Err(Error::Busy(_))
        ));
        return;
    }
    let mut opened = None;
    until(|| match registry.reserve(Some("lab"), &base.join("lab")) {
        Ok(instance) => {
            opened = Some(instance);
            true
        }
        Err(Error::Busy(_)) => false,
        Err(error) => panic!("{error}"),
    });
    let instance = opened.unwrap();
    fs::write(output, instance.id()).unwrap();
    if mode == "hold" {
        loop {
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
fn root_lock_excludes_an_independent_process_and_releases_on_drop() {
    let fixture = Fixture::new();
    let held = fixture.reserve("lab");
    let mut contender = child(&fixture, "busy", &fixture.root("unused"));
    until(|| contender.0.try_wait().unwrap().is_some());
    assert!(contender.0.wait().unwrap().success());
    let id = held.id().to_owned();
    drop(held);
    assert_eq!(fixture.reserve("lab").id(), id);
}

#[test]
fn concurrent_process_reservations_reuse_one_identity() {
    let fixture = Fixture::new();
    let first_output = fixture.root("first-result");
    let second_output = fixture.root("second-result");
    let mut first = child(&fixture, "reserve", &first_output);
    let mut second = child(&fixture, "reserve", &second_output);
    until(|| first.0.try_wait().unwrap().is_some() && second.0.try_wait().unwrap().is_some());
    assert!(first.0.wait().unwrap().success());
    assert!(second.0.wait().unwrap().success());
    assert_eq!(
        fs::read_to_string(first_output).unwrap(),
        fs::read_to_string(second_output).unwrap()
    );
    fixture.reserve("lab").verify().unwrap();
}

#[test]
fn a_crashed_owner_releases_its_lock_without_losing_identity() {
    let fixture = Fixture::new();
    let output = fixture.root("result");
    let mut owner = child(&fixture, "hold", &output);
    until(|| fs::metadata(&output).is_ok_and(|metadata| metadata.len() == 26));
    let id = fs::read_to_string(&output).unwrap();
    assert!(matches!(
        fixture.registry.reserve(Some("lab"), &fixture.root("lab")),
        Err(Error::Busy(_))
    ));
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    assert_eq!(fixture.reserve("lab").id(), id);
}
