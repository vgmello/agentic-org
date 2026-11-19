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
    /// `enable --now` enables the unit and then fails to start it, as when the unit's
    /// program cannot be started.
    enable_fails: bool,
    /// What a drop-in for every service changes: [`EFFECTIVE`] properties the manager
    /// reports with these values instead of the ones its file says.
    changed: Vec<(&'static str, String)>,
    /// `Some`: what `loginctl show-user` prints for `Linger`; `None`: it fails.
    linger: Option<&'static str>,
    /// No user manager answers: `systemctl` fails as it does without a user bus.
    no_bus: bool,
    /// The programs are not installed.
    missing: bool,
    no_runtime_dir: bool,
    /// What `show --property=Version` prints after `Version=` (`None`: 255).
    version: Option<&'static str>,
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
            // The enablement link stays, dangling, until a `disable` removes it.
            Err(_) if again && self.pid.is_none() => {
                self.fragment = None;
                self.read = None;
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
        let mut effective = self.read.as_deref().map(reported).unwrap_or_default();
        for (name, value) in &self.changed {
            effective.retain(|(key, _)| key != name);
            effective.push((name, value.clone()));
        }
        let effective: String = effective
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(name, value)| format!("{name}={value}\n"))
            .collect();
        format!(
            "LoadState={load}\nActiveState={}\nMainPID={}\nFragmentPath={fragment}\n\
             UnitFileState={}\nNeedDaemonReload={}\nDropInPaths={}\n{effective}",
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

/// What systemd 257 reports of a unit file for the [`EFFECTIVE`] properties, as
/// `systemctl show` prints them: written here from systemd's rules, not from the adapter's.
/// A property the file does not set has systemd's default.
fn reported(file: &[u8]) -> Vec<(&'static str, String)> {
    // systemd reading a quoted word of a unit file: escapes and `%%` undone, `$$` kept.
    fn words(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut word = String::new();
        let (mut quoted, mut chars) = (false, line.chars());
        while let Some(c) = chars.next() {
            match (c, quoted) {
                ('"', _) => quoted = !quoted,
                (' ', false) => words.push(std::mem::take(&mut word)),
                ('\\', true) => word.push(match chars.next().unwrap() {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    other => other,
                }),
                ('%', _) => word.push(chars.next().unwrap()),
                (c, _) => word.push(c),
            }
        }
        words.push(word);
        words
    }
    // systemd's `shell_maybe_quote`: a word with a space or a shell character is
    // double-quoted, with `\`, `"`, `$` and `` ` `` escaped.
    fn shell(word: &str) -> String {
        if !word.chars().any(|c| " \t\n\"\\`$*?['()<>|&;!".contains(c)) {
            return word.to_owned();
        }
        let mut out = String::from('"');
        for c in word.chars() {
            if "\"\\`$".contains(c) {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    }
    let text = String::from_utf8_lossy(file);
    let set = |key: &str| {
        text.lines()
            .filter_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .collect::<Vec<_>>()
    };
    let last = |key: &str, default: &str| set(key).last().copied().unwrap_or(default).to_owned();
    let exec = set("ExecStart")
        .last()
        .map(|line| {
            let argv = words(line);
            format!(
                "{{ path={} ; argv[]={} ; ignore_errors=no ; start_time=[n/a] ; \
                 stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}",
                argv[0],
                argv.join(" ")
            )
        })
        .unwrap_or_default();
    let environment: Vec<String> = set("Environment")
        .into_iter()
        .flat_map(words)
        .map(|word| shell(&word))
        .collect();
    vec![
        ("ExecStart", exec),
        ("Environment", environment.join(" ")),
        ("UnsetEnvironment", set("UnsetEnvironment").join(" ")),
        ("Type", last("Type", "simple")),
        ("KillMode", last("KillMode", "control-group")),
        ("KillSignal", "15".into()),
        ("SendSIGKILL", "yes".into()),
        (
            "TimeoutStopUSec",
            set("TimeoutStopSec")
                .last()
                .map_or("1min 30s".into(), |s| format!("{s}s")),
        ),
        ("Restart", last("Restart", "no")),
        ("UMask", last("UMask", "0022")),
        ("NoNewPrivileges", last("NoNewPrivileges", "no")),
    ]
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
            "show" if call[3] == "--property=Version" => {
                done(0, format!("Version={}\n", s.version.unwrap_or("255")), "")
            }
            // The manager's unit path: the directory it reads, among others it would read.
            "show" if call[3] == "--property=UnitPath" => {
                let read = s
                    .search
                    .clone()
                    .unwrap_or_else(|| "/elsewhere/systemd/user".into());
                done(
                    0,
                    format!(
                        "UnitPath=/run/user/1000/systemd/user.control {} /etc/systemd/user\n",
                        read.display()
                    ),
                    "",
                )
            }
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
                    // systemd links the unit into its target first, then starts it.
                    s.enabled = true;
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
                s.enabled = false;
                if call[3] == "--now" {
                    s.pid = None;
                }
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
         UnsetEnvironment=XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME\n\
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
/// command, a second setting, a specifier, a variable or a continuation line. (A control
/// character is refused before anything is rendered, `unit_file_safe`; the rendering
/// escapes it all the same.)
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
    assert!(validate(&hostile).is_ok() && unit_file_safe(&hostile).is_err());
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
            "UnsetEnvironment",
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
    assert!(validate(&hostile).is_ok());
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
        // What `systemctl show` would print raw, so that what systemd runs could not be
        // compared with the definition.
        Unit {
            env: vec![("A".into(), "x\ty".into())],
            ..unit()
        },
        Unit {
            env: vec![("A".into(), "x\ny".into())],
            ..unit()
        },
        Unit {
            args: vec!["two\nlines".into()],
            ..unit()
        },
        Unit {
            args: vec!["tab\there".into()],
            ..unit()
        },
        Unit {
            args: vec!["c1\u{85}".into()],
            ..unit()
        },
        Unit {
            program: "/opt/a\tb/agentic-org".into(),
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
                "show".into(),
                "--property=UnitPath".into(),
            ],
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
    let directory = systemd.directory(true).unwrap().unwrap();
    directory.publish(NAME, b"first", false).unwrap();
    assert!(matches!(
        directory.publish(NAME, b"second", false),
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
        .directory(true)
        .unwrap()
        .unwrap()
        .publish(NAME, &render(NAME, &unit()), false)
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
/// Where a user manager answers (a CI runner), it does not read this test's temporary
/// placement, and the adapter says so before writing anything.
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
        Err(ServiceError::System(why)) if why.contains("does not read unit files from") => {
            let placement = temp.path().join("systemd/user");
            assert!(why.contains(&placement.display().to_string()), "{why}");
            assert!(why.ends_with("nothing was changed"), "{why}");
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

/// X5 review V8: a drop-in that systemd applies to the unit by its name can replace
/// `ExecStart` or add to the environment, so a unit with one is not ours to act on, on any
/// operation. That is a drop-in of the unit's own and one inherited from a prefix of its
/// name that ends at a dash (systemd.unit(5)); a drop-in that systemd applies to every
/// service, or one of another unit whose name merely starts the same, is not.
#[test]
fn a_unit_with_a_drop_in_that_applies_by_its_name_is_a_conflict_for_every_operation() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    systemd.register(&unit()).unwrap();
    let file = temp.path().join(format!("systemd/user/{NAME}"));
    for drop_ins in [
        "/usr/lib/systemd/user/service.d/10-timeout.conf",
        "/home/a/.config/systemd/user/agentic-orgx-.service.d/x.conf",
        "/home/a/.config/systemd/user/agentic-org-test-01965d2e.service.d/x.conf",
        "/home/a/.config/systemd/user/agentic-org-test-01965d2e-8b1c-7c3a-9f00-0123456789ab.service.d.conf",
    ] {
        fake.set(|s| s.drop_ins = drop_ins.into());
        assert_eq!(
            systemd.register(&unit()).unwrap(),
            Registration::Unchanged,
            "{drop_ins}"
        );
    }
    for drop_ins in [
        format!("/home/a/.config/systemd/user/{NAME}.d/override.conf"),
        format!(
            "/usr/lib/systemd/user/service.d/10-timeout.conf /run/user/1000/systemd/user.control/{NAME}.d/50-x.conf"
        ),
        "/home/a/.config/systemd/user/agentic-org-.service.d/override.conf".into(),
        "/etc/systemd/user/agentic-.service.d/override.conf".into(),
        "/home/a/.config/systemd/user/agentic-org-test-.service.d/override.conf".into(),
        "/home/a/.config/systemd/user/agentic-org-test-01965d2e-8b1c-7c3a-9f00-.service.d/x.conf"
            .into(),
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
        [
            "show",
            "show",
            "daemon-reload",
            "show",
            "enable",
            "disable",
            "daemon-reload",
            "reset-failed"
        ]
    );
    // X5 review (third round): `enable --now` linked the unit into its target before its
    // start failed; that link went with the definition, and no restart is scheduled.
    assert!(
        !fake.0.lock().unwrap().enabled,
        "an enablement link was left behind"
    );
    assert!(fake.calls().contains(
        &["systemctl", "--user", "disable", "--now", NAME]
            .map(String::from)
            .to_vec()
    ));
    // The next registration writes and loads it as the first would have.
    fake.set(|s| {
        s.enable_fails = false;
        s.fragment = None;
        s.read = None;
    });
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Loaded);
    assert!(file.exists());
}

/// The unit of the comparison fixture below, with every character `systemctl show` quotes
/// or escapes.
fn odd_unit() -> Unit {
    Unit {
        args: vec![
            "serve".into(),
            "--name".into(),
            "a&b<c>".into(),
            "$HOME %h \"q\" back\\slash".into(),
        ],
        env: vec![
            ("HOME".into(), "/home/a b".into()),
            ("XDG_DATA_HOME".into(), "/x/$y \"z\" back\\slash é".into()),
        ],
        ..unit()
    }
}

/// What systemd 257 printed for [`odd_unit`]'s rendering (`systemctl show` of the
/// properties the adapter asks for, on Debian 13), with nothing changing it.
const ODD_SHOWN: &str = "Type=exec\nRestart=on-failure\nTimeoutStopUSec=15s\nMainPID=0\n\
ExecStart={ path=/opt/agentic org/agentic-org ; argv[]=/opt/agentic org/agentic-org serve --name \
a&b<c> $$HOME %h \"q\" back\\slash ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; \
pid=0 ; code=(null) ; status=0/0 }\n\
Environment=\"HOME=/home/a b\" \"XDG_DATA_HOME=/x/\\$y \\\"z\\\" back\\\\slash é\"\n\
PassEnvironment=\nUnsetEnvironment=XDG_CACHE_HOME XDG_STATE_HOME\nUMask=0077\n\
NoNewPrivileges=yes\nKillMode=mixed\nKillSignal=15\nSendSIGKILL=yes\nLoadState=loaded\nActiveState=inactive\n\
FragmentPath=/etc/systemd/system/agentic-org-test-01965d2e-8b1c-7c3a-9f00-0123456789ab.service\n\
DropInPaths=\nUnitFileState=disabled\nNeedDaemonReload=no\n";

/// X5 review (third round): what systemd itself reports for a rendered unit matches it,
/// quoting and all, and each property a drop-in for every service changed is found. The
/// changed lines are what systemd 257 printed with such a drop-in
/// (`service.d/50-x.conf`: `Environment=XDG_CACHE_HOME=/elsewhere`, `ExecStart=` reset to
/// `/bin/sh -c true`, `KillMode=process`).
#[test]
fn what_systemd_reports_is_compared_with_the_definition_property_by_property() {
    let unit = odd_unit();
    let shown = parse_show(ODD_SHOWN).unwrap();
    assert_eq!(effective_differs(&unit, &shown), None);
    // The definition read back from the file is the unit, and so compares the same.
    let defined = definition(NAME, &render(NAME, &unit)).unwrap();
    assert_eq!(
        (&defined.program, &defined.args, &defined.env),
        (&unit.program, &unit.args, &unit.env)
    );
    assert_eq!(effective_differs(&defined, &shown), None);
    let changed = |from: &str, to: &str| {
        assert!(ODD_SHOWN.contains(from), "{from}");
        parse_show(&ODD_SHOWN.replacen(from, to, 1)).unwrap()
    };
    let exec = ODD_SHOWN
        .lines()
        .find(|l| l.starts_with("ExecStart="))
        .unwrap();
    let environment = ODD_SHOWN
        .lines()
        .find(|l| l.starts_with("Environment="))
        .unwrap();
    for (from, to, property) in [
        (
            exec,
            "ExecStart={ path=/bin/sh ; argv[]=/bin/sh -c true ; ignore_errors=no ; \
             start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }",
            "ExecStart",
        ),
        // A second command appended to the first (a drop-in without the reset).
        (
            " status=0/0 }",
            " status=0/0 } { path=/bin/sh ; argv[]=/bin/sh ; ignore_errors=no ; \
             start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }",
            "ExecStart",
        ),
        (" serve --name", " serve --name x", "ExecStart"),
        // An argument `;` and another word after ours, as systemd 257 printed it for a
        // drop-in that reset ExecStart= to our command followed by `\; x`.
        (
            "back\\slash ; ignore_errors=no",
            "back\\slash ; x ; ignore_errors=no",
            "ExecStart",
        ),
        (" ignore_errors=no ;", " ignore_errors=yes ;", "ExecStart"),
        // The same, with words that copy the field that follows the arguments.
        (
            "back\\slash ; ignore_errors=no",
            "back\\slash ; ignore_errors=no ; x ; ignore_errors=no",
            "ExecStart",
        ),
        (
            environment,
            "Environment=\"HOME=/home/a b\" \"XDG_DATA_HOME=/x/\\$y \\\"z\\\" \
             back\\\\slash é\" XDG_CACHE_HOME=/elsewhere",
            "Environment",
        ),
        ("HOME=/home/a b", "HOME=/home/a", "Environment"),
        (
            "\nPassEnvironment=\n",
            "\nPassEnvironment=XDG_DATA_HOME\n",
            "PassEnvironment",
        ),
        (
            "UnsetEnvironment=XDG_CACHE_HOME XDG_STATE_HOME",
            "UnsetEnvironment=XDG_CACHE_HOME",
            "UnsetEnvironment",
        ),
        ("KillMode=mixed", "KillMode=process", "KillMode"),
        ("KillSignal=15", "KillSignal=9", "KillSignal"),
        ("SendSIGKILL=yes", "SendSIGKILL=no", "SendSIGKILL"),
        ("Type=exec", "Type=simple", "Type"),
        (
            "TimeoutStopUSec=15s",
            "TimeoutStopUSec=1min 30s",
            "TimeoutStopUSec",
        ),
        ("Restart=on-failure", "Restart=always", "Restart"),
        ("UMask=0077", "UMask=0022", "UMask"),
        (
            "NoNewPrivileges=yes",
            "NoNewPrivileges=no",
            "NoNewPrivileges",
        ),
        (
            "MainPID=0\n",
            "MainPID=0\nExecStartPre={ path=/bin/true ; argv[]=/bin/true ; ignore_errors=no \
             ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n",
            "ExecStartPre",
        ),
        (
            "MainPID=0\n",
            "MainPID=0\nExecStartPost={ path=/bin/true ; argv[]=/bin/true ; ignore_errors=no \
             ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n",
            "ExecStartPost",
        ),
        (
            "MainPID=0\n",
            "MainPID=0\nEnvironmentFiles=/etc/foo (ignore_errors=yes)\n",
            "EnvironmentFiles",
        ),
        (
            "MainPID=0\n",
            "MainPID=0\nExecStop={ path=/bin/true ; argv[]=/bin/true ; ignore_errors=no ; \
             start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n",
            "ExecStop",
        ),
        (
            "MainPID=0\n",
            "MainPID=0\nExecStopPost={ path=/bin/true ; argv[]=/bin/true ; ignore_errors=no ; \
             start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n",
            "ExecStopPost",
        ),
        (
            "MainPID=0\n",
            "MainPID=0\nExecCondition={ path=/bin/true ; argv[]=/bin/true ; ignore_errors=no \
             ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n",
            "ExecCondition",
        ),
    ] {
        assert_eq!(
            effective_differs(&unit, &changed(from, to)),
            Some(property),
            "{to}"
        );
    }
    // A property systemd did not print at all is not what the definition says either.
    assert_eq!(
        effective_differs(&unit, &changed("KillMode=mixed\n", "")),
        Some("KillMode")
    );
}

#[test]
fn the_words_systemctl_prints_are_read_with_its_quotes() {
    assert_eq!(
        shell_words(r#"A=plain "B=x\$y" "C=x\\y" "D=q's" E=é "F=a;b" G= "H=*?[" I=~x "#),
        Some(
            ["A=plain", "B=x$y", "C=x\\y", "D=q's", "E=é", "F=a;b", "G=", "H=*?[", "I=~x"]
                .map(String::from)
                .to_vec()
        )
    );
    assert_eq!(shell_words(""), Some(vec![]));
    assert_eq!(shell_words(r#""""#), Some(vec![String::new()]));
    for unreadable in [r#""open"#, r#""\t""#, r#"a\b"#] {
        assert_eq!(shell_words(unreadable), None, "{unreadable}");
    }
}

/// Only a file that is exactly a rendering of a unit is read back as one: anything else is
/// not a definition this adapter wrote. (A file that renders another unit, one more
/// argument for one, is read back as that unit, and what systemd runs is compared with it.)
#[test]
fn a_definition_is_read_back_only_from_its_exact_rendering() {
    let unit = odd_unit();
    let bytes = render(NAME, &unit);
    assert!(definition(NAME, &bytes).is_some());
    let text = String::from_utf8(bytes).unwrap();
    for changed in [
        text.replace("KillMode=mixed", "KillMode=process"),
        text.replace("UnsetEnvironment=XDG_CACHE_HOME XDG_STATE_HOME\n", ""),
        format!("{text}[Service]\nExecStartPre=/bin/true\n"),
        text.replace("%%h", "%h"),
        "[Service]\nExecStart=/bin/someone-elses\n".into(),
    ] {
        assert!(definition(NAME, changed.as_bytes()).is_none(), "{changed}");
    }
    assert!(definition("agentic-org-test-other.service", text.as_bytes()).is_none());
}

/// X5 review (third round), P2: the service resolves the instance's locations from what
/// setup gave it and nothing else. A location variable the unit does not set is unset, so
/// the user manager's own value does not reach the service; one it sets is not unset.
#[test]
fn the_location_variables_the_unit_does_not_set_are_unset() {
    let line = |unit: &Unit| {
        rendered(unit)
            .lines()
            .find(|l| l.starts_with("UnsetEnvironment="))
            .map(str::to_owned)
    };
    assert_eq!(
        line(&unit()).as_deref(),
        Some("UnsetEnvironment=XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME")
    );
    let some = Unit {
        env: vec![
            ("HOME".into(), "/home/a".into()),
            ("XDG_CACHE_HOME".into(), "/c".into()),
        ],
        ..unit()
    };
    assert_eq!(
        line(&some).as_deref(),
        Some("UnsetEnvironment=XDG_DATA_HOME XDG_STATE_HOME")
    );
    let all = Unit {
        env: ["XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"]
            .map(|name| (name.into(), "/x".into()))
            .to_vec(),
        ..unit()
    };
    assert_eq!(line(&all), None);
    for name in org_platform_location_variables() {
        assert!(rendered(&unit()).contains(name), "{name} is not unset");
    }
}

fn org_platform_location_variables() -> &'static [&'static str] {
    crate::dirs::location_variables().unwrap()
}

/// X5 review (third round), P2: a drop-in for every service of the user is accepted
/// (distributions ship some), but one that changes what systemd would run under our name
/// (its command, its environment, how it is stopped) is a conflict for register, start and
/// status. Stop and unregister still end and remove our own unit.
#[test]
fn a_drop_in_for_every_service_that_changes_what_runs_is_a_conflict_for_what_runs_it() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    systemd.register(&unit()).unwrap();
    let file = temp.path().join(format!("systemd/user/{NAME}"));
    // One that changes nothing the service depends on.
    fake.set(|s| s.drop_ins = "/usr/lib/systemd/user/service.d/10-timeout-abort.conf".into());
    assert_eq!(systemd.register(&unit()).unwrap(), Registration::Unchanged);
    assert_eq!(
        systemd.status(ID).unwrap(),
        ServiceStatus::Running { pid: 42 }
    );
    for (property, value) in [
        (
            "ExecStart",
            "{ path=/bin/sh ; argv[]=/bin/sh -c true ; ignore_errors=no ; start_time=[n/a] ; \
             stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }",
        ),
        ("Environment", "HOME=/home/a XDG_DATA_HOME=/elsewhere"),
        ("UnsetEnvironment", ""),
        ("KillMode", "process"),
        ("KillSignal", "9"),
        ("EnvironmentFiles", "/etc/x (ignore_errors=no)"),
    ] {
        fake.set(|s| {
            s.drop_ins = "/home/a/.config/systemd/user/service.d/50-x.conf".into();
            s.changed = vec![(property, value.into())];
        });
        fake.clear();
        for call in [
            systemd.register(&unit()).map(drop),
            systemd.start(ID),
            systemd.status(ID).map(drop),
        ] {
            let error = call.unwrap_err();
            assert!(
                matches!(&error, ServiceError::Conflict(why)
                    if why.contains(property) && why.contains("50-x.conf")),
                "{property}: {error}"
            );
        }
        assert_eq!(fake.verbs(), ["show"; 3], "{property}");
        assert_eq!(fs::read(&file).unwrap(), render(NAME, &unit()));
    }
    // Ending our own unit is not refused: that would only leave it running.
    systemd.stop(ID).unwrap();
    assert_eq!(fake.0.lock().unwrap().pid, None, "the unit was not stopped");
    systemd.unregister(ID).unwrap();
    assert!(!file.exists());
    // A first registration that such a drop-in would change is not enabled, and what it
    // wrote is removed again.
    let (fresh, fake) = self::systemd(&temp.path().join("fresh"));
    fake.set(|s| s.changed = vec![("KillMode", "process".into())]);
    let error = fresh.register(&unit()).unwrap_err();
    assert!(
        matches!(&error, ServiceError::Conflict(why) if why.contains("KillMode")),
        "{error}"
    );
    assert!(!fake.verbs().contains(&"enable".to_string()));
    assert!(!temp
        .path()
        .join(format!("fresh/systemd/user/{NAME}"))
        .exists());
}

/// X5 final review: a user manager older than systemd 240 does not know `Type=exec`, ignores
/// it and reports `simple` with no drop-in. It is named as too old, before anything is
/// written (`available`), and if it is found later (register, start, status of a unit it
/// loaded), not as a drop-in that changes the type. Version strings as distributions print
/// them; one that is not a number is not refused by its version.
#[test]
fn a_systemd_too_old_for_the_unit_is_named_as_that() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    for version in [
        "255",
        "240",
        "252 (252.22-1~deb12u1)",
        "249.11-0ubuntu3.12",
        "v999",
    ] {
        fake.set(|s| s.version = Some(version));
        assert!(systemd.available().is_ok(), "{version}");
    }
    for version in ["239 (239-78.el8)", "237", " 219"] {
        fake.set(|s| s.version = Some(version));
        let error = systemd.available().unwrap_err();
        assert!(
            matches!(&error, ServiceError::Unavailable(why)
                if why.contains("needs 240 or later") && why.contains("Type=exec")),
            "{version}: {error}"
        );
    }
    assert!(!temp.path().join("systemd/user").exists());
    // Found once it holds the unit: the type it reports is the old manager's, not a drop-in's.
    fake.set(|s| s.version = None);
    systemd.register(&unit()).unwrap();
    fake.set(|s| {
        s.version = Some("239 (239-78.el8)");
        s.changed = vec![("Type", "simple".into())];
    });
    for call in [
        systemd.register(&unit()).map(drop),
        systemd.start(ID),
        systemd.status(ID).map(drop),
    ] {
        let error = call.unwrap_err();
        assert!(
            matches!(&error, ServiceError::Unavailable(why) if why.contains("version 239")),
            "{error}"
        );
    }
    // A recent manager with a drop-in that changes the type is still that.
    fake.set(|s| {
        s.version = None;
        s.drop_ins = "/home/a/.config/systemd/user/service.d/50-x.conf".into();
    });
    let error = systemd.status(ID).unwrap_err();
    assert!(
        matches!(&error, ServiceError::Conflict(why) if why.contains("Type") && why.contains("50-x.conf")),
        "{error}"
    );
}

/// X5 review (third round): a loaded file that is not exactly a rendering of a unit (one
/// written by an earlier build, before the unit unset the location variables, or edited by
/// hand) is not run on: register, start and status are conflicts. It can still be stopped
/// and unregistered, which removes it.
#[test]
fn a_definition_that_is_no_rendering_is_not_run_but_can_be_stopped_and_unregistered() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    systemd.register(&unit()).unwrap();
    let file = temp.path().join(format!("systemd/user/{NAME}"));
    let earlier = String::from_utf8(render(NAME, &unit())).unwrap().replace(
        "UnsetEnvironment=XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME\n",
        "",
    );
    fs::write(&file, &earlier).unwrap();
    // systemd read that file: what it holds is the file on disk.
    fake.set(|s| s.read = Some(earlier.clone().into_bytes()));
    for call in [
        systemd.register(&unit()).map(drop),
        systemd.start(ID),
        systemd.status(ID).map(drop),
    ] {
        assert!(matches!(call, Err(ServiceError::Conflict(_))), "{call:?}");
    }
    systemd.stop(ID).unwrap();
    assert_eq!(fake.0.lock().unwrap().pid, None);
    systemd.unregister(ID).unwrap();
    assert!(!file.exists());
    assert_eq!(systemd.status(ID).unwrap(), ServiceStatus::NotRegistered);
}

/// X5 review (third round): a unit that runs although it is not enabled (disabled by hand)
/// keeps running when register's `enable --now` fails: the rollback takes back only the
/// enablement that call made.
#[test]
fn a_failed_enable_of_a_running_unit_takes_back_the_link_and_leaves_it_running() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    systemd.register(&unit()).unwrap();
    fake.set(|s| {
        s.enabled = false;
        s.enable_fails = true;
    });
    fake.clear();
    assert!(systemd.register(&unit()).is_err());
    assert!(fake.calls().contains(
        &["systemctl", "--user", "disable", NAME]
            .map(String::from)
            .to_vec()
    ));
    let state = fake.0.lock().unwrap();
    assert!(!state.enabled, "the link this call made was left");
    assert_eq!(state.pid, Some(42), "a running unit was stopped");
}

/// X5 review (fourth round, P2-4): a user manager that reads its units from another
/// directory (its `XDG_CONFIG_HOME` names another folder) is refused before anything is
/// written, by `available` (which setup asks before it reserves anything) and by `register`.
#[test]
fn a_manager_that_does_not_read_the_placement_is_refused_before_anything_is_written() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    fake.set(|s| s.search = Some("/home/a/elsewhere/systemd/user".into()));
    for call in [systemd.available(), systemd.register(&unit()).map(drop)] {
        let error = call.unwrap_err();
        assert!(
            matches!(&error, ServiceError::System(why) if why.contains("does not read unit files from")
                && why.contains("/home/a/elsewhere/systemd/user")),
            "{error}"
        );
    }
    assert!(
        !temp.path().join("systemd").exists(),
        "something was written"
    );
    for verb in ["daemon-reload", "enable"] {
        assert!(!fake.verbs().contains(&verb.to_string()), "{verb} ran");
    }
    // The directory it reads, named through a link, is the placement's.
    let linked = temp.path().join("linked");
    fs::create_dir_all(temp.path().join("systemd/user")).unwrap();
    std::os::unix::fs::symlink(temp.path().join("systemd"), &linked).unwrap();
    fake.set(|s| s.search = Some(linked.join("user")));
    assert!(systemd.available().is_ok());
}

/// X5 review V-b: a definition that is exactly an earlier rendering of a unit for the same
/// instance (another program or environment, as when setup runs again with what the service
/// lacked) is replaced: what systemd runs from it is disabled and stopped first, the new
/// rendering is published and loaded. A file that is no rendering is still never taken over.
#[test]
fn an_earlier_rendering_for_the_same_instance_is_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    systemd.register(&unit()).unwrap();
    let file = temp.path().join(format!("systemd/user/{NAME}"));
    let fixed = Unit {
        env: vec![
            ("HOME".into(), "/home/a".into()),
            ("ORG_OFFICE_DEV_KEY_DIR".into(), "/home/a/keys".into()),
        ],
        ..unit()
    };
    fake.clear();
    assert_eq!(systemd.register(&fixed).unwrap(), Registration::Loaded);
    assert_eq!(fs::read(&file).unwrap(), render(NAME, &fixed));
    assert_eq!(
        fake.verbs(),
        ["show", "show", "disable", "daemon-reload", "show", "enable"]
    );
    assert!(fake.calls().contains(
        &["systemctl", "--user", "disable", "--now", NAME]
            .map(String::from)
            .to_vec()
    ));
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
        "a temporary file remains: {leftovers:?}"
    );
    // The same rendering again is left alone.
    assert_eq!(systemd.register(&fixed).unwrap(), Registration::Unchanged);
    // A file that is not a rendering (edited by hand) is not replaced.
    let edited = String::from_utf8(render(NAME, &fixed))
        .unwrap()
        .replace("Restart=on-failure", "Restart=always");
    fs::write(&file, &edited).unwrap();
    fake.set(|s| s.read = Some(edited.clone().into_bytes()));
    fake.clear();
    assert!(matches!(
        systemd.register(&unit()),
        Err(ServiceError::Conflict(_))
    ));
    assert_eq!(fs::read_to_string(&file).unwrap(), edited);
    assert!(!fake.verbs().contains(&"disable".to_string()));
}

/// X5 review (fourth round, P2-5): the definitions are written only in a directory of this
/// user that other users cannot write to, relative to that directory, and the mode is set
/// on the open file.
#[test]
fn a_unit_directory_other_users_can_write_to_holds_no_definition() {
    let temp = tempfile::tempdir().unwrap();
    let (systemd, fake) = systemd(temp.path());
    let directory = temp.path().join("systemd/user");
    fs::create_dir_all(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o777)).unwrap();
    let error = systemd.register(&unit()).unwrap_err();
    assert!(
        matches!(&error, ServiceError::Conflict(why) if why.contains("writable by other users")),
        "{error}"
    );
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
    assert!(!fake.verbs().contains(&"enable".to_string()));
    // A directory setup makes is private, whatever the process's mask.
    fs::remove_dir(&directory).unwrap();
    systemd.register(&unit()).unwrap();
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
