//! `agentic-org setup` and `service` through the Linux service adapter itself: the real
//! binary composes `org_platform::service`'s systemd adapter, whose runner executes
//! `systemctl` and `loginctl` stand-ins from a folder of the test
//! (`ORG_DEV_SERVICE_PROGRAMS`). The stand-in reads the unit file the adapter wrote and
//! runs its `ExecStart=`, so the service that answers setup is started from the rendered
//! unit. systemd itself is never touched; on a system whose adapter runs no such programs
//! these tests have nothing to exercise and say so.
use super::*;

const SYSTEMCTL: &str = include_str!("systemd/systemctl.sh");
const LOGINCTL: &str = include_str!("systemd/loginctl.sh");

/// An account whose service adapter is the system's own, with the stand-in programs.
/// `None` where this system's adapter cannot be pointed at stand-in programs.
fn account() -> Option<Account> {
    let mut account = Account::new();
    let programs = account.base.join("programs");
    for dir in ["programs", "run"] {
        std::fs::create_dir(account.base.join(dir)).unwrap();
    }
    let units = match org_platform::service::Placement::user(&account.base) {
        Ok(placement) => placement.directory,
        Err(refused) => {
            println!("skipped: {refused}");
            return None;
        }
    };
    if let Err(refused) = org_platform::service::testing::registered_with_programs(
        org_platform::service::Placement {
            prefix: "probe-".into(),
            directory: units.clone(),
        },
        &programs,
        None,
    ) {
        println!("skipped: {refused}");
        return None;
    }
    for (name, text) in [("systemctl", SYSTEMCTL), ("loginctl", LOGINCTL)] {
        std::fs::write(programs.join(name), text).unwrap();
        std::fs::set_permissions(programs.join(name), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    std::fs::write(programs.join("unit-directory"), units.display().to_string()).unwrap();
    std::fs::write(programs.join("linger"), "no").unwrap();
    account.programs = Some(programs);
    Some(account)
}

fn calls(account: &Account) -> Vec<String> {
    let programs = account.programs.as_ref().unwrap();
    std::fs::read_to_string(programs.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn service_pid(account: &Account) -> Option<u32> {
    let programs = account.programs.as_ref().unwrap();
    std::fs::read_to_string(programs.join("pid"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// SRV-2, SRV-6 and the linger report: setup writes the unit, enables and starts it once,
/// waits for the instance the unit started, and says that the service stops at logout. A
/// second run changes nothing; `service` stops, shows and starts the same unit.
#[test]
fn setup_registers_a_systemd_user_unit_starts_it_once_and_reports_lingering() {
    let Some(account) = account() else { return };
    let port = free_port().to_string();
    let (ok, out, err) = account.run_in_terminal(&["setup", "--port", &port]);
    assert!(ok, "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("Service registered."), "{out}");
    assert!(out.contains("Service running."), "{out}");
    assert!(
        out.contains("To keep it running without a login session, run: loginctl enable-linger "),
        "{out}"
    );
    assert!(link_token(&out).is_some(), "{out}");

    let id = account.record("default")["instance_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let name = format!("agentic-org-{id}.service");
    let file = account.base.join(".config/systemd/user").join(&name);
    let unit = std::fs::read_to_string(&file).unwrap();
    assert!(
        unit.contains(&format!(
            "ExecStart=\"{}\" \"serve\" \"--name\" \"default\"\n",
            account.bin.display()
        )),
        "{unit}"
    );
    for setting in [
        "Type=exec",
        "KillMode=mixed",
        "TimeoutStopSec=15",
        "Restart=on-failure",
        "UMask=0077",
        "NoNewPrivileges=yes",
        "WantedBy=default.target",
    ] {
        assert!(
            unit.lines().any(|line| line == setting),
            "{setting}: {unit}"
        );
    }
    assert!(
        unit.contains(&format!(
            "Environment=\"HOME={}\"\n",
            account.base.display()
        )),
        "{unit}"
    );
    // The link never reaches the unit or the service's output.
    let token = link_token(&out).unwrap();
    assert!(!unit.contains(&token));
    let log = std::fs::read_to_string(account.programs.as_ref().unwrap().join("service.log"))
        .unwrap_or_default();
    assert!(!log.contains(&token), "{log}");

    // What was asked of systemd, in order: whether it answers and reads the unit
    // directory, the unit's state, the unit directory again before the file is written, a
    // reload, the state again, enable and start; then only questions. Nothing ran
    // `enable-linger`.
    let first = calls(&account);
    let verbs: Vec<&str> = first
        .iter()
        .map(|call| call.split(' ').nth(2).unwrap_or(""))
        .collect();
    assert_eq!(
        verbs[..7],
        [
            "show",
            "show",
            "show",
            "show",
            "daemon-reload",
            "show",
            "enable"
        ],
        "{first:#?}"
    );
    assert_eq!(first[0], "systemctl --user show --property=Version");
    assert_eq!(first[1], "systemctl --user show --property=UnitPath");
    assert_eq!(first[3], "systemctl --user show --property=UnitPath");
    assert_eq!(first[6], format!("systemctl --user enable --now {name}"));
    assert!(
        first.iter().all(|call| !call.contains("enable-linger")),
        "{first:#?}"
    );
    let pid = service_pid(&account).expect("the unit started a process");
    assert!(alive(pid));

    // A second run: the same instance and process, nothing reloaded, enabled or started.
    let second = account.run(&["setup"]);
    let (out2, err2) = text(&second);
    assert!(second.status.success(), "stdout:\n{out2}\nstderr:\n{err2}");
    assert_eq!(
        service_pid(&account),
        Some(pid),
        "the service was restarted"
    );
    let later = calls(&account)[first.len()..].to_vec();
    assert!(
        later
            .iter()
            .all(|call| call.starts_with("systemctl --user show ")
                || call.starts_with("loginctl show-user ")),
        "{later:#?}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), unit);

    // service status, stop and start go through the same adapter.
    let status = account.run(&["service", "status"]);
    let (shown, _) = text(&status);
    assert!(shown.contains(&format!("running (pid {pid})")), "{shown}");
    assert!(shown.contains("loginctl enable-linger "), "{shown}");
    let stop = account.run(&["service", "stop", "--yes"]);
    assert!(stop.status.success(), "{:?}", text(&stop));
    assert!(!alive(pid), "the unit's process outlived its stop");
    assert!(calls(&account).contains(&format!("systemctl --user stop {name}")));
    let status = account.run(&["service", "status"]);
    assert!(
        text(&status).0.starts_with("stopped\n"),
        "{:?}",
        text(&status)
    );
    let start = account.run(&["service", "start"]);
    assert!(start.status.success(), "{:?}", text(&start));
    assert!(calls(&account).contains(&format!("systemctl --user start {name}")));
    let restarted = service_pid(&account).expect("started again");
    assert!(alive(restarted) && restarted != pid);

    // With lingering on, nothing is said about it.
    std::fs::write(account.programs.as_ref().unwrap().join("linger"), "yes").unwrap();
    let status = account.run(&["service", "status"]);
    assert_eq!(
        text(&status).0,
        format!("running (pid {restarted})\n"),
        "stderr: {}",
        text(&status).1
    );
    let stop = account.run(&["service", "stop", "--yes"]);
    assert!(stop.status.success(), "{:?}", text(&stop));
}

/// SRV-34: on a machine or in a container without a systemd user manager, setup refuses
/// before it saves anything and names the foreground mode.
#[test]
fn without_a_user_manager_setup_names_the_foreground_mode_and_saves_nothing() {
    for case in ["no user bus", "no runtime directory", "no systemctl"] {
        let Some(account) = account() else { return };
        let programs = account.programs.clone().unwrap();
        let mut command = account.command(&["setup", "--port", &free_port().to_string()]);
        let expected = match case {
            "no user bus" => {
                std::fs::write(programs.join("no-bus"), "").unwrap();
                "Failed to connect to bus"
            }
            "no runtime directory" => {
                command.env_remove("XDG_RUNTIME_DIR");
                "XDG_RUNTIME_DIR is not set"
            }
            _ => {
                std::fs::remove_file(programs.join("systemctl")).unwrap();
                "systemd is not installed"
            }
        };
        let refused = output(&mut command);
        let (out, err) = text(&refused);
        assert!(!refused.status.success(), "{case}: {out}");
        assert!(err.contains(expected), "{case}: {err}");
        assert!(
            err.contains("\"service\": \"foreground\"")
                && err.contains("agentic-org serve --setup")
                && err.contains("nothing was changed"),
            "{case}: {err}"
        );
        assert!(
            !account.base.join("app/setup").exists(),
            "{case}: a setup record was saved"
        );
        assert!(
            !account.base.join(".config").exists(),
            "{case}: a unit was written"
        );
        assert_eq!(service_pid(&account), None, "{case}");
    }
}

/// X5 review V7: the unit carries the variables that decided the instance's locations when
/// setup ran (each of the three XDG base directories, when it is set to an absolute path),
/// so that the service, which the user manager starts with its own environment, resolves
/// the same paths. A relative value, which the path rule ignores, is not carried, and the
/// variables not carried are unset in the unit.
#[test]
fn the_unit_carries_the_variables_that_decided_the_instances_paths() {
    for (variable, others) in [
        ("XDG_DATA_HOME", ["XDG_CACHE_HOME", "XDG_STATE_HOME"]),
        ("XDG_CACHE_HOME", ["XDG_DATA_HOME", "XDG_STATE_HOME"]),
        ("XDG_STATE_HOME", ["XDG_DATA_HOME", "XDG_CACHE_HOME"]),
    ] {
        let Some(account) = account() else { return };
        let place = account.base.join("xdg");
        let mut command = account.command(&["setup", "--port", &free_port().to_string()]);
        command
            .env(variable, &place)
            .env(others[0], "relative/place");
        let ran = output(&mut command);
        assert!(ran.status.success(), "{variable}: {:?}", text(&ran));
        let id = account.record("default")["instance_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let unit = std::fs::read_to_string(
            account
                .base
                .join(".config/systemd/user")
                .join(format!("agentic-org-{id}.service")),
        )
        .unwrap();
        assert!(
            unit.contains(&format!("Environment=\"{variable}={}\"\n", place.display())),
            "{variable}: {unit}"
        );
        // The others are not carried, and are unset: the user manager's own values never
        // reach the service (X5 review, third round).
        let unset = unit
            .lines()
            .find_map(|line| line.strip_prefix("UnsetEnvironment="))
            .unwrap_or_default();
        for other in others {
            assert!(
                !unit.contains(&format!("Environment=\"{other}=")),
                "{variable}: {other} was carried: {unit}"
            );
            assert!(
                unset.split(' ').any(|name| name == other),
                "{variable}: {other} is not unset: {unit}"
            );
        }
        let stop = account.run(&["service", "stop", "--yes"]);
        assert!(stop.status.success(), "{:?}", text(&stop));
    }
}
