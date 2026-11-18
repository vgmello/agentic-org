use super::*;
use std::sync::{Arc, Mutex};

/// What the stand-in for systemd's user manager holds of the one unit a test registers.
#[derive(Default)]
struct State {
    /// The directory the manager reads unit files from (`None`: it reads none of ours).
    search: Option<PathBuf>,
    /// The file the manager read the unit from, once it has: kept until a reload finds it
    /// gone, as systemd keeps a loaded unit.
    fragment: Option<String>,
    enabled: bool,
    pid: Option<u32>,
    /// Bytes of the file when the manager last read it; a difference is `NeedDaemonReload`.
    read: Option<Vec<u8>>,
    /// A load state other than `loaded` for a file the manager cannot use.
    load: Option<&'static str>,
    /// What the manager lists as the unit's drop-in files.
    drop_ins: String,
    /// `enable` fails, as when the unit's program cannot be started.
    enable_fails: bool,
    /// `Some`: what `loginctl show-user` prints for `Linger`; `None`: it fails.
    linger: Option<&'static str>,
    /// No user manager answers: `systemctl` fails as it does without a user bus.
    no_bus: bool,
    /// The programs are not installed.
    missing: bool,
    no_runtime_dir: bool,
    calls: Vec<Vec<String>>,
}

/// A systemd stand-in: `show` reads a unit file on demand, `daemon-reload` reads it again,
/// `enable --now` enables and starts, `start` starts, `stop` stops, `disable --now` stops
/// and disables.
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);

impl Fake {
    fn calls(&self) -> Vec<Vec<String>> {
        self.0.lock().unwrap().calls.clone()
    }
    /// The verb of each `systemctl --user` call, and `loginctl` for a call of that.
    fn verbs(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .map(|c| {
                if c[0] == "loginctl" {
                    c[0].clone()
                } else {
                    c[2].clone()
                }
            })
            .collect()
    }
    fn clear(&self) {
        self.0.lock().unwrap().calls.clear();
    }
    fn set(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.0.lock().unwrap());
    }
}

impl State {
    /// Reads the unit file as systemd does when it is first asked about a unit.
    fn read_file(&mut self, name: &str, again: bool) {
        let Some(file) = self.search.as_ref().map(|dir| dir.join(name)) else {
            return;
        };
        match fs::read(&file) {
            Ok(bytes) if self.fragment.is_none() || again => {
                self.fragment = Some(file.display().to_string());
                self.read = Some(bytes);
            }
            Ok(_) => {}
            Err(_) if again && self.pid.is_none() => {
                self.fragment = None;
                self.read = None;
                self.enabled = false;
            }
            Err(_) => {}
        }
    }

    fn show(&mut self, name: &str) -> String {
        self.read_file(name, false);
        let on_disk = self
            .search
            .as_ref()
            .and_then(|dir| fs::read(dir.join(name)).ok());
        let (load, fragment) = match &self.fragment {
            Some(fragment) => (self.load.unwrap_or("loaded"), fragment.clone()),
            None => ("not-found", String::new()),
        };
        format!(
            "LoadState={load}\nActiveState={}\nMainPID={}\nFragmentPath={fragment}\n\
             UnitFileState={}\nNeedDaemonReload={}\nDropInPaths={}\n",
            if self.pid.is_some() {
                "active"
            } else {
                "inactive"
            },
            self.pid.unwrap_or(0),
            match (&self.fragment, self.enabled) {
                (None, _) => "",
                (Some(_), true) => "enabled",
                (Some(_), false) => "disabled",
            },
            if self.fragment.is_some() && on_disk != self.read {
                "yes"
            } else {
                "no"
            },
            self.drop_ins,
        )
    }
}

impl Commands for Fake {
    fn runtime_dir(&self) -> Option<PathBuf> {
        (!self.0.lock().unwrap().no_runtime_dir).then(|| "/run/user/1000".into())
    }

    fn run(&self, program: Program, args: &[OsString]) -> io::Result<Outcome> {
        let mut call = vec![program.name().to_owned()];
        call.extend(args.iter().map(|a| a.to_string_lossy().into_owned()));
        let mut s = self.0.lock().unwrap();
        s.calls.push(call.clone());
        if s.missing {
            return Err(io::Error::new(io::ErrorKind::NotFound, "not installed"));
        }
        let done = |code: i32, stdout: String, stderr: &str| {
            Ok(Outcome {
                code,
                stdout,
                stderr: stderr.to_owned(),
            })
        };
        if program == Program::Loginctl {
            assert_eq!(call[1..], ["show-user", "1000", "--property=Linger"]);
            return match s.linger {
                Some(value) => done(0, format!("Linger={value}\n"), ""),
                None => done(1, String::new(), "Failed to get user: no such user\n"),
            };
        }
        assert_eq!(
            call[1], "--user",
            "every systemctl call asks the user manager"
        );
        if s.no_bus {
            return done(
                1,
                String::new(),
                "Failed to connect to bus: No medium found\n",
            );
        }
        let unit = call.last().cloned().unwrap_or_default();
        match call[2].as_str() {
            "show" if call[3] == "--property=Version" => done(0, "Version=255\n".into(), ""),
            "show" => {
                assert_eq!(call[4], PROPERTIES);
                let name = call[3].clone();
                let text = s.show(&name);
                done(0, text, "")
            }
            "daemon-reload" => {
                // The fake holds one unit; its name is the only file in the directory.
                let names: Vec<String> = s
                    .search
                    .as_ref()
                    .and_then(|dir| fs::read_dir(dir).ok())
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok()?.file_name().into_string().ok())
                    .collect();
                if names.is_empty() {
                    s.read_file("gone", true);
                }
                for name in names {
                    s.read_file(&name, true);
                }
                done(0, String::new(), "")
            }
            "enable" => {
                assert_eq!(call[3], "--now");
                if s.fragment.is_none() {
                    return done(1, String::new(), &format!("Unit {unit} does not exist\n"));
                }
                if s.enable_fails {
                    return done(1, String::new(), "Job failed. See journalctl.\n");
                }
                s.enabled = true;
                s.pid.get_or_insert(42);
                done(0, String::new(), "")
            }
            "start" => {
                s.pid.get_or_insert(43);
                done(0, String::new(), "")
            }
            "stop" => {
                s.pid = None;
                done(0, String::new(), "")
            }
            "disable" => {
                assert_eq!(call[3], "--now");
                s.enabled = false;
                s.pid = None;
                done(0, String::new(), "")
            }
            "reset-failed" => done(1, String::new(), "Unit not loaded\n"),
            _ => done(1, String::new(), "unknown verb\n"),
        }
    }
}

const ID: &str = "01965d2e-8b1c-7c3a-9f00-0123456789ab";
const NAME: &str = "agentic-org-test-01965d2e-8b1c-7c3a-9f00-0123456789ab.service";

fn unit() -> Unit {
    Unit {
        instance_id: ID.into(),
        program: "/opt/agentic org/agentic-org".into(),
        args: vec!["serve".into(), "--name".into(), "a&b<c>".into()],
        env: vec![("HOME".into(), "/home/a".into())],
    }
}

fn systemd(dir: &Path) -> (Systemd, Fake) {
    let fake = Fake::default();
    let directory = dir.join("systemd/user");
    fake.set(|s| {
        s.search = Some(directory.clone());
        s.linger = Some("yes");
    });
    let placement = Placement {
        prefix: "agentic-org-test-".into(),
        directory,
    };
    (Systemd::new(placement, 1000, Box::new(fake.clone())), fake)
}

fn rendered(unit: &Unit) -> String {
    String::from_utf8(render(NAME, unit)).unwrap()
}

#[test]
fn the_unit_restarts_only_after_a_failure_and_stops_the_host_first() {
    let text = rendered(&unit());
    assert_eq!(
        text,
        "# Written by agentic-org setup. A changed file is refused, never overwritten.\n\
         [Unit]\n\
         Description=Agentic Org instance (agentic-org-test-01965d2e-8b1c-7c3a-9f00-0123456789ab.service)\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart=\"/opt/agentic org/agentic-org\" \"serve\" \"--name\" \"a&b<c>\"\n\
         Environment=\"HOME=/home/a\"\n\
         KillMode=mixed\n\
         TimeoutStopSec=15\n\
         Restart=on-failure\n\
         UMask=0077\n\
         NoNewPrivileges=yes\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    );
    assert_eq!(
        render(NAME, &unit()),
        render(NAME, &unit()),
        "deterministic"
    );
}

/// Every value is one quoted word on one line: nothing a value holds starts a second
/// command, a second setting, a specifier, a variable or a continuation line.
#[test]
fn every_value_is_escaped_for_the_unit_file_grammar() {
    let hostile = Unit {
        args: vec![
            "a b".into(),
            "tab\there".into(),
            "two\nlines".into(),
            "cr\rhere".into(),
            "quote\"and'apostrophe".into(),
            "back\\slash\\".into(),
            "100%n %h %%".into(),
            "$HOME ${PATH} $$".into(),
            ";".into(),
            "".into(),
            "\nRestart=always".into(),
            "é\u{1F600}".into(),
        ],
        env: vec![
            ("A".into(), "x y\nExecStart=/bin/sh".into()),
            ("B_2".into(), "50% $HOME \"q\" back\\".into()),
            ("_C".into(), "".into()),
        ],
        ..unit()
    };
    assert!(validate(&hostile).is_ok() && unit_file_safe(&hostile).is_ok());
    let text = rendered(&hostile);
    let exec = text.lines().find(|l| l.starts_with("ExecStart=")).unwrap();
    assert_eq!(
        exec,
        "ExecStart=\"/opt/agentic org/agentic-org\" \"a b\" \"tab\\there\" \"two\\nlines\" \
         \"cr\\rhere\" \"quote\\\"and'apostrophe\" \"back\\\\slash\\\\\" \"100%%n %%h %%%%\" \
         \"$$HOME $${PATH} $$$$\" \";\" \"\" \"\\nRestart=always\" \"é\u{1F600}\""
    );
    let environment: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("Environment="))
        .collect();
    assert_eq!(
        environment,
        [
            "Environment=\"A=x y\\nExecStart=/bin/sh\"",
            "Environment=\"B_2=50%% $HOME \\\"q\\\" back\\\\\"",
            "Environment=\"_C=\"",
        ]
    );
    // The settings are exactly the ones the adapter writes: no value added a line.
    let keys: Vec<&str> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('['))
        .map(|l| l.split_once('=').unwrap().0)
        .collect();
    assert_eq!(
        keys,
        [
            "Description",
            "Type",
            "ExecStart",
            "Environment",
            "Environment",
            "Environment",
            "KillMode",
            "TimeoutStopSec",
            "Restart",
            "UMask",
            "NoNewPrivileges",
            "WantedBy"
        ]
    );
    assert!(text
        .lines()
        .all(|l| !l.ends_with('\\') || l.ends_with("\\\\\"")));
    for line in text.lines().filter(|l| l.ends_with('"')) {
        assert!(
            !line.ends_with("\\\"") || line.ends_with("\\\\\""),
            "a line ends inside a quoted word: {line}"
        );
    }
}

/// systemd's own parser reads the rendered unit without a complaint, hostile values
/// included: `systemd-analyze verify`, where this machine has it (it needs no manager).
/// It reports each line it drops or refuses with the file's name.
#[test]
fn systemd_analyze_accepts_the_rendered_unit() {
    let Some(analyze) = ["/usr/bin/systemd-analyze", "/bin/systemd-analyze"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
    else {
        println!("skipped: this machine has no systemd-analyze");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let verify = |name: &str, bytes: &[u8]| {
        let file = temp.path().join(name);
        fs::write(&file, bytes).unwrap();
        let out = Command::new(analyze)
            .arg("verify")
            .arg(&file)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stderr).into_owned();
        let about_ours: Vec<String> = said
            .lines()
            .filter(|line| line.contains(name))
            .map(str::to_owned)
            .collect();
        (out.status.code(), about_ours, said)
    };
    let hostile = Unit {
        program: "/bin/sh".into(),
        args: vec![
            "a b".into(),
            "two\nlines\tand\rmore".into(),
            "quote\"and'apostrophe".into(),
            "back\\slash\\".into(),
            "100%n %h %%".into(),
            "$HOME ${PATH} $$".into(),
            ";".into(),
            "".into(),
            "\nRestart=always".into(),
        ],
        env: vec![
            ("A".into(), "x y\nExecStart=/bin/sh".into()),
            ("B_2".into(), "50% $HOME \"q\" back\\".into()),
            ("_C".into(), "".into()),
        ],
        ..unit()
    };
    assert!(validate(&hostile).is_ok() && unit_file_safe(&hostile).is_ok());
    let name = "agentic-org-test-verify.service";
    let (code, about_ours, said) = verify(name, &render(name, &hostile));
    assert!(about_ours.is_empty(), "systemd complains: {said}");
    println!("systemd-analyze verify of the rendered unit: exit {code:?}");
    // The check can see a mistake: what the adapter refuses, systemd drops.
    let name = "agentic-org-test-dropped.service";
    let dropped = String::from_utf8(render(name, &unit()))
        .unwrap()
        .replace("Environment=\"HOME=/home/a\"", "Environment=MY-VAR=x")
        .replace("\"/opt/agentic org/agentic-org\"", "\"/bin/sh\"");
    let (_, about_ours, said) = verify(name, dropped.as_bytes());
    assert!(
        about_ours.iter().any(|line| line.contains("MY-VAR")),
        "systemd-analyze did not report a dropped assignment: {said}"
    );
}

/// What the grammar cannot carry is refused before a file is written or systemd is asked.
#[test]
fn a_unit_the_grammar_cannot_carry_is_refused_and_nothing_is_written() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    for broken in [
        Unit {
            env: vec![("MY-VAR".into(), "x".into())],
            ..unit()
        },
        Unit {
            env: vec![("1A".into(), "x".into())],
            ..unit()
        },
        Unit {
            env: vec![("A B".into(), "x".into())],
            ..unit()
        },
        Unit {
            env: vec![("É".into(), "x".into())],
            ..unit()
        },
        Unit {
            env: vec![("A".into(), "x\ry".into())],
            ..unit()
        },
        Unit {
            env: vec![("A".into(), "x\u{7f}".into())],
            ..unit()
        },
        Unit {
            program: "/opt/$HOME/agentic-org".into(),
            ..unit()
        },
        // The contract's own refusals reach this adapter too.
        Unit {
            env: vec![("LD_PRELOAD".into(), "/x".into())],
            ..unit()
        },
        Unit {
            program: "agentic-org".into(),
            ..unit()
        },
    ] {
        assert!(
            matches!(systemd.register(&broken), Err(ServiceError::Invalid(_))),
            "{broken:?}"
        );
    }
    assert!(
        fake.calls().is_empty(),
        "systemd was asked: {:?}",
        fake.calls()
    );
    assert!(!temp.path().join("systemd").exists());
}

#[test]
fn registering_twice_writes_and_enables_once_and_never_restarts_a_healthy_service() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    assert!(systemd.available().is_ok());
    fake.clear();
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Loaded);
    let file = temp.path().join(format!("systemd/user/{NAME}"));
    assert_eq!(fs::read(&file).unwrap(), render(NAME, &unit()));
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o644
    );
    let show = vec![
        "systemctl".to_string(),
        "--user".into(),
        "show".into(),
        NAME.into(),
        PROPERTIES.into(),
    ];
    assert_eq!(
        fake.calls(),
        vec![
            show.clone(),
            vec![
                "systemctl".to_string(),
                "--user".into(),
                "daemon-reload".into()
            ],
            show.clone(),
            vec![
                "systemctl".to_string(),
                "--user".into(),
                "enable".into(),
                "--now".into(),
                NAME.into()
            ],
        ]
    );
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    fake.clear();
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Unchanged);
    assert_eq!(fake.verbs(), ["show"]);
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
    fake.clear();
    systemd.start(ID).unwrap();
    assert_eq!(
        fake.verbs(),
        ["show"],
        "a running service is not started again"
    );
    assert_eq!(
        systemd.status(ID).unwrap(),
        ServiceStatus::Running { pid: 42 }
    );
    let leftovers: Vec<_> = fs::read_dir(temp.path().join("systemd/user"))
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
fn another_definition_under_the_name_is_never_taken_over() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    let dir = temp.path().join("systemd/user");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(NAME);
    fs::write(&file, b"[Service]\nExecStart=/bin/someone-elses\n").unwrap();
    assert!(matches!(
        systemd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    assert_eq!(
        fs::read(&file).unwrap(),
        b"[Service]\nExecStart=/bin/someone-elses\n"
    );
    for verb in ["daemon-reload", "enable", "start"] {
        assert!(!fake.verbs().contains(&verb.to_string()), "{verb} ran");
    }

    // A symbolic link at the path is not a definition of ours, wherever it leads.
    fs::remove_file(&file).unwrap();
    fake.set(|s| s.search = None);
    std::os::unix::fs::symlink("/etc/hosts", &file).unwrap();
    assert!(matches!(
        systemd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    fs::remove_file(&file).unwrap();

    // systemd holds a unit under the name and our file does not exist.
    fake.set(|s| s.fragment = Some(file.display().to_string()));
    assert!(matches!(
        systemd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    assert!(!file.exists());
}

#[test]
fn a_unit_systemd_has_from_another_file_under_the_name_is_never_taken_over() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    systemd.register(&unit()).unwrap();
    let file = temp.path().join(format!("systemd/user/{NAME}"));
    for elsewhere in [
        "/etc/systemd/user/elsewhere.service".to_owned(),
        format!("/run/user/1000/systemd/transient/{NAME}"),
        "/dev/null".to_owned(),
    ] {
        fake.set(|s| s.fragment = Some(elsewhere.clone()));
        fake.clear();
        assert!(matches!(
            systemd.register(&unit()),
            Err(ServiceError::Conflict(_))
        ));
        assert!(matches!(systemd.start(ID), Err(ServiceError::Conflict(_))));
        assert!(matches!(systemd.stop(ID), Err(ServiceError::Conflict(_))));
        assert!(matches!(
            systemd.unregister(ID),
            Err(ServiceError::Conflict(_))
        ));
        assert!(matches!(systemd.status(ID), Err(ServiceError::Conflict(_))));
        assert_eq!(fake.verbs(), ["show", "show", "show", "show", "show"]);
        assert_eq!(fs::read(&file).unwrap(), render(NAME, &unit()));
    }

    // The same file reached through a symlinked directory is ours.
    let linked = temp.path().join("linked");
    std::os::unix::fs::symlink(temp.path().join("systemd/user"), &linked).unwrap();
    fake.set(|s| s.fragment = Some(linked.join(NAME).display().to_string()));
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Unchanged);
}

/// A unit that runs although systemd has no file for it (its file was removed under a
/// running service, or it never had one) is not ours to act on.
#[test]
fn a_unit_without_a_definition_file_that_still_runs_is_a_conflict() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    fake.set(|s| {
        s.search = None;
        s.pid = Some(7);
    });
    for call in [
        systemd.status(ID).map(drop),
        systemd.start(ID),
        systemd.stop(ID),
        systemd.unregister(ID),
        systemd.register(&unit()).map(drop),
    ] {
        assert!(matches!(call, Err(ServiceError::Conflict(_))));
    }
    assert_eq!(fake.verbs(), ["show"; 5]);
}

#[test]
fn a_definition_that_is_not_enabled_or_changed_on_disk_is_loaded_again_only_by_register() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    systemd.register(&unit()).unwrap();
    let file = temp.path().join(format!("systemd/user/{NAME}"));

    // Disabled and stopped behind our back: not loaded in the contract's sense.
    fake.set(|s| {
        s.enabled = false;
        s.pid = None;
    });
    assert_eq!(systemd.status(ID).unwrap(), ServiceStatus::Unloaded);
    fake.clear();
    assert!(matches!(systemd.start(ID), Err(ServiceError::Invalid(_))));
    assert_eq!(fake.verbs(), ["show"], "start neither enables nor reloads");
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Loaded);

    // The file changed on disk since systemd read it: start refuses it, register refuses
    // to take the changed file over, and nothing reloads or enables it.
    systemd.stop(ID).unwrap();
    assert_eq!(systemd.status(ID).unwrap(), ServiceStatus::Stopped);
    fs::write(&file, b"[Service]\nExecStart=/bin/replaced\n").unwrap();
    assert_eq!(systemd.status(ID).unwrap(), ServiceStatus::Unloaded);
    fake.clear();
    assert!(matches!(systemd.start(ID), Err(ServiceError::Invalid(_))));
    assert!(matches!(
        systemd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    assert_eq!(fake.verbs(), ["show", "show"]);

    // Someone made systemd read the changed file, and then our bytes are back: what
    // systemd holds is not what is on disk, so register reloads, enables and starts.
    fake.set(|s| s.read = Some(b"[Service]\nExecStart=/bin/replaced\n".to_vec()));
    fs::write(&file, render(NAME, &unit())).unwrap();
    assert_eq!(systemd.status(ID).unwrap(), ServiceStatus::Unloaded);
    fake.clear();
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Loaded);
    assert_eq!(fake.verbs(), ["show", "daemon-reload", "show", "enable"]);
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Unchanged);

    // A file systemd's manager does not read (another unit path) is reported, not enabled.
    let (elsewhere, fake) = self::systemd(&temp.path().join("other"));
    fake.set(|s| s.search = None);
    let error = elsewhere.register(&unit()).unwrap_err();
    assert!(
        matches!(&error, ServiceError::System(why) if why.contains("does not read")),
        "{error}"
    );
    assert!(!fake.verbs().contains(&"enable".to_string()));
    // The definition this call wrote is not left behind for a manager that never read it.
    assert_eq!(elsewhere.status(ID).unwrap(), ServiceStatus::NotRegistered);
    assert!(!temp
        .path()
        .join(format!("other/systemd/user/{NAME}"))
        .exists());

    // A file systemd read but cannot use is reported with its load state.
    let (broken, fake) = self::systemd(&temp.path().join("broken"));
    fake.set(|s| s.load = Some("bad-setting"));
    let error = broken.register(&unit()).unwrap_err();
    assert!(
        matches!(&error, ServiceError::System(why) if why.contains("bad-setting")),
        "{error}"
    );
    assert!(!fake.verbs().contains(&"enable".to_string()));
}

#[test]
fn publishing_never_replaces_a_definition_that_appeared_meanwhile() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, _fake) = systemd(temp.path());
    let file = temp.path().join(format!("systemd/user/{NAME}"));
    systemd.write_definition(&file, b"first").unwrap();
    assert!(matches!(
        systemd.write_definition(&file, b"second"),
        Err(ServiceError::Conflict(_))
    ));
    assert_eq!(fs::read(&file).unwrap(), b"first");
    let leftovers: Vec<_> = fs::read_dir(temp.path().join("systemd/user"))
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
fn stop_stops_only_a_live_unit_and_start_starts_only_a_stopped_one() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    assert!(matches!(systemd.start(ID), Err(ServiceError::Invalid(_))));
    assert_eq!(systemd.status(ID).unwrap(), ServiceStatus::NotRegistered);
    fake.clear();
    systemd.stop(ID).unwrap();
    assert_eq!(
        fake.verbs(),
        ["show"],
        "nothing registered, nothing stopped"
    );
    systemd.register(&unit()).unwrap();
    fake.clear();
    systemd.stop(ID).unwrap();
    assert_eq!(
        fake.calls()[1],
        ["systemctl", "--user", "stop", NAME].map(String::from)
    );
    assert_eq!(systemd.status(ID).unwrap(), ServiceStatus::Stopped);
    fake.clear();
    systemd.stop(ID).unwrap();
    assert_eq!(
        fake.verbs(),
        ["show"],
        "a stopped unit is not stopped again"
    );
    fake.clear();
    systemd.start(ID).unwrap();
    assert_eq!(
        fake.calls()[1],
        ["systemctl", "--user", "start", NAME].map(String::from)
    );
    assert_eq!(
        systemd.status(ID).unwrap(),
        ServiceStatus::Running { pid: 43 }
    );
}

#[test]
fn unregister_disables_stops_and_removes_the_definition_and_is_repeatable() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    systemd.register(&unit()).unwrap();
    fake.clear();
    systemd.unregister(ID).unwrap();
    assert_eq!(
        fake.verbs(),
        ["show", "disable", "daemon-reload", "reset-failed", "show"]
    );
    assert!(!temp.path().join(format!("systemd/user/{NAME}")).exists());
    assert_eq!(systemd.status(ID).unwrap(), ServiceStatus::NotRegistered);
    fake.clear();
    systemd.unregister(ID).unwrap();
    assert_eq!(fake.verbs(), ["show"]);
    assert!(matches!(
        systemd.status("../x"),
        Err(ServiceError::Invalid(_))
    ));
    // A definition systemd never read is removed without asking systemd to forget it.
    let (unread, fake) = self::systemd(&temp.path().join("unread"));
    let file = temp.path().join(format!("unread/systemd/user/{NAME}"));
    unread
        .write_definition(&file, &render(NAME, &unit()))
        .unwrap();
    fake.set(|s| s.search = None);
    unread.unregister(ID).unwrap();
    assert!(!file.exists());
    assert_eq!(fake.verbs(), ["show"]);
}

/// Without a user manager nothing is written and nothing is asked twice: the error names
/// what is missing and the foreground mode.
#[test]
fn without_a_user_manager_every_operation_is_unavailable_and_changes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    for (case, missing) in [
        ("no runtime directory", "XDG_RUNTIME_DIR"),
        ("no user bus", "Failed to connect to bus"),
        ("no systemctl", "systemd is not installed"),
    ] {
        let (systemd, fake) = systemd(&temp.path().join(case));
        fake.set(|s| match case {
            "no runtime directory" => s.no_runtime_dir = true,
            "no user bus" => s.no_bus = true,
            _ => s.missing = true,
        });
        for call in [
            systemd.available(),
            systemd.register(&unit()).map(drop),
            systemd.start(ID),
            systemd.stop(ID),
            systemd.status(ID).map(drop),
            systemd.unregister(ID),
        ] {
            let error = call.unwrap_err();
            assert!(
                matches!(&error, ServiceError::Unavailable(why) if why.contains(missing)),
                "{case}: {error}"
            );
            assert!(error.to_string().contains("foreground"), "{case}: {error}");
        }
        assert!(
            !temp.path().join(case).exists(),
            "{case}: something was written"
        );
        if case == "no runtime directory" {
            assert!(
                fake.calls().is_empty(),
                "nothing is run without a runtime directory"
            );
        }
    }
}

#[test]
fn lingering_is_read_from_logind_and_never_changed() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    assert_eq!(systemd.lingering(), Lingering::On);
    fake.set(|s| s.linger = Some("no"));
    let Lingering::Off { enable } = systemd.lingering() else {
        panic!("lingering is off");
    };
    assert_eq!(
        enable,
        format!("loginctl enable-linger {}", user_name(1000))
    );
    fake.set(|s| s.linger = Some("maybe"));
    assert!(matches!(systemd.lingering(), Lingering::Unknown(_)));
    fake.set(|s| s.linger = None);
    assert!(
        matches!(systemd.lingering(), Lingering::Unknown(why) if why.contains("Failed to get user"))
    );
    fake.set(|s| s.missing = true);
    assert!(matches!(systemd.lingering(), Lingering::Unknown(_)));
    assert_eq!(fake.verbs(), ["loginctl"; 5], "only show-user is ever run");
    assert!(fake.calls().iter().all(|c| c[1] == "show-user"));
    // The name of an account the database does not have is its number.
    assert_eq!(user_name(u32::MAX - 7), (u32::MAX - 7).to_string());
    assert_eq!(user_name(0), "root");
}

#[test]
fn show_output_is_read_by_key_and_an_incomplete_answer_is_not_a_state() {
    let shown = parse_show(
        "MainPID=77\nLoadState=loaded\nActiveState=active\nFragmentPath=/a b/c.service\n\
         UnitFileState=enabled\nNeedDaemonReload=no\n",
    )
    .unwrap();
    assert_eq!(shown.running(), Some(77));
    assert!(shown.settled());
    assert_eq!(shown.fragment, "/a b/c.service");
    let restarting =
        parse_show("LoadState=loaded\nActiveState=activating\nMainPID=0\nUnitFileState=enabled\n")
            .unwrap();
    assert_eq!(
        restarting.running(),
        None,
        "a scheduled restart has no process"
    );
    let unknown = parse_show("LoadState=not-found\nActiveState=inactive\nMainPID=0\n").unwrap();
    assert_eq!(
        (
            unknown.fragment.as_str(),
            unknown.running(),
            unknown.settled()
        ),
        ("", None, false)
    );
    assert_eq!(parse_show("ActiveState=active\n"), None);
    assert_eq!(parse_show(""), None);
    assert_eq!(
        property("LoadStateX=1\nLoadState=2\n", "LoadState"),
        Some("2")
    );
}

#[test]
fn the_user_placement_is_the_accounts_systemd_user_directory() {
    let p = user_placement(Path::new("/home/a")).unwrap();
    assert_eq!(p.directory, Path::new("/home/a/.config/systemd/user"));
    assert_eq!(p.prefix, "agentic-org-");
}

/// The real runner asks for the two programs by absolute path with an explicit
/// environment, and reports a missing one as that. Where this machine has `systemctl` but
/// no user manager (a container, a build sandbox), the adapter's answer is `Unavailable`.
#[test]
fn the_real_runner_answers_on_this_machine_without_changing_anything() {
    let temp = tempfile::tempdir().unwrap();
    let manager = registered(Placement {
        prefix: "agentic-org-test-".into(),
        directory: temp.path().join("systemd/user"),
    })
    .unwrap();
    let answer = manager.available();
    println!("systemd user manager on this machine: {answer:?}");
    match answer {
        Ok(()) => {}
        Err(ServiceError::Unavailable(why)) => {
            assert!(!why.is_empty());
            assert!(matches!(
                manager.status(ID),
                Err(ServiceError::Unavailable(_))
            ));
        }
        Err(other) => panic!("neither available nor unavailable: {other}"),
    }
    assert!(!temp.path().join("systemd").exists());
    println!("lingering on this machine: {:?}", manager.lingering());
}

/// Opt-in, against the real systemd user manager with a disposable unit name in the
/// account's own unit directory (systemd reads no other):
/// `AGENTIC_ORG_REAL_SYSTEMD=1 cargo test -p org-platform -- --ignored real_systemd --test-threads=1`.
/// The guard disables the unit and removes its definition even when an assertion fails.
#[test]
#[ignore = "registers a real systemd user unit"]
fn real_systemd() {
    if std::env::var_os("AGENTIC_ORG_REAL_SYSTEMD").is_none() {
        return;
    }
    struct Guard<'a>(&'a dyn ServiceManager, String);
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            let _ = self.0.unregister(&self.1);
        }
    }
    let wait_for = |manager: &dyn ServiceManager, id: &str, running: bool| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = manager.status(id).unwrap();
            if matches!(status, ServiceStatus::Running { .. }) == running {
                return status;
            }
            assert!(Instant::now() < deadline, "still {status:?} after 10 s");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
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
    let placement = Placement {
        prefix: "agentic-org-test-".into(),
        ..user_placement(&home).unwrap()
    };
    println!(
        "unit agentic-org-test-{id}.service in {}",
        placement.directory.display()
    );
    let manager = registered(placement.clone()).unwrap();
    manager.available().unwrap();
    println!("lingering: {:?}", manager.lingering());
    let _guard = Guard(manager.as_ref(), id.clone());
    // Exits 0 on SIGTERM, as the host does, so systemd does not restart it. It creates
    // `ready` once the trap is installed; the argument with a dollar sign, a percent sign
    // and a quote must arrive unchanged.
    let marker = tempfile::tempdir().unwrap();
    let ready = marker.path().join("ready");
    let odd = "$HOME %h \"q\" back\\slash";
    let unit = Unit {
        instance_id: id.clone(),
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "trap 'exit 0' TERM; printf '%s|%s' \"$1\" \"$ODD\" > \"$0\"; \
             while :; do /bin/sleep 1; done"
                .into(),
            ready.clone().into(),
            odd.into(),
        ],
        env: vec![("ODD".into(), odd.into())],
    };
    assert_eq!(manager.register(&unit).unwrap(), Registration::Loaded);
    println!("registered: {:?}", wait_for(manager.as_ref(), &id, true));
    assert_eq!(manager.register(&unit).unwrap(), Registration::Unchanged);
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read_to_string(&ready).map_or(true, |text| !text.contains('|')) {
        assert!(Instant::now() < deadline, "the unit never became ready");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(&ready).unwrap(),
        format!("{odd}|{odd}"),
        "an argument and an environment value arrive exactly as given"
    );
    manager.stop(&id).unwrap();
    println!("stopped: {:?}", wait_for(manager.as_ref(), &id, false));
    // A clean exit is not restarted: still stopped after systemd's restart delay.
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(manager.status(&id).unwrap(), ServiceStatus::Stopped);
    manager.start(&id).unwrap();
    println!("started: {:?}", wait_for(manager.as_ref(), &id, true));
    manager.unregister(&id).unwrap();
    assert_eq!(manager.status(&id).unwrap(), ServiceStatus::NotRegistered);
    assert!(!placement
        .directory
        .join(format!("agentic-org-test-{id}.service"))
        .exists());
}

/// X5 review V8: a drop-in of the unit's own can replace `ExecStart` or add to the
/// environment, so a unit with one is not ours to act on, on any operation; a drop-in that
/// systemd applies to every service is not the unit's own.
#[test]
fn a_unit_with_a_drop_in_of_its_own_is_a_conflict_for_every_operation() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    systemd.register(&unit()).unwrap();
    let file = temp.path().join(format!("systemd/user/{NAME}"));
    fake.set(|s| s.drop_ins = "/usr/lib/systemd/user/service.d/10-timeout.conf".into());
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Unchanged);
    for drop_ins in [
        format!("/home/a/.config/systemd/user/{NAME}.d/override.conf"),
        format!(
            "/usr/lib/systemd/user/service.d/10-timeout.conf /run/user/1000/systemd/user.control/{NAME}.d/50-x.conf"
        ),
    ] {
        fake.set(|s| s.drop_ins = drop_ins.clone());
        fake.clear();
        for call in [
            systemd.register(&unit()).map(drop),
            systemd.start(ID),
            systemd.stop(ID),
            systemd.status(ID).map(drop),
            systemd.unregister(ID),
        ] {
            let error = call.unwrap_err();
            assert!(
                matches!(&error, ServiceError::Conflict(why) if why.contains("drop-in")),
                "{error}"
            );
        }
        assert_eq!(fake.verbs(), ["show"; 5], "{drop_ins}");
        assert_eq!(fs::read(&file).unwrap(), render(NAME, &unit()));
    }
}

/// A registration whose load fails leaves no definition behind: the file this call wrote is
/// removed again, so the next call starts from nothing and not from a conflict.
#[test]
fn a_definition_whose_load_fails_is_not_left_behind() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    fake.set(|s| s.enable_fails = true);
    let error = systemd.register(&unit()).unwrap_err();
    assert!(
        matches!(&error, ServiceError::System(why) if why.contains("enable")),
        "{error}"
    );
    let file = temp.path().join(format!("systemd/user/{NAME}"));
    assert!(!file.exists(), "the definition was left behind");
    assert_eq!(
        fake.verbs(),
        ["show", "daemon-reload", "show", "enable", "daemon-reload"]
    );
    // The next registration writes and loads it as the first would have.
    fake.set(|s| {
        s.enable_fails = false;
        s.fragment = None;
        s.read = None;
    });
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Loaded);
    assert!(file.exists());
}
