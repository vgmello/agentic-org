//! `setup`: its help, its refusal order and its federation flags.
use super::run::{run_as, SetUp, HELP};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// X5 review V-d: the help says what setup does on every system it runs on.
#[test]
fn the_help_names_no_one_system() {
    for help in [
        HELP,
        crate::cli::command()
            .render_long_help()
            .to_string()
            .as_str(),
    ] {
        assert!(!help.contains("Mac"), "{help}");
    }
    assert!(HELP.starts_with("Set up this computer's instance"));
}

/// A user who may not own a service is refused before setup reads or makes anything:
/// before its options are even parsed, so no instance is reserved, no custody is made
/// and no service is registered. Help and the satellite refusal read nothing and are
/// answered to anyone.
#[test]
fn a_user_who_may_not_run_the_service_is_refused_before_setup_reads_anything() {
    let refusal = || Err("the service does not run as this user".to_owned());
    for list in [
        vec![],
        vec!["--name", "lab"],
        vec!["--port", "7420"],
        // Refused as the user, not as a bad option: nothing was parsed yet.
        vec!["--frobnicate"],
        vec!["--port", "not-a-port"],
    ] {
        assert_eq!(
            run_as(&args(&list), refusal, &mut Vec::new()),
            Err("the service does not run as this user".into()),
            "{list:?}"
        );
    }
    let unasked = || -> Result<(), String> { panic!("asked who runs a help query") };
    assert_eq!(
        run_as(&args(&["--help"]), unasked, &mut Vec::new()),
        Ok(SetUp::Help)
    );
    let unasked = || -> Result<(), String> { panic!("asked before the satellite refusal") };
    assert!(run_as(&args(&["--satellite"]), unasked, &mut Vec::new())
        .unwrap_err()
        .contains("setup creates HQ only"));
    // The composition's own check is the host's rule for this process's user.
    assert_eq!(
        org_host::Config::refuse_root().is_err(),
        org_socket::effective_uid() == 0
    );
}

#[test]
fn federation_flags_are_validated_before_setup_runs() {
    let allow = || Ok(());
    let no_bind = run_as(
        &args(&["--federation-advertise", "hq.example.com"]),
        allow,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(no_bind.contains("--federation-bind"), "{no_bind}");
    let bad_bind = run_as(
        &args(&["--federation-bind", "0.0.0.0:0"]),
        allow,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(bad_bind.contains("1 to 65535"), "{bad_bind}");
    let bad_adv = run_as(
        &args(&[
            "--federation-bind",
            "0.0.0.0:7443",
            "--federation-advertise",
            "hq.example.com:7443",
        ]),
        allow,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(bad_adv.contains("without a port"), "{bad_adv}");
    for empty_adv in ["", ",,,", "büro.example.com"] {
        assert!(
            run_as(
                &args(&[
                    "--federation-bind",
                    "0.0.0.0:7443",
                    "--federation-advertise",
                    empty_adv,
                ]),
                allow,
                &mut Vec::new(),
            )
            .is_err(),
            "--federation-advertise {empty_adv:?} should be refused"
        );
    }
    let with_cfg = run_as(
        &args(&[
            "--non-interactive",
            "--config",
            "/tmp/nonexistent.json",
            "--federation-bind",
            "0.0.0.0:7443",
        ]),
        allow,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(with_cfg.contains("not used with --config"), "{with_cfg}");
}
