use super::*;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    loaded: bool,
    pid: Option<u32>,
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
                Some(pid) => format!("{}= {{\n\tactive count = 1\n\tstate = running\n\n\tpid = {pid}\n\tendpoints = {{\n\t\tpid = 9\n\t}}\n}}\n", args[1]),
                None => format!("{} = {{\n\tstate = not running\n}}\n", args[1]),
            }),
            "bootstrap" => {
                s.loaded = true;
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
fn a_definition_left_unloaded_is_loaded_again_without_rewriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let (launchd, fake) = launchd(temp.path());
    launchd.register(&unit()).unwrap();
    fake.0.lock().unwrap().loaded = false;
    assert_eq!(launchd.status(ID).unwrap(), ServiceStatus::Unloaded);
    fake.clear();
    assert_eq!(launchd.register(&unit()).unwrap(), Registration::Loaded);
    assert_eq!(fake.verbs(), ["print", "bootstrap"]);
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
fn print_parsing_reads_only_the_top_level_state_and_pid() {
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
    let temp = tempfile::tempdir().unwrap();
    let id = format!(
        "t{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let manager = registered(Placement {
        prefix: "com.ellosoft.agentic-org.test.".into(),
        directory: temp.path().to_owned(),
    })
    .unwrap();
    let _guard = Guard(manager.as_ref(), id.clone());
    let unit = Unit {
        instance_id: id.clone(),
        program: "/bin/sleep".into(),
        args: vec!["300".into()],
        env: vec![],
    };
    assert_eq!(manager.register(&unit).unwrap(), Registration::Loaded);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !matches!(manager.status(&id).unwrap(), ServiceStatus::Running { .. }) {
        assert!(
            std::time::Instant::now() < deadline,
            "the job did not start"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(manager.register(&unit).unwrap(), Registration::Unchanged);
    manager.stop(&id).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    // `sleep` exits on SIGTERM by signal, which launchd treats as a failure and restarts;
    // the host exits 0 on SIGTERM. This checks only that stop reaches the job.
    manager.unregister(&id).unwrap();
    assert_eq!(manager.status(&id).unwrap(), ServiceStatus::NotRegistered);
}
