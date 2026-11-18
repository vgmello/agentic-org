use super::*;

#[test]
fn ports_default_to_7420_and_named_instances_take_the_first_free_from_7421() {
    assert_eq!(choose_port(None, None, |_| true, &[]).unwrap(), 7420);
    assert_eq!(
        choose_port(Some("default"), None, |_| true, &[]).unwrap(),
        7420
    );
    assert_eq!(
        choose_port(Some("lab"), None, |p| p != 7421, &[]).unwrap(),
        7422
    );
    assert_eq!(
        choose_port(Some("lab"), Some(9000), |_| true, &[]).unwrap(),
        9000
    );
}

#[test]
fn ports_other_instances_saved_are_never_chosen() {
    // A stopped instance's saved port is free to bind, but it is still that instance's.
    let saved = [("a".to_owned(), 7421), ("b".to_owned(), 7423)];
    assert_eq!(
        choose_port(Some("lab"), None, |p| p != 7422, &saved).unwrap(),
        7424
    );
    let why = choose_port(Some("lab"), Some(7423), |_| true, &saved).unwrap_err();
    assert!(
        why.contains("saved address of instance \"b\"") && why.contains("--port"),
        "{why}"
    );
    let taken = [("lab".to_owned(), DEFAULT_PORT)];
    let why = choose_port(None, None, |_| true, &taken).unwrap_err();
    assert!(
        why.contains("instance \"lab\"") && why.contains("nothing was saved"),
        "{why}"
    );
}

#[test]
fn a_held_saved_port_names_the_port_and_the_way_out() {
    let why = port_held_elsewhere(7425, Some("lab"));
    assert!(
        why.contains("port 7425") && why.contains("another program"),
        "{why}"
    );
    assert!(why.ends_with("run agentic-org setup --name lab"), "{why}");
    assert!(port_held_elsewhere(7420, None).ends_with("run agentic-org setup"));
}

#[test]
fn an_occupied_port_is_an_actionable_error() {
    for (name, requested) in [(None, None), (None, Some(9000)), (Some("lab"), Some(9000))] {
        let why = choose_port(name, requested, |_| false, &[]).unwrap_err();
        assert!(why.contains("in use") && why.contains("--port"), "{why}");
        assert!(why.contains("nothing was saved"), "{why}");
    }
    assert!(choose_port(None, Some(0), |_| true, &[]).is_err());
}

#[test]
fn the_protection_report_states_encryption_modes_and_isolation() {
    let mac = Wording {
        name: "FileVault",
        setting: "FileVault in System Settings › Privacy & Security",
    };
    let on = protection_report(&EncryptionState::On, mac, "alex", &[]);
    assert_eq!(on[0], "Disk encryption (FileVault): on.");
    assert!(on[1].contains("readable only by alex (folders 700, files 600)"));
    assert!(on[2].contains("same-user mode"));
    let off = protection_report(&EncryptionState::Off, mac, "alex", &[]);
    assert_eq!(
        off[0],
        "Disk encryption is off. Agentic Org does not encrypt its data; turn on FileVault in \
         System Settings › Privacy & Security."
    );
    let unknown = protection_report(
        &EncryptionState::Unknown("no volume".into()),
        mac,
        "alex",
        &[],
    );
    assert_eq!(
        unknown[0],
        "Disk encryption could not be determined (no volume). Agentic Org does not encrypt \
         its data; check FileVault in System Settings › Privacy & Security."
    );
    let open = protection_report(&EncryptionState::On, mac, "alex", &[("/r/x".into(), 0o644)]);
    assert!(
        open[1].contains("1 path(s)") && open[2] == "  644 /r/x",
        "{open:?}"
    );
    // This system's own words fit the same sentences.
    let here = org_platform::disk::wording();
    let on = protection_report(&EncryptionState::On, here, "alex", &[]);
    assert_eq!(on[0], format!("Disk encryption ({}): on.", here.name));
    let off = protection_report(&EncryptionState::Off, here, "alex", &[]);
    assert!(
        off[0].ends_with(&format!("turn on {}.", here.setting)),
        "{off:?}"
    );
}

#[test]
fn a_service_that_stops_at_logout_is_reported_with_the_command_that_changes_it() {
    assert!(lingering_report(&Lingering::On, "alex").is_empty());
    assert!(lingering_report(&Lingering::NotApplicable, "alex").is_empty());
    let off = lingering_report(
        &Lingering::Off {
            enable: "loginctl enable-linger alex".into(),
        },
        "alex",
    );
    assert_eq!(
        off,
        [
            "The service runs only while alex is logged in: it stops at logout and does not \
             start when the machine starts.",
            "To keep it running without a login session, run: loginctl enable-linger alex",
        ]
    );
    let unknown = lingering_report(&Lingering::Unknown("loginctl could not run".into()), "alex");
    assert_eq!(unknown.len(), 1);
    assert!(
        unknown[0].contains("(loginctl could not run)"),
        "{unknown:?}"
    );
}

#[test]
fn a_missing_service_manager_is_reported_with_the_foreground_command() {
    let text = no_service_manager(ServiceError::Unavailable(
        "XDG_RUNTIME_DIR is not set, so this session has no systemd user manager".into(),
    ));
    assert!(text.contains("XDG_RUNTIME_DIR is not set"), "{text}");
    assert!(
        text.contains("\"service\": \"foreground\"") && text.contains("agentic-org serve --setup"),
        "{text}"
    );
    assert!(text.ends_with("nothing was changed"), "{text}");
    let other = no_service_manager(ServiceError::System("launchctl exited with 5".into()));
    assert!(!other.contains("foreground"), "{other}");
}

#[test]
fn modes_are_read_back_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("r");
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.join("a"), b"x").unwrap();
    std::fs::set_permissions(root.join("a"), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(mode_exceptions(&root).unwrap().is_empty());
    std::fs::set_permissions(root.join("a"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        mode_exceptions(&root).unwrap(),
        vec![(root.join("a"), 0o644)]
    );
}

#[test]
fn the_development_pin_is_valid_and_stable() {
    let pin = release_pin().unwrap();
    pin.validate().unwrap();
    assert_eq!(pin, release_pin().unwrap());
}

#[test]
fn a_repeated_setup_with_an_administrator_says_where_to_sign_in() {
    assert_eq!(
        running_with_administrator("default", "http://127.0.0.1:7420"),
        "Instance default is running. Open http://127.0.0.1:7420 and sign in."
    );
}

#[test]
fn admin_reset_off_a_terminal_refuses_with_the_spec_copy_before_anything_else() {
    let mut out = Vec::new();
    let refused = admin_reset(Some("no-such-instance"), None, false, &mut out).unwrap_err();
    assert_eq!(refused, NOT_A_TERMINAL);
    assert!(out.is_empty());
}

/// SIGN-33: whose instance `admin reset` acts on. Root acts for the account it names (or
/// the one `sudo` names) and never for itself; anyone else acts only for itself.
#[test]
fn admin_reset_acts_for_the_named_service_user_only_when_root_runs_it() {
    let agentic = Account {
        uid: 61000,
        gid: 61000,
    };
    let by_name = |name: &str| match name {
        "agentic" => Some(agentic),
        "root" => Some(Account { uid: 0, gid: 0 }),
        "other" => Some(Account {
            uid: 1001,
            gid: 1001,
        }),
        _ => None,
    };
    let by_id = |uid: u32| match uid {
        61000 => Some(agentic),
        0 => Some(Account { uid: 0, gid: 0 }),
        _ => None,
    };
    let decide = |me, named, sudo| service_account(me, named, sudo, by_name, by_id);
    // Root: the named account, or the one sudo names; --user wins.
    assert_eq!(decide(0, Some("agentic"), None), Ok(Some(agentic)));
    assert_eq!(decide(0, None, Some("61000")), Ok(Some(agentic)));
    assert_eq!(decide(0, Some("agentic"), Some("1001")), Ok(Some(agentic)));
    assert!(decide(0, None, None)
        .unwrap_err()
        .contains("--user <service user>"));
    assert!(decide(0, Some("nobody-here"), None)
        .unwrap_err()
        .contains("no account"));
    assert!(decide(0, None, Some("4242"))
        .unwrap_err()
        .contains("no account"));
    assert!(decide(0, None, Some("not-a-number")).is_err());
    // Root never acts for root: the service does not run as root.
    assert!(decide(0, Some("root"), None)
        .unwrap_err()
        .contains("does not run as root"));
    assert!(decide(0, None, Some("0")).is_err());
    // Anyone else: itself, with or without its own name; never another account.
    assert_eq!(decide(61000, None, None), Ok(None));
    assert_eq!(
        decide(61000, None, Some("0")),
        Ok(None),
        "SUDO_UID means nothing here"
    );
    assert_eq!(decide(61000, Some("agentic"), None), Ok(None));
    assert!(decide(61000, Some("other"), None)
        .unwrap_err()
        .contains("only root acts for another user"));
    assert!(decide(1001, Some("agentic"), None).is_err());
    assert!(decide(61000, Some("nobody-here"), None).is_err());
    // The real account database knows this process's own user, by number and by name.
    let me = account_by_id(crate::home::euid()).expect("this user has an account");
    assert_eq!(me.uid, crate::home::euid());
    assert_eq!(account_by_name("no-such-account-x5"), None);
}

/// A service manager no foreground setup may touch.
struct Untouchable;
impl ServiceManager for Untouchable {
    fn register(&self, _: &Unit) -> Result<org_platform::service::Registration, ServiceError> {
        panic!("the foreground mode registered a service")
    }
    fn start(&self, _: &str) -> Result<(), ServiceError> {
        panic!("the foreground mode started a service")
    }
    fn stop(&self, _: &str) -> Result<(), ServiceError> {
        panic!("the foreground mode stopped a service")
    }
    fn status(&self, _: &str) -> Result<ServiceStatus, ServiceError> {
        panic!("the foreground mode asked for a service's status")
    }
    fn unregister(&self, _: &str) -> Result<(), ServiceError> {
        panic!("the foreground mode unregistered a service")
    }
    fn available(&self) -> Result<(), ServiceError> {
        panic!("the foreground mode asked whether a service manager answers")
    }
    fn lingering(&self) -> Lingering {
        panic!("the foreground mode asked about lingering")
    }
}

/// Non-interactive setup in the foreground mode: the record is frozen from the file's
/// input, the instance is reserved and its layout made, and that is all. No service
/// manager is asked anything, nothing is waited for, no link is asked for or printed, and a
/// second run changes nothing. A file that differs from what the instance was set up with
/// is refused.
#[test]
fn foreground_setup_reserves_the_instance_and_never_touches_a_service_manager() {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let base = temp.path().canonicalize().unwrap().join("b");
    for dir in [base.clone(), base.join("t")] {
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let paths = Paths::at(&base, base.join("t"), None).unwrap();
    let input = Input {
        version: 1,
        service: Service::Foreground,
        root: base.join("data"),
        credentials: None,
        listen: loopback(7420).unwrap(),
        certificate: None,
    };
    let platform = Platform {
        service: Box::new(Untouchable),
        disk: Box::new(org_platform::testing::FakeDiskEncryption(
            EncryptionState::Unknown("a test".into()),
        )),
        credentials: CredentialDirectory::Unused,
    };
    let run = |input: &Input| {
        let request = Request {
            name: None,
            port: None,
            prompt: None,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            // Even at a terminal: the configuration decides, and no link is printed.
            terminal: true,
            configuration: Some(Configuration(input.clone())),
        };
        assert!(request.configuration.as_ref().unwrap().foreground());
        let mut out = Vec::new();
        let result = run_at(&paths, &platform, request, &mut out);
        (result, String::from_utf8(out).unwrap())
    };
    let (result, out) = run(&input);
    result.unwrap();
    assert!(
        out.contains("Service: none is registered (foreground mode)."),
        "{out}"
    );
    assert!(out.contains("agentic-org serve --setup <file>"), "{out}");
    assert!(out.contains("agentic-org admin reset"), "{out}");
    assert!(!out.contains("/register"), "{out}");
    assert!(!out.contains("Service registered"), "{out}");
    let record = Journal::snapshot(&base, None).unwrap();
    assert_eq!(record.input, input);
    assert!(record.offline, "non-interactive setup is offline");
    assert!(record.instance_id.is_some());
    assert_eq!(
        (
            record.service_registered,
            record.registry_created,
            record.initial_link_issued,
            &record.credentials
        ),
        (false, false, false, &None)
    );
    assert!(
        base.join("data/office-state").is_dir(),
        "the layout is made"
    );
    // A second run is a no-op that says the same.
    let (result, again) = run(&input);
    result.unwrap();
    assert_eq!(again, out);
    assert_eq!(Journal::snapshot(&base, None).unwrap(), record);
    // Another configuration for the same instance is refused, whatever differs.
    let mut other = input.clone();
    other.service = Service::Registered;
    let mut moved = input.clone();
    moved.listen = loopback(7421).unwrap();
    for changed in [other, moved] {
        let request = Request {
            name: None,
            port: None,
            prompt: None,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            terminal: false,
            configuration: Some(Configuration(changed)),
        };
        let mut out = Vec::new();
        let refused = run_at(&paths, &platform, request, &mut out).unwrap_err();
        assert!(refused.contains("a different configuration"), "{refused}");
        assert!(refused.ends_with("nothing was changed"), "{refused}");
    }
    assert_eq!(Journal::snapshot(&base, None).unwrap(), record);
}

/// The configuration file is read strictly and read-only, and says what it is missing.
#[test]
fn a_configuration_file_is_read_without_changing_anything() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("setup.json");
    let missing = configuration(&file).unwrap_err();
    assert!(
        missing.starts_with("reading the setup configuration "),
        "{missing}"
    );
    std::fs::write(&file, b"{\"version\": 1}").unwrap();
    let refused = configuration(&file).unwrap_err();
    assert!(
        refused.contains("\"service\": \"registered\" or \"foreground\"")
            && refused.ends_with("nothing was changed"),
        "{refused}"
    );
    std::fs::write(
        &file,
        br#"{"version":1,"service":"registered","root":"/srv/agentic/instance",
            "listen":{"address":"127.0.0.1","port":7420,"scheme":"http_loopback",
                      "origin":"http://127.0.0.1:7420","hsts":"off"}}"#,
    )
    .unwrap();
    let registered = configuration(&file).unwrap();
    assert!(!registered.foreground());
    assert_eq!(registered.0.root, PathBuf::from("/srv/agentic/instance"));
    assert_eq!(
        std::fs::read_dir(temp.path()).unwrap().count(),
        1,
        "reading made a file"
    );
}

mod credentials {
    //! The credential directory setup makes and records (ADR-0017 decision 8).
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    use org_platform::service::{Registration, ServiceError};
    use org_platform_host::credentials::{protected_store, ProtectedStore};
    use org_platform_host::testing::credentials::release_directory_below;

    use super::*;

    /// A private application base on disk, and a reserved instance in it.
    struct Account {
        base: PathBuf,
        _temp: tempfile::TempDir,
    }

    fn account_in(shared: &Path) -> Account {
        let temp = tempfile::tempdir_in(shared).unwrap();
        let base = temp.path().canonicalize().unwrap();
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
        Account { base, _temp: temp }
    }

    fn account() -> Account {
        account_in(&org_platform::dirs::testing::disk_backed_temp().unwrap())
    }

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().mode() & 0o7777
    }

    fn release_directory_system() -> bool {
        protected_store() == Ok(ProtectedStore::ReleaseDirectory)
    }

    #[test]
    fn a_development_build_records_no_credential_directory() {
        // These tests are a development build: its secrets are in its key folder.
        assert_eq!(
            CredentialDirectory::for_host(Path::new("/home/someone")),
            Ok(CredentialDirectory::Unused)
        );
    }

    #[test]
    fn setup_makes_the_directory_owner_only_with_its_parents_and_checks_it_as_the_host_will() {
        let account = account();
        let data = account.base.join("data");
        fs::create_dir(&data).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
        let data_root = fs::File::open(&data).unwrap();
        let data_fd = || std::os::fd::AsFd::as_fd(&data_root);
        let id = crate::ids::uuid_v7().unwrap();
        // Neither `state` nor anything below it exists yet.
        let credentials = account.base.join("state/agentic-org/credentials");
        let enter = |location: org_platform_host::credentials::ReleaseDirectory<'_>| {
            release_directory_below(&account.base, location)
        };

        if !release_directory_system() {
            // The platform refuses a release directory here, so setup refuses too.
            let before = tree(&account.base);
            let refused =
                make_credential_directory(&credentials, &id, data_fd(), &account.base, enter);
            assert!(refused.unwrap_err().contains("cannot be used"));
            assert_eq!(
                tree(&account.base),
                before,
                "a refused location kept something"
            );
            return;
        }
        let made =
            make_credential_directory(&credentials, &id, data_fd(), &account.base, enter).unwrap();
        assert_eq!(made, credentials.join(&id));
        for dir in [
            account.base.join("state"),
            account.base.join("state/agentic-org"),
            credentials.clone(),
            made.clone(),
        ] {
            assert_eq!(mode(&dir), 0o700, "{}", dir.display());
            assert_eq!(
                fs::symlink_metadata(&dir).unwrap().uid(),
                crate::home::euid()
            );
        }
        // The check opened it as the host does: its lock is there, and nothing else.
        let entries: Vec<_> = fs::read_dir(&made)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, [".lock"]);
        // A second run finds the same directory and leaves it as it is.
        assert_eq!(
            make_credential_directory(&credentials, &id, data_fd(), &account.base, enter).unwrap(),
            made
        );
        // A base spelled with `.` or a repeated separator is the same directory, in the
        // one spelling the record accepts.
        let spelled = PathBuf::from(format!(
            "{}//state/./agentic-org/credentials/",
            account.base.display()
        ));
        assert_eq!(
            make_credential_directory(&spelled, &id, data_fd(), &account.base, enter).unwrap(),
            made
        );

        // A directory the host would refuse is refused now, in the host's words, and is
        // not repaired.
        fs::set_permissions(&made, fs::Permissions::from_mode(0o750)).unwrap();
        let refused = make_credential_directory(&credentials, &id, data_fd(), &account.base, enter)
            .unwrap_err();
        assert_eq!(
            refused,
            format!(
                "the credential directory cannot be used: {}: the credential directory's mode \
                 is not 0700",
                made.display()
            )
        );
        assert_eq!(mode(&made), 0o750);
        // No absolute base, a base with `..`, or an instance ID that is none: nothing is
        // made and nothing is opened.
        let before = tree(&account.base);
        let dotted = account.base.join("state/../elsewhere");
        for (base, instance) in [
            (Path::new("relative/credentials"), id.as_str()),
            (dotted.as_path(), id.as_str()),
            (credentials.as_path(), "../escape"),
            (credentials.as_path(), ""),
        ] {
            let never = |_: org_platform_host::credentials::ReleaseDirectory<'_>| panic!("opened");
            assert!(
                make_credential_directory(base, instance, data_fd(), &account.base, never).is_err()
            );
        }
        assert_eq!(tree(&account.base), before);
    }

    /// Every path below `root` with its kind and mode, sorted: what a refused location
    /// must leave exactly as it was.
    fn tree(root: &Path) -> Vec<(PathBuf, bool, u32)> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_owned()];
        while let Some(path) = pending.pop() {
            let meta = fs::symlink_metadata(&path).unwrap();
            found.push((path.clone(), meta.is_dir(), meta.mode()));
            if meta.is_dir() {
                pending.extend(fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
            }
        }
        found.sort();
        found
    }

    /// X3 review S4: a location that is refused leaves nothing behind, whichever rule
    /// refuses it and however far the walk came. On every system: the rules that make
    /// nothing are the host's own, and the platform's check is given by the test.
    #[test]
    fn a_refused_location_is_left_exactly_as_it_was_found() {
        let account = account();
        let data = account.base.join("data");
        let other = account.base.join("other");
        for dir in [&data, &other] {
            fs::create_dir(dir).unwrap();
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::os::unix::fs::symlink(&other, account.base.join("link")).unwrap();
        fs::write(account.base.join("file"), b"x").unwrap();
        let open = account.base.join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
        let data_root = fs::File::open(&data).unwrap();
        let data_fd = || std::os::fd::AsFd::as_fd(&data_root);
        let id = crate::ids::uuid_v7().unwrap();
        let before = tree(&account.base);
        let never =
            |_: org_platform_host::credentials::ReleaseDirectory<'_>| -> Result<
                org_platform_host::credentials::ReleaseRoot,
                org_platform_host::credentials::CredentialError,
            > { panic!("the platform was asked about a location the walk refused") };

        // Inside the data root, at any depth; through a symbolic link; below a file; below
        // a directory that others can write to. Nothing is made for any of them.
        for (base, rule) in [
            (data.join("credentials"), "is inside the data root"),
            (data.join("a/b/credentials"), "is inside the data root"),
            (account.base.join("link/credentials"), "is not a directory"),
            (account.base.join("file/credentials"), "is not a directory"),
            (open.join("state/credentials"), "is writable by others"),
        ] {
            let refused =
                make_credential_directory(&base, &id, data_fd(), &account.base, never).unwrap_err();
            assert!(
                refused.starts_with("the credential directory cannot be used: ")
                    && refused.contains(rule),
                "{}: {refused}",
                base.display()
            );
            assert_eq!(tree(&account.base), before, "{}", base.display());
        }
        assert!(fs::read_dir(&other).unwrap().next().is_none());

        // The walk passes and makes four directories; then the platform's check refuses
        // (as it does for a filesystem that cannot hold a store). All four are gone again.
        let state = account.base.join("state/agentic-org/credentials");
        let refusing = |location: org_platform_host::credentials::ReleaseDirectory<'_>| {
            assert!(
                location.path.is_dir(),
                "the directory exists when it is checked"
            );
            assert!(!location.mount_point_required && !location.expect_items);
            // What the platform's check leaves in a store it opened.
            fs::write(location.path.join(".lock"), b"").unwrap();
            Err(org_platform_host::credentials::CredentialError::Unsafe)
        };
        let refused =
            make_credential_directory(&state, &id, data_fd(), &account.base, refusing).unwrap_err();
        assert!(
            refused.starts_with("the credential directory cannot be used: "),
            "{refused}"
        );
        assert_eq!(tree(&account.base), before);

        // Directories that existed before a refusal stay, with what they hold.
        fs::create_dir_all(account.base.join("state/agentic-org")).unwrap();
        fs::write(account.base.join("state/keep"), b"x").unwrap();
        let before = tree(&account.base);
        assert!(
            make_credential_directory(&state, &id, data_fd(), &account.base, refusing).is_err()
        );
        assert_eq!(tree(&account.base), before);

        // The walk itself: how many directories it made, and that it follows no link.
        let made =
            store_directories::make(&state.join(&id), Some(data_fd()), &account.base).unwrap();
        assert_eq!(made.count(), 2);
        assert_eq!(mode(&state.join(&id)), 0o700);
        made.remove();
        assert_eq!(tree(&account.base), before);
        assert!(
            store_directories::make(Path::new("/elsewhere/x"), Some(data_fd()), &account.base)
                .unwrap_err()
                .contains("is not below")
        );
    }

    /// The foreground mode: the credential directory is a mounted volume. Setup never
    /// makes it, asks the platform to require the mount, and says so when it is refused.
    #[test]
    fn in_the_foreground_mode_setup_makes_no_credential_directory_and_requires_a_mount() {
        let account = account();
        let data = account.base.join("data");
        fs::create_dir(&data).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
        let data_root = fs::File::open(&data).unwrap();
        let data_fd = || std::os::fd::AsFd::as_fd(&data_root);
        let secrets = account.base.join("secrets");
        let before = tree(&account.base);
        let asked = std::cell::Cell::new(0);
        let refusing = |location: org_platform_host::credentials::ReleaseDirectory<'_>| {
            asked.set(asked.get() + 1);
            assert_eq!(location.path, secrets.as_path());
            assert!(location.mount_point_required, "the mount is required");
            assert!(!location.expect_items);
            assert!(!location.path.exists(), "setup made the directory");
            Err(org_platform_host::credentials::CredentialError::Unsafe)
        };
        let refused = use_credential_directory(&secrets, true, data_fd(), &account.base, refusing)
            .unwrap_err();
        assert_eq!(asked.get(), 1);
        assert!(
            refused.starts_with("the credential directory cannot be used: ")
                && refused.contains("mounted volume")
                && refused.contains("setup does not make it"),
            "{refused}"
        );
        assert_eq!(tree(&account.base), before);
        // The registered mode makes the directory at exactly the configured path and does
        // not require a mount.
        let lenient = |location: org_platform_host::credentials::ReleaseDirectory<'_>| {
            assert!(!location.mount_point_required);
            assert!(location.path.is_dir());
            Err(org_platform_host::credentials::CredentialError::Unsafe)
        };
        let refused = use_credential_directory(&secrets, false, data_fd(), &account.base, lenient)
            .unwrap_err();
        assert!(!refused.contains("mounted volume"), "{refused}");
        assert_eq!(tree(&account.base), before, "a refused directory was kept");
        // On this system, with the platform's own check: a plain directory is not a mount.
        if release_directory_system() {
            fs::create_dir(&secrets).unwrap();
            fs::set_permissions(&secrets, fs::Permissions::from_mode(0o700)).unwrap();
            let enter = |location: org_platform_host::credentials::ReleaseDirectory<'_>| {
                release_directory_below(&account.base, location)
            };
            let refused = use_credential_directory(&secrets, true, data_fd(), &account.base, enter)
                .unwrap_err();
            println!("a plain directory in the foreground mode: {refused}");
            assert!(refused.contains("mount"), "{refused}");
            assert_eq!(
                use_credential_directory(&secrets, false, data_fd(), &account.base, enter),
                Ok(secrets.clone())
            );
        }
    }

    /// A service manager that registers nothing: setup stops there, after its third step.
    struct NoService;
    impl ServiceManager for NoService {
        fn register(&self, _unit: &Unit) -> Result<Registration, ServiceError> {
            Err(ServiceError::System(
                "no service manager in this test".into(),
            ))
        }
        fn start(&self, _instance_id: &str) -> Result<(), ServiceError> {
            unreachable!("nothing was registered")
        }
        fn stop(&self, _instance_id: &str) -> Result<(), ServiceError> {
            unreachable!("nothing was registered")
        }
        fn status(&self, _instance_id: &str) -> Result<ServiceStatus, ServiceError> {
            unreachable!("nothing was registered")
        }
        fn unregister(&self, _instance_id: &str) -> Result<(), ServiceError> {
            unreachable!("nothing was registered")
        }
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// Runs setup in `account` with a credential directory below `state`, up to the
    /// service step, which this test's manager refuses.
    fn run_setup(account: &Account, state: &Path, port: u16) -> (Result<(), String>, String) {
        // The user temp base has a short path of its own (socket paths are bounded).
        let temp = account._temp.path().canonicalize().unwrap().join("t");
        if !temp.exists() {
            fs::create_dir(&temp).unwrap();
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let paths = Paths::at(&account.base, temp, None).unwrap();
        let platform = Platform {
            service: Box::new(NoService),
            disk: Box::new(org_platform::testing::FakeDiskEncryption(
                EncryptionState::On,
            )),
            credentials: CredentialDirectory::Below(state.to_owned()),
        };
        let request = Request {
            name: None,
            port: Some(port),
            prompt: None,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            terminal: false,
            configuration: None,
        };
        let mut out = Vec::new();
        let result = run_at(&paths, &platform, request, &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    /// Setup on this machine, with the host's own walk from `/`. A credential directory
    /// below a shared temporary directory is one the host would refuse (that directory is
    /// writable by others): setup refuses it in the host's words and saves no instance.
    /// Where cargo's test directory passes the walk, setup makes the directory there,
    /// records it with the instance, and a second run keeps it.
    #[test]
    fn setup_records_the_directory_once_with_the_instance_and_refuses_one_the_host_would() {
        let shared = account();
        let port = free_port();
        let state = shared.base.join("state/agentic-org/credentials");
        let (result, _) = run_setup(&shared, &state, port);
        let refused = result.unwrap_err();
        assert!(
            refused.starts_with("the credential directory cannot be used: "),
            "{refused}"
        );
        // Nothing was saved: no instance is recorded without its credential directory.
        let record = Journal::snapshot(&shared.base, None).unwrap();
        assert_eq!((record.instance_id, record.credentials), (None, None));
        if !release_directory_system() {
            return;
        }
        assert!(refused.contains("is writable by others"), "{refused}");

        // <target>/<profile>/deps/<test binary>
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let tmp = executable.ancestors().nth(3).unwrap().join("tmp");
        if fs::create_dir_all(&tmp).is_err() {
            println!("skipped: {} cannot be made", tmp.display());
            return;
        }
        let Ok(temp) = tempfile::tempdir_in(&tmp) else {
            println!("skipped: nothing can be made in {}", tmp.display());
            return;
        };
        let clean = Account {
            base: temp.path().canonicalize().unwrap(),
            _temp: temp,
        };
        // The application base is private; the credential directory is next to it, below a
        // directory of this user that others can pass through, as a home directory is.
        let home = clean.base.clone();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o755)).unwrap();
        let account = Account {
            base: home.join("share"),
            _temp: tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
                .unwrap(),
        };
        fs::create_dir(&account.base).unwrap();
        fs::set_permissions(&account.base, fs::Permissions::from_mode(0o700)).unwrap();
        let state = home.join("state/agentic-org/credentials");
        let (result, out) = run_setup(&account, &state, port);
        let stopped = result.unwrap_err();
        if stopped.starts_with("the credential directory cannot be used: ") {
            println!("skipped: no directory here passes the ancestor rule: {stopped}");
            return;
        }
        assert!(stopped.starts_with("registering the service"), "{stopped}");
        let record = Journal::snapshot(&account.base, None).unwrap();
        let id = record
            .instance_id
            .clone()
            .expect("the instance is recorded");
        let directory = state.join(&id);
        assert_eq!(record.credentials.as_deref(), Some(directory.as_path()));
        assert!(
            out.contains(&format!("Secrets: {}\n", directory.display())),
            "{out}"
        );
        assert_eq!(mode(&directory), 0o700);
        assert_eq!(mode(&state), 0o700);
        // Outside the data root.
        assert!(!directory.starts_with(&record.input.root));
        // A second run, with another state folder in its platform (as if `XDG_STATE_HOME`
        // had changed): the record's directory stays, and nothing is made elsewhere.
        let other = home.join("other-state/agentic-org/credentials");
        let (result, out) = run_setup(&account, &other, port);
        assert!(result.unwrap_err().starts_with("registering the service"));
        assert_eq!(Journal::snapshot(&account.base, None).unwrap(), record);
        assert!(
            out.contains(&format!("Secrets: {}\n", directory.display())),
            "{out}"
        );
        assert!(!home.join("other-state").exists());
        println!("setup recorded {}", directory.display());
    }
}
