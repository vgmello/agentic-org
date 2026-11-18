use super::*;

#[test]
fn ports_default_to_7420_and_named_instances_take_the_first_free_from_7421() {
    assert_eq!(
        choose_port(None, None, NAMED_PORT_BASE, |_| true, &[]).unwrap(),
        7420
    );
    assert_eq!(
        choose_port(Some("default"), None, NAMED_PORT_BASE, |_| true, &[]).unwrap(),
        7420
    );
    assert_eq!(
        choose_port(Some("lab"), None, NAMED_PORT_BASE, |p| p != 7421, &[]).unwrap(),
        7422
    );
    assert_eq!(
        choose_port(Some("lab"), Some(9000), NAMED_PORT_BASE, |_| true, &[]).unwrap(),
        9000
    );
}

/// The production search base is 7421, the port after `default`'s; another base moves the
/// whole search and is named in the refusal.
#[test]
fn the_named_port_search_starts_at_7421_and_at_no_other_base_unless_given() {
    assert_eq!(NAMED_PORT_BASE, 7421);
    assert_eq!(
        choose_port(Some("lab"), None, NAMED_PORT_BASE, |_| true, &[]).unwrap(),
        7421
    );
    assert_eq!(
        choose_port(Some("lab"), None, 31000, |p| p != 31000, &[]).unwrap(),
        31001
    );
    // The base does not move `default`, nor an explicit port.
    assert_eq!(choose_port(None, None, 31000, |_| true, &[]).unwrap(), 7420);
    assert_eq!(
        choose_port(Some("lab"), Some(9000), 31000, |_| true, &[]).unwrap(),
        9000
    );
    let why = choose_port(Some("lab"), None, 65535, |_| false, &[]).unwrap_err();
    assert!(why.contains("from 65535"), "{why}");
}

#[test]
fn ports_other_instances_saved_are_never_chosen() {
    // A stopped instance's saved port is free to bind, but it is still that instance's.
    let saved = [("a".to_owned(), 7421), ("b".to_owned(), 7423)];
    assert_eq!(
        choose_port(Some("lab"), None, NAMED_PORT_BASE, |p| p != 7422, &saved).unwrap(),
        7424
    );
    let why = choose_port(Some("lab"), Some(7423), NAMED_PORT_BASE, |_| true, &saved).unwrap_err();
    assert!(
        why.contains("saved address of instance \"b\"") && why.contains("--port"),
        "{why}"
    );
    let taken = [("lab".to_owned(), DEFAULT_PORT)];
    let why = choose_port(None, None, NAMED_PORT_BASE, |_| true, &taken).unwrap_err();
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
        let why = choose_port(name, requested, NAMED_PORT_BASE, |_| false, &[]).unwrap_err();
        assert!(why.contains("in use") && why.contains("--port"), "{why}");
        assert!(why.contains("nothing was saved"), "{why}");
    }
    assert!(choose_port(None, Some(0), NAMED_PORT_BASE, |_| true, &[]).is_err());
}

#[test]
fn the_protection_report_states_encryption_modes_and_isolation() {
    let on = protection_report(&EncryptionState::On, "alex", &[]);
    assert_eq!(on[0], "Disk encryption (FileVault): on.");
    assert!(on[1].contains("readable only by alex (folders 700, files 600)"));
    assert!(on[2].contains("same-user mode"));
    let off = protection_report(&EncryptionState::Off, "alex", &[]);
    assert!(off[0].starts_with("Disk encryption is off."));
    let unknown = protection_report(&EncryptionState::Unknown("no volume".into()), "alex", &[]);
    assert!(unknown[0].contains("could not be determined (no volume)"));
    let open = protection_report(&EncryptionState::On, "alex", &[("/r/x".into(), 0o644)]);
    assert!(
        open[1].contains("1 path(s)") && open[2] == "  644 /r/x",
        "{open:?}"
    );
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
    let refused = admin_reset(Some("no-such-instance"), false, &mut out).unwrap_err();
    assert_eq!(refused, NOT_A_TERMINAL);
    assert!(out.is_empty());
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
            let refused = make_credential_directory(&credentials, &id, data_fd(), enter);
            assert!(refused.unwrap_err().contains("cannot be used"));
            return;
        }
        let made = make_credential_directory(&credentials, &id, data_fd(), enter).unwrap();
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
            make_credential_directory(&credentials, &id, data_fd(), enter).unwrap(),
            made
        );

        // A directory the host would refuse is refused now, in the host's words, and is
        // not repaired.
        fs::set_permissions(&made, fs::Permissions::from_mode(0o750)).unwrap();
        let refused = make_credential_directory(&credentials, &id, data_fd(), enter).unwrap_err();
        assert_eq!(
            refused,
            format!(
                "the credential directory cannot be used: {}: the credential directory's mode \
                 is not 0700",
                made.display()
            )
        );
        assert_eq!(mode(&made), 0o750);
        // Inside the data root.
        let refused = make_credential_directory(&data.join("credentials"), &id, data_fd(), enter)
            .unwrap_err();
        assert!(refused.ends_with("is inside the data root"), "{refused}");
        // No absolute base, or an instance ID that is none: nothing is made.
        for (base, instance) in [
            (Path::new("relative/credentials"), id.as_str()),
            (credentials.as_path(), "../escape"),
            (credentials.as_path(), ""),
        ] {
            let never = |_: org_platform_host::credentials::ReleaseDirectory<'_>| panic!("opened");
            assert!(make_credential_directory(base, instance, data_fd(), never).is_err());
        }
        assert!(!account.base.join("state/agentic-org/escape").exists());
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
            named_port_base: NAMED_PORT_BASE,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            terminal: false,
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
