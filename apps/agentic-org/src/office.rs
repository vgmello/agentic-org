//! `agentic-org office list`, `restart`, `join`, `remove-pending`, and `re-enroll`.

use std::io::Read;

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", org_host::setup_flow::OFFICE_HELP);
            Ok(())
        }
        [sub, rest @ ..] if sub == "list" => run_list(rest),
        [sub, rest @ ..] if sub == "restart" => run_restart(rest),
        [sub, rest @ ..] if sub == "join" => run_join(rest),
        [sub, rest @ ..] if sub == "remove-pending" => run_remove_pending(rest),
        [sub, rest @ ..] if sub == "re-enroll" => run_re_enroll(rest),
        _ => Err(org_host::setup_flow::OFFICE_HELP.to_owned()),
    }
}

fn strip_flags(arguments: &[String]) -> Result<(Vec<String>, bool, bool, bool), String> {
    let mut json = false;
    let mut yes = false;
    let mut stdin_inv = false;
    let mut rest = Vec::with_capacity(arguments.len());
    for arg in arguments {
        match arg.as_str() {
            "--json" => {
                if json {
                    return Err("--json is given more than once".into());
                }
                json = true;
            }
            "--yes" => {
                if yes {
                    return Err("--yes is given more than once".into());
                }
                yes = true;
            }
            "--invitation-stdin" => {
                if stdin_inv {
                    return Err("--invitation-stdin is given more than once".into());
                }
                stdin_inv = true;
            }
            _ => rest.push(arg.clone()),
        }
    }
    Ok((rest, json, yes, stdin_inv))
}

fn read_invitation_token(file: Option<&str>, from_stdin: bool) -> Result<String, String> {
    match (file, from_stdin) {
        (Some(_), true) => {
            Err("specify either --invitation-file or --invitation-stdin, not both".into())
        }
        (None, false) => Err(format!(
            "--invitation-file <path> or --invitation-stdin is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        )),
        (Some(path), false) => std::fs::read_to_string(path)
            .map(|s| s.trim().to_owned())
            .map_err(|e| format!("cannot read invitation file `{path}`: {e}")),
        (None, true) => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("cannot read invitation token from stdin: {e}"))?;
            Ok(buf.trim().to_owned())
        }
    }
}

fn strip_json_flag(arguments: &[String]) -> Result<(Vec<String>, bool), String> {
    let mut json = false;
    let mut rest = Vec::with_capacity(arguments.len());
    for arg in arguments {
        if arg == "--json" {
            if json {
                return Err("--json is given more than once".into());
            }
            json = true;
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((rest, json))
}

fn run_list(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (rest, json) = strip_json_flag(arguments)?;
    let options = crate::options(&rest, &["--login", "--org", "--name"])?;
    let login = options
        .get("--login")
        .ok_or_else(|| format!("--login is required\n{}", org_host::setup_flow::OFFICE_HELP))?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_list(
        options.get("--name").map(String::as_str),
        login,
        options.get("--org").map(String::as_str),
        json,
        &mut out,
    )
}

fn run_restart(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let Some((office_id, tail)) = arguments.split_first() else {
        return Err(format!(
            "<office_id> is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        ));
    };
    if office_id.starts_with("--") {
        return Err(format!(
            "<office_id> is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        ));
    }
    let (rest, json) = strip_json_flag(tail)?;
    let options = crate::options(&rest, &["--login", "--name"])?;
    let login = options
        .get("--login")
        .ok_or_else(|| format!("--login is required\n{}", org_host::setup_flow::OFFICE_HELP))?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_restart(
        options.get("--name").map(String::as_str),
        login,
        office_id,
        json,
        &mut out,
    )
}

fn run_join(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (rest, json, yes, stdin_inv) = strip_flags(arguments)?;
    let options = crate::options(&rest, &["--login", "--name", "--invitation-file"])?;
    if !yes {
        return Err("--yes is required to confirm joining this host to the organization".into());
    }
    let token = read_invitation_token(
        options.get("--invitation-file").map(String::as_str),
        stdin_inv,
    )?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_join(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        token,
        yes,
        json,
        &mut out,
    )
}

fn run_remove_pending(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let Some((office_id, tail)) = arguments.split_first() else {
        return Err(format!(
            "<office_id> is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        ));
    };
    if office_id.starts_with("--") {
        return Err(format!(
            "<office_id> is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        ));
    }
    let (rest, json) = strip_json_flag(tail)?;
    let options = crate::options(&rest, &["--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_remove_pending(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        office_id,
        json,
        &mut out,
    )
}

fn run_re_enroll(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let Some((office_id, tail)) = arguments.split_first() else {
        return Err(format!(
            "<office_id> is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        ));
    };
    if office_id.starts_with("--") {
        return Err(format!(
            "<office_id> is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        ));
    }
    let (rest, json, yes, stdin_inv) = strip_flags(tail)?;
    let options = crate::options(&rest, &["--login", "--name", "--invitation-file"])?;
    if !yes {
        return Err("--yes is required to confirm re-enrolling this satellite office".into());
    }
    let token = read_invitation_token(
        options.get("--invitation-file").map(String::as_str),
        stdin_inv,
    )?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_re_enroll(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        office_id,
        token,
        yes,
        json,
        &mut out,
    )
}

#[cfg(test)]
mod tests {
    fn run(arguments: &[&str]) -> Result<(), String> {
        super::run(&arguments.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn office_cli_validates_arguments_before_host_contact() {
        assert!(run(&["--help"]).is_ok());
        assert!(run(&["list", "--help"]).is_ok());
        assert!(run(&["restart", "--help"]).is_ok());
        assert!(run(&["join", "--help"]).is_ok());
        assert!(run(&["remove-pending", "--help"]).is_ok());
        assert!(run(&["re-enroll", "--help"]).is_ok());
        let missing = run(&["list"]).unwrap_err();
        assert!(missing.starts_with("--login is required"), "{missing}");
        let missing_office = run(&["restart", "--login", "Owner"]).unwrap_err();
        assert!(
            missing_office.starts_with("<office_id> is required"),
            "{missing_office}"
        );
        let missing_login = run(&["restart", "office-1"]).unwrap_err();
        assert!(
            missing_login.starts_with("--login is required"),
            "{missing_login}"
        );
        let no_yes = run(&["join", "--login", "Owner", "--invitation-stdin"]).unwrap_err();
        assert!(no_yes.contains("--yes is required"), "{no_yes}");
        let no_inv = run(&["join", "--login", "Owner", "--yes"]).unwrap_err();
        assert!(no_inv.contains("--invitation-file"), "{no_inv}");
        let no_re_office = run(&["re-enroll", "--login", "Owner"]).unwrap_err();
        assert!(
            no_re_office.starts_with("<office_id> is required"),
            "{no_re_office}"
        );
        let dup = run(&["list", "--login", "Owner", "--json", "--json"]).unwrap_err();
        assert!(dup.contains("more than once"), "{dup}");
    }
}
