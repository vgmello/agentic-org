use super::*;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    loaded: bool,
    pid: Option<u32>,
    /// The file the job was bootstrapped from, as `print` reports it.
    path: String,
    calls: Vec<Vec<String>>,
}

/// A launchd stand-in: `bootstrap` loads and starts (RunAtLoad), `kill` stops cleanly,
/// `kickstart` starts, `bootout` unloads.
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);

impl Fake {
    fn calls(&self) -> Vec<Vec<String>> {
        self.0.lock().unwrap().calls.clone()
    }
    fn verbs(&self) -> Vec<String> {
        self.calls().into_iter().map(|c| c[0].clone()).collect()
    }
    fn clear(&self) {
        self.0.lock().unwrap().calls.clear();
    }
}

impl Launchctl for Fake {
    fn run(&self, args: &[OsString]) -> io::Result<Outcome> {
        let args: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let mut s = self.0.lock().unwrap();
        s.calls.push(args.clone());
        let ok = |stdout: String| Ok(Outcome { code: 0, stdout });
        match args[0].as_str() {
            "print" if !s.loaded => Ok(Outcome { code: NO_SUCH_SERVICE, stdout: String::new() }),
            "print" => ok(match s.pid {
                Some(pid) => format!("{}= {{\n\tactive count = 1\n\tpath = {}\n\tstate = running\n\n\tpid = {pid}\n\tendpoints = {{\n\t\tpid = 9\n\t\tpath = /x\n\t}}\n}}\n", args[1], s.path),
                None => format!("{} = {{\n\tpath = {}\n\tstate = not running\n}}\n", args[1], s.path),
            }),
            "bootstrap" => {
                s.loaded = true;
                s.path = args[2].clone();
                s.pid = Some(42);
                ok(String::new())
            }
            "kickstart" => {
                s.pid = Some(43);
                ok(String::new())
            }
            "kill" => {
                s.pid = None;
                ok(String::new())
            }
            "bootout" => {
                s.loaded = false;
                s.pid = None;
                ok(String::new())
            }
            _ => Ok(Outcome { code: 1, stdout: String::new() }),
        }
    }
}

const ID: &str = "01965d2e-8b1c-7c3a-9f00-0123456789ab";
const LABEL: &str = "com.ellosoft.agentic-org.test.01965d2e-8b1c-7c3a-9f00-0123456789ab";

fn unit() -> Unit {
    Unit {
        instance_id: ID.into(),
        program: "/opt/agentic org/agentic-org".into(),
        args: vec!["serve".into(), "--name".into(), "a&b<c>".into()],
        env: vec![("HOME".into(), "/Users/a".into())],
    }
}

fn launchd(dir: &Path) -> (Launchd, Fake) {
    let fake = Fake::default();
    let placement = Placement {
        prefix: "com.ellosoft.agentic-org.test.".into(),
        directory: dir.join("LaunchAgents"),
    };
    (Launchd::new(placement, 501, Box::new(fake.clone())), fake)
}

#[test]
fn the_definition_restarts_only_after_a_failure_and_escapes_every_value() {
    let text = String::from_utf8(render(LABEL, &unit())).unwrap();
    assert!(text.contains(&format!("<key>Label</key>\n\t<string>{LABEL}</string>")));
    assert!(text.contains("<string>/opt/agentic org/agentic-org</string>\n\t\t<string>serve</string>\n\t\t<string>--name</string>\n\t\t<string>a&amp;b&lt;c&gt;</string>"));
    assert!(text
        .contains("<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<false/>"));
    assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
    assert!(text.contains("<key>HOME</key>\n\t\t<string>/Users/a</string>"));
    assert!(text.contains("<key>Umask</key>\n\t<integer>63</integer>"));
    assert!(
        !text.contains("ProcessType"),
        "no Background throttling of the host"
    );
    let cr = Unit {
        args: vec!["a\rb".into()],
        ..unit()
    };
    assert!(String::from_utf8(render(LABEL, &cr))
        .unwrap()
        .contains("<string>a&#13;b</string>"));
    assert_eq!(
        render(LABEL, &unit()),
        render(LABEL, &unit()),
        "rendering is deterministic"
    );
}

#[test]
fn registering_twice_writes_and_loads_once_and_never_restarts_a_healthy_job() {
    let temp = tempfile::tempdir().unwrap();
    let (launchd, fake) = launchd(temp.path());
    assert_eq!(launchd.register(&unit()).unwrap(), Registration::Loaded);
    let file = temp.path().join(format!("LaunchAgents/{LABEL}.plist"));
    assert_eq!(fs::read(&file).unwrap(), render(LABEL, &unit()));
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(
        fake.calls(),
        vec![
            vec!["print".to_string(), format!("gui/501/{LABEL}")],
            vec![
                "bootstrap".to_string(),
                "gui/501".into(),
                file.display().to_string()
            ],
        ]
    );
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    fake.clear();
    assert_eq!(launchd.register(&unit()).unwrap(), Registration::Unchanged);
    assert_eq!(fake.verbs(), ["print"]);
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
    fake.clear();
    launchd.start(ID).unwrap();
    assert_eq!(fake.verbs(), ["print"], "a running job is not kickstarted");
    assert_eq!(
        launchd.status(ID).unwrap(),
        ServiceStatus::Running { pid: 42 }
    );
}

#[test]
fn another_definition_or_job_under_the_label_is_never_taken_over() {
    let temp = tempfile::tempdir().unwrap();
    let (launchd, fake) = launchd(temp.path());
    let dir = temp.path().join("LaunchAgents");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{LABEL}.plist"));
    fs::write(&file, b"someone else's job").unwrap();
    assert!(matches!(
        launchd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    assert_eq!(fs::read(&file).unwrap(), b"someone else's job");
    assert!(!fake.verbs().contains(&"bootstrap".to_string()));

    fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink("/etc/hosts", &file).unwrap();
    assert!(matches!(
        launchd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    fs::remove_file(&file).unwrap();

    fake.0.lock().unwrap().loaded = true;
    assert!(matches!(
        launchd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    assert!(!file.exists());
}

#[test]
fn a_job_loaded_from_another_file_under_the_label_is_never_taken_over() {
    let temp = tempfile::tempdir().unwrap();
    let (launchd, fake) = launchd(temp.path());
    launchd.register(&unit()).unwrap();
    let file = temp.path().join(format!("LaunchAgents/{LABEL}.plist"));
    fake.0.lock().unwrap().path = "/Users/a/Library/LaunchAgents/elsewhere.plist".into();
    fake.clear();
    assert!(matches!(
        launchd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    assert!(matches!(launchd.start(ID), Err(ServiceError::Conflict(_))));
    assert!(matches!(launchd.stop(ID), Err(ServiceError::Conflict(_))));
    assert!(matches!(
        launchd.unregister(ID),
        Err(ServiceError::Conflict(_))
    ));
    assert!(matches!(launchd.status(ID), Err(ServiceError::Conflict(_))));
    assert_eq!(fake.verbs(), ["print", "print", "print", "print", "print"]);
    assert_eq!(fs::read(&file).unwrap(), render(LABEL, &unit()));

    // The same file reached through a symlinked directory is ours.
    let linked = temp.path().join("linked");
    std::os::unix::fs::symlink(temp.path().join("LaunchAgents"), &linked).unwrap();
    fake.0.lock().unwrap().path = linked.join(format!("{LABEL}.plist")).display().to_string();
    assert_eq!(launchd.register(&unit()).unwrap(), Registration::Unchanged);
}

#[test]
fn a_definition_left_unloaded_is_loaded_again_only_by_register() {
    let temp = tempfile::tempdir().unwrap();
    let (launchd, fake) = launchd(temp.path());
    launchd.register(&unit()).unwrap();
    fake.0.lock().unwrap().loaded = false;
    assert_eq!(launchd.status(ID).unwrap(), ServiceStatus::Unloaded);
    // Whatever now sits at the definition path is not loaded by start.
    let file = temp.path().join(format!("LaunchAgents/{LABEL}.plist"));
    fs::write(&file, b"replaced").unwrap();
    fake.clear();
    assert!(matches!(launchd.start(ID), Err(ServiceError::Invalid(_))));
    assert!(!fake.verbs().contains(&"bootstrap".to_string()));
    assert!(matches!(
        launchd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    assert!(!fake.verbs().contains(&"bootstrap".to_string()));
    fs::write(&file, render(LABEL, &unit())).unwrap();
    fake.clear();
    assert_eq!(launchd.register(&unit()).unwrap(), Registration::Loaded);
    assert_eq!(fake.verbs(), ["print", "bootstrap"]);
}

#[test]
fn publishing_never_replaces_a_definition_that_appeared_meanwhile() {
    let temp = tempfile::tempdir().unwrap();
    let (launchd, _fake) = launchd(temp.path());
    let file = temp.path().join(format!("LaunchAgents/{LABEL}.plist"));
    launchd.write_definition(&file, b"first").unwrap();
    assert!(matches!(
        launchd.write_definition(&file, b"second"),
        Err(ServiceError::Conflict(_))
    ));
    assert_eq!(fs::read(&file).unwrap(), b"first");
    let leftovers: Vec<_> = fs::read_dir(temp.path().join("LaunchAgents"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        leftovers.len(),
        1,
        "no temporary file remains: {leftovers:?}"
    );
}

#[test]
fn stop_sends_sigterm_only_to_a_running_job_and_start_kickstarts_without_k() {
    let temp = tempfile::tempdir().unwrap();
    let (launchd, fake) = launchd(temp.path());
    assert!(matches!(launchd.start(ID), Err(ServiceError::Invalid(_))));
    assert_eq!(launchd.status(ID).unwrap(), ServiceStatus::NotRegistered);
    launchd.register(&unit()).unwrap();
    fake.clear();
    launchd.stop(ID).unwrap();
    assert_eq!(
        fake.calls()[1],
        vec![
            "kill".to_string(),
            "SIGTERM".into(),
            format!("gui/501/{LABEL}")
        ]
    );
    assert_eq!(launchd.status(ID).unwrap(), ServiceStatus::Stopped);
    fake.clear();
    launchd.stop(ID).unwrap();
    assert_eq!(fake.verbs(), ["print"], "a stopped job gets no signal");
    fake.clear();
    launchd.start(ID).unwrap();
    assert_eq!(
        fake.calls()[1],
        vec!["kickstart".to_string(), format!("gui/501/{LABEL}")]
    );
}

#[test]
fn unregister_boots_out_and_removes_the_definition_and_is_repeatable() {
    let temp = tempfile::tempdir().unwrap();
    let (launchd, fake) = launchd(temp.path());
    launchd.register(&unit()).unwrap();
    launchd.unregister(ID).unwrap();
    assert!(fake.verbs().contains(&"bootout".to_string()));
    assert_eq!(launchd.status(ID).unwrap(), ServiceStatus::NotRegistered);
    launchd.unregister(ID).unwrap();
    assert!(matches!(
        launchd.status("../x"),
        Err(ServiceError::Invalid(_))
    ));
}

#[test]
fn print_parsing_reads_only_the_top_level_state_pid_and_path() {
    assert_eq!(
        parse_path("x = {\n\t\tpath = /inner\n\tpath = /a b/c.plist\n}").as_deref(),
        Some("/a b/c.plist")
    );
    assert_eq!(parse_path("x = {\n\t\tpath = /inner\n}"), None);
    assert_eq!(
        parse_print("x = {\n\tstate = running\n\tpid = 7\n}"),
        ServiceStatus::Running { pid: 7 }
    );
    assert_eq!(
        parse_print("x = {\n\tstate = not running\n\t\tpid = 7\n}"),
        ServiceStatus::Stopped
    );
    assert_eq!(
        parse_print("x = {\n\tstate = running\n}"),
        ServiceStatus::Stopped
    );
}

#[test]
fn the_user_placement_is_the_accounts_launch_agents() {
    let p = user_placement(Path::new("/Users/a")).unwrap();
    assert_eq!(p.directory, Path::new("/Users/a/Library/LaunchAgents"));
    assert_eq!(p.prefix, "com.ellosoft.agentic-org.instance.");
}

/// Opt-in, against the real launchd with a disposable test label and a temporary agents
/// directory: `AGENTIC_ORG_REAL_LAUNCHD=1 cargo test -p org-platform -- --ignored real_launchd --test-threads=1`.
/// The guard boots the job out and removes its definition even when an assertion fails.
#[test]
#[ignore = "registers a real launchd job"]
fn real_launchd() {
    if std::env::var_os("AGENTIC_ORG_REAL_LAUNCHD").is_none() {
        return;
    }
    struct Guard<'a>(&'a dyn ServiceManager, String);
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            let _ = self.0.unregister(&self.1);
        }
    }
    let wait_for = |manager: &dyn ServiceManager, id: &str, running: bool| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let status = manager.status(id).unwrap();
            if matches!(status, ServiceStatus::Running { .. }) == running {
                return status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "still {status:?} after 10 s"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    let temp = tempfile::tempdir().unwrap();
    let mut random = [0u8; 16];
    io::Read::read_exact(&mut fs::File::open("/dev/urandom").unwrap(), &mut random).unwrap();
    let hex: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let id = format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    println!(
        "label com.ellosoft.agentic-org.test.{id} in {}",
        temp.path().display()
    );
    let manager = registered(Placement {
        prefix: "com.ellosoft.agentic-org.test.".into(),
        directory: temp.path().to_owned(),
    })
    .unwrap();
    let _guard = Guard(manager.as_ref(), id.clone());
    // Exits 0 on SIGTERM, as the host does, so launchd does not restart it. It creates
    // `ready` (outside the agents directory) once the trap is installed; a SIGTERM before
    // then would kill it with a signal, which launchd rightly restarts.
    let marker = tempfile::tempdir().unwrap();
    let ready = marker.path().join("ready");
    let unit = Unit {
        instance_id: id.clone(),
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "trap 'exit 0' TERM; : > \"$0\"; while :; do /bin/sleep 1; done".into(),
            ready.clone().into(),
        ],
        env: vec![],
    };
    assert_eq!(manager.register(&unit).unwrap(), Registration::Loaded);
    println!("registered: {:?}", wait_for(manager.as_ref(), &id, true));
    // The real `print` names our file, so an identical registration is left alone.
    assert_eq!(manager.register(&unit).unwrap(), Registration::Unchanged);
    // The top-level (one-tab) `key = value` lines of the raw `launchctl print`.
    let target = format!(
        "gui/{}/com.ellosoft.agentic-org.test.{id}",
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    );
    let top_level = |key: &str| -> Option<String> {
        let out = Command::new(LAUNCHCTL)
            .args(["print", &target])
            .output()
            .unwrap();
        assert!(out.status.success(), "print {target}: {out:?}");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix('\t').filter(|r| !r.starts_with('\t'))?;
                let (k, v) = rest.split_once(" = ")?;
                (k == key).then(|| v.to_owned())
            })
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the job never became ready"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    manager.stop(&id).unwrap();
    println!("stopped: {:?}", wait_for(manager.as_ref(), &id, false));
    // launchd throttles a respawn to 10 s after the last spawn and reports it as
    // "spawn scheduled", which `status` reads as Stopped. The last spawn was before
    // `stop`, so 12 s from here covers the whole throttle window.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while std::time::Instant::now() < deadline {
        assert_eq!(
            manager.status(&id).unwrap(),
            ServiceStatus::Stopped,
            "a clean exit is not restarted"
        );
        assert_eq!(
            top_level("state").as_deref(),
            Some("not running"),
            "a clean exit is not restarted or scheduled"
        );
        assert_eq!(top_level("last exit code").as_deref(), Some("0"));
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    manager.start(&id).unwrap();
    println!("started: {:?}", wait_for(manager.as_ref(), &id, true));
    manager.unregister(&id).unwrap();
    assert_eq!(manager.status(&id).unwrap(), ServiceStatus::NotRegistered);
    assert!(fs::read_dir(temp.path()).unwrap().next().is_none());
}
