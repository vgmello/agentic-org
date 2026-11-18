//! Non-interactive setup from a file (`setup --non-interactive --config <file>`) and the
//! foreground entry (`serve --setup <file>`), with the real binary. The registered mode
//! runs through the detached-child service adapter. The foreground mode needs a mounted
//! secrets volume on a system that keeps secrets in a directory, so its whole path runs in
//! the container test (`deploy/container/lnx2.sh`); here are its refusals.
use super::*;

fn file(account: &Account, name: &str, text: &str) -> String {
    let path = account.base.join(name);
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

fn registered(account: &Account, port: u16) -> String {
    format!(
        r#"{{"version":1,"service":"registered","root":"{}","listen":{{"address":"127.0.0.1","port":{port},"scheme":"http_loopback","origin":"http://127.0.0.1:{port}","hsts":"off"}}}}"#,
        account.base.join("instance").display()
    )
}

/// The file is the whole input: the address is the file's, nothing is asked, the service
/// is registered and started, and no link is asked for or printed even at a terminal
/// (ADR-0022, UX §4a.2). A second run changes nothing; another file for the same instance
/// is refused.
#[test]
fn setup_from_a_file_registers_the_service_and_prints_no_link() {
    let account = Account::new();
    let port = free_port();
    let config = file(&account, "setup.json", &registered(&account, port));
    let (ok, out, err) =
        account.run_in_terminal(&["setup", "--non-interactive", "--config", &config]);
    assert!(ok, "stdout:\n{out}\nstderr:\n{err}");
    assert!(
        out.contains(&format!("Address: http://127.0.0.1:{port}")),
        "{out}"
    );
    assert!(out.contains("Service running."), "{out}");
    assert!(
        link_token(&out).is_none() && !out.contains("/register"),
        "{out}"
    );
    assert!(
        out.contains("For a new link, run: agentic-org admin reset"),
        "{out}"
    );
    let record = account.record("default");
    assert_eq!(record["offline"], true);
    assert_eq!(record["input"]["service"], "registered");
    assert_eq!(record["initial_link_issued"], false);
    let pids = account.service_pids();
    assert_eq!(pids.len(), 1);
    assert!(alive(pids[0].1));

    let again = account.run(&["setup", "--non-interactive", "--config", &config]);
    assert!(again.status.success(), "{:?}", text(&again));
    assert_eq!(account.service_pids(), pids, "the service was restarted");
    assert_eq!(account.record("default"), record);

    // The same instance with another address: refused, and nothing changes.
    let other = file(&account, "other.json", &registered(&account, free_port()));
    let refused = account.run(&["setup", "--non-interactive", "--config", &other]);
    assert!(!refused.status.success());
    assert!(
        text(&refused).1.contains("a different configuration"),
        "{:?}",
        text(&refused)
    );
    assert_eq!(account.record("default"), record);
    assert_eq!(account.service_pids(), pids);

    // `serve --setup` is the foreground entry: a file that sets up a registered service
    // is refused before anything is changed or served.
    let served = account.run(&["serve", "--setup", &config]);
    assert!(!served.status.success());
    assert!(
        text(&served)
            .1
            .contains("needs \"service\": \"foreground\""),
        "{:?}",
        text(&served)
    );
    assert_eq!(account.record("default"), record);
}

/// What is refused before anything is made: the options that do not go together, a file
/// that is missing, not strict or names another version, and a foreground file whose
/// credential directory is not what this system needs (on Linux a plain directory is not a
/// mounted volume; where secrets are kept elsewhere a directory is not named at all).
#[test]
fn a_configuration_that_cannot_be_used_is_refused_and_nothing_is_saved() {
    let account = Account::new();
    let port = free_port();
    let good = file(&account, "good.json", &registered(&account, port));
    let unknown = file(
        &account,
        "unknown.json",
        &registered(&account, port).replacen(
            "\"version\":1",
            "\"version\":1,\"password\":\"x\"",
            1,
        ),
    );
    let version = file(
        &account,
        "version.json",
        &registered(&account, port).replacen("\"version\":1", "\"version\":2", 1),
    );
    // Plain HTTP on a published port is the foreground mode's: chosen for a registered
    // service it is refused, whatever the address and the origin are.
    let published = file(
        &account,
        "published.json",
        &registered(&account, port)
            .replacen("\"127.0.0.1\"", "\"0.0.0.0\"", 1)
            .replacen("\"http_loopback\"", "\"http_published\"", 1)
            .replacen("http://127.0.0.1:", "http://localhost:", 1),
    );
    // Never implied by plain `http_loopback` and another address.
    let implied = file(
        &account,
        "implied.json",
        &registered(&account, port)
            .replacen("\"registered\"", "\"foreground\"", 1)
            .replacen("\"127.0.0.1\"", "\"0.0.0.0\"", 1),
    );
    let missing = account.base.join("missing.json").display().to_string();
    for (args, expected) in [
        (
            vec!["setup", "--non-interactive"],
            "--non-interactive needs --config",
        ),
        (
            vec!["setup", "--config", &good],
            "only with --non-interactive",
        ),
        (
            vec![
                "setup",
                "--non-interactive",
                "--config",
                &good,
                "--port",
                "7420",
            ],
            "the address is in the file",
        ),
        (
            vec!["setup", "--non-interactive", "--config", &missing],
            "reading the setup configuration",
        ),
        (
            vec!["setup", "--non-interactive", "--config", &unknown],
            "no other property is accepted",
        ),
        (
            vec!["setup", "--non-interactive", "--config", &version],
            "\"version\" must be 1",
        ),
        (
            vec!["serve", "--setup", &missing],
            "reading the setup configuration",
        ),
        (
            vec!["serve", "--setup", &unknown],
            "no other property is accepted",
        ),
        (vec!["serve", "--setup"], "--setup needs a file"),
        (
            vec!["setup", "--non-interactive", "--config", &published],
            "only for \"service\": \"foreground\"",
        ),
        (
            vec!["serve", "--setup", &published],
            "only for \"service\": \"foreground\"",
        ),
        (
            vec!["setup", "--non-interactive", "--config", &implied],
            "no other property is accepted",
        ),
        (
            vec!["serve", "--setup", &implied],
            "no other property is accepted",
        ),
    ] {
        let refused = account.run(&args);
        let (out, err) = text(&refused);
        assert!(!refused.status.success(), "{args:?}: {out}");
        assert!(err.contains(expected), "{args:?}: {err}");
    }
    assert!(
        !account.base.join("app/setup").exists(),
        "a setup record was saved"
    );

    // The foreground mode with a plain directory as its credential directory.
    let secrets = account.base.join("secrets");
    std::fs::create_dir(&secrets).unwrap();
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o700)).unwrap();
    let foreground = file(
        &account,
        "foreground.json",
        &registered(&account, port)
            .replacen("\"registered\"", "\"foreground\"", 1)
            .replacen(
                "\"listen\"",
                &format!("\"credentials\":\"{}\",\"listen\"", secrets.display()),
                1,
            ),
    );
    for command in [
        vec!["setup", "--non-interactive", "--config", &foreground],
        vec!["serve", "--setup", &foreground],
    ] {
        let refused = account.run(&command);
        let (out, err) = text(&refused);
        assert!(!refused.status.success(), "{command:?}: {out}");
        assert!(
            err.contains("the credential directory cannot be used") || err.contains("remove it"),
            "{command:?}: {err}"
        );
        assert!(!out.contains("Service"), "{command:?}: {out}");
    }
    assert!(account.service_pids().is_empty(), "something was started");
    assert!(
        std::fs::read_dir(&secrets).unwrap().next().is_none(),
        "the refused directory was written to"
    );
}
