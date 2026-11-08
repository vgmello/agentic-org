#![cfg(target_os = "macos")]
//! The child's environment is exactly what was built: nothing of this process's leaks.
//! One test in its own binary, so setting the canary races with no other test thread.

mod common;

use std::collections::BTreeSet;

use common::*;
use org_supervise::spawn_session;

#[test]
fn the_child_gets_exactly_the_built_environment() {
    std::env::set_var("OSV_CANARY", "leaked");
    std::env::set_var("SSH_AUTH_SOCK", "/leaked.sock");
    std::env::set_var("GH_TOKEN", "leaked");

    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let rt = tmp.dir("rt");
    let mark = mark("env");
    let env = session_env(&tmp, &ws, &mark);
    let mut p = plan("/usr/bin/env", &[], &ws);
    p.env_add = vec![
        (name("DISABLE_AUTOUPDATER"), "1".into()),
        (name("HOME"), "/override-attempt".into()),
        (name("GIT_DIR"), "/override-attempt".into()),
    ];
    let mut child = spawn_session(&p, &targets(&rt), &env).unwrap();
    let pid = child.pid();
    let out = Reader::spawn(child.take_stdout().unwrap()).read_to_end();
    let exit = child.reap().unwrap();
    assert_eq!(exit.info.code, Some(0));

    let got: BTreeSet<&str> = out.lines().collect();
    let t = tmp.path().display().to_string();
    let expected_owned: Vec<String> = vec![
        format!("PATH={t}/office/bin:{LOGIN_PATH}"),
        format!("HOME={t}"),
        "LANG=en_US.UTF-8".into(),
        "USER=tester".into(),
        "LOGNAME=tester".into(),
        "SHELL=/bin/zsh".into(),
        format!("TMPDIR={t}/t"),
        "ORG_AGENT=ac-dev-ada".into(),
        "ORG_OFFICE=office-1".into(),
        format!("ORG_OFFICE_HOME={t}/office"),
        "ORG_SESSION_TOKEN=tok-SECRET-123".into(),
        format!("ORG_SESSION_MARK={mark}"),
        "GIT_CONFIG_NOSYSTEM=1".into(),
        "GIT_CONFIG_GLOBAL=/dev/null".into(),
        "GIT_CONFIG_COUNT=7".into(),
        "GIT_CONFIG_KEY_0=user.name".into(),
        "GIT_CONFIG_VALUE_0=ada (agent)".into(),
        "GIT_CONFIG_KEY_1=user.email".into(),
        "GIT_CONFIG_VALUE_1=ada@agents.example".into(),
        "GIT_CONFIG_KEY_2=credential.helper".into(),
        "GIT_CONFIG_VALUE_2=".into(),
        "GIT_CONFIG_KEY_3=core.hooksPath".into(),
        "GIT_CONFIG_VALUE_3=/dev/null".into(),
        "GIT_CONFIG_KEY_4=core.fsmonitor".into(),
        "GIT_CONFIG_VALUE_4=false".into(),
        "GIT_CONFIG_KEY_5=gc.auto".into(),
        "GIT_CONFIG_VALUE_5=0".into(),
        "GIT_CONFIG_KEY_6=maintenance.auto".into(),
        "GIT_CONFIG_VALUE_6=false".into(),
        format!("GH_CONFIG_DIR={t}/t/gh/"),
        "DISABLE_AUTOUPDATER=1".into(),
    ];
    let expected: BTreeSet<&str> = expected_owned.iter().map(String::as_str).collect();
    assert_eq!(got, expected);
    assert!(!out.contains("leaked") && !out.contains("TERM="));
    // The token never shows in Debug output.
    assert!(!format!("{env:?} {p:?}").contains("tok-SECRET-123"));
    assert_clean(&[pid], Some(&mark));
}
