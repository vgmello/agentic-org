use super::*;
#[test]
fn service_xml_and_registered_arguments_keep_exact_paths_and_scope() {
    let program = Path::new("/private/fixture & data/agentic-org");
    let args = vec!["serve".into(), "--name".into(), "lab".into()];
    let text = String::from_utf8(
        description(
            "com.ellosoft.agentic-org.TEST",
            program,
            &args,
            Path::new("/Users/generated"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(text.contains("fixture &amp; data"));
    assert!(text.contains("<string>--name</string><string>lab</string>"));
    assert!(xml("bad\narg").is_err());
    let unit = Path::new("/private/fixture/service.plist");
    let printed=format!("gui/123/test = {{\n path = {}\n program = {}\n arguments = {{\n {}\n serve\n --name\n lab\n }}\n}}",unit.display(),program.display(),program.display());
    assert!(registration_matches(
        printed.as_bytes(),
        unit,
        program,
        &args
    ));
    assert!(!registration_matches(
        printed.replace("--name", "--server-bin").as_bytes(),
        unit,
        program,
        &args
    ));
    assert!(!registration_matches(
        printed
            .replace(
                "path = /private/fixture/service.plist",
                "path = /different.plist"
            )
            .as_bytes(),
        unit,
        program,
        &args
    ));
}
#[test]
fn autostart_is_create_new_descriptor_bound_and_never_takes_other_state() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    fs::create_dir(base.join("Library")).unwrap();
    let directory = AgentDirectory::open(&base.join("Library/LaunchAgents")).unwrap();
    let target = base.join("protected.plist");
    fs::write(&target, b"public fixture unit").unwrap();
    directory.ensure_link("fixture.plist", &target).unwrap();
    directory.ensure_link("fixture.plist", &target).unwrap();
    assert!(directory
        .ensure_link("fixture.plist", &base.join("different"))
        .is_err());
    let occupied = base.join("Library/LaunchAgents/occupied.plist");
    fs::write(&occupied, b"unrelated fixture").unwrap();
    assert!(directory.ensure_link("occupied.plist", &target).is_err());
    assert_eq!(fs::read(occupied).unwrap(), b"unrelated fixture");
    assert!(directory.ensure_link("../escape", &target).is_err());
    let original = base.join("held-original");
    fs::rename(base.join("Library/LaunchAgents"), &original).unwrap();
    fs::create_dir(base.join("Library/LaunchAgents")).unwrap();
    assert!(directory.ensure_link("held.plist", &target).is_err());
    assert!(!original.join("held.plist").exists());
    assert!(!base.join("Library/LaunchAgents/held.plist").exists());
}

#[test]
#[ignore = "root-only explicit disposable launchd registration; never a live unit or account directory"]
fn actual_disposable_launchd_reconciles_exact_unit_and_cleans_owned_identity() {
    let parent = PathBuf::from(
        std::env::var_os("ORG_SERVICE_FIXTURE_ROOT")
            .expect("explicit generated private /private/tmp root"),
    );
    let parent = parent.canonicalize().unwrap();
    let m = fs::symlink_metadata(&parent).unwrap();
    assert!(
        parent.starts_with("/private/tmp/")
            && m.is_dir()
            && m.uid() == org_socket::effective_uid()
            && m.mode() & 0o7777 == 0o700
    );
    let temp = tempfile::tempdir_in(&parent).unwrap();
    let account = temp.path().canonicalize().unwrap();
    fs::create_dir(account.join("Library")).unwrap();
    let label = format!(
        "com.ellosoft.agentic-org.fixture.{}",
        crate::ids::generate().unwrap()
    );
    let job = format!("gui/{}/{label}", org_socket::effective_uid());
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = launchctl(&["bootout".as_ref(), self.0.as_ref()]);
        }
    }
    let cleanup = Cleanup(job.clone());
    let program = Path::new("/usr/bin/true");
    let args = Vec::new();
    let unit = account.join("service.plist");
    fs::write(
        &unit,
        description(&label, program, &args, &account).unwrap(),
    )
    .unwrap();
    fs::set_permissions(&unit, fs::Permissions::from_mode(0o600)).unwrap();
    reconcile(&account, &label, &unit, program, &args).unwrap();
    reconcile(&account, &label, &unit, program, &args).unwrap();
    let printed = launchctl(&["print".as_ref(), job.as_ref()]).unwrap();
    assert!(printed.status.success());
    assert!(registration_matches(&printed.stdout, &unit, program, &args));
    let before = fs::read(&unit).unwrap();
    assert!(reconcile(&account, &label, &unit, Path::new("/bin/false"), &args).is_err());
    assert_eq!(fs::read(&unit).unwrap(), before);
    let stopped = launchctl(&["bootout".as_ref(), job.as_ref()]).unwrap();
    assert!(stopped.status.success());
    drop(cleanup);
    assert!(!launchctl(&["print".as_ref(), job.as_ref()])
        .unwrap()
        .status
        .success());
    eprintln!("disposable launchd bootstrap/exact replay/conflict/bootout verified; owned temporary registration cleaned; no release signing or HQ readiness assertion");
}
