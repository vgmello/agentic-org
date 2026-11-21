//! `agentic-org office list` and `agentic-org office restart`.

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", org_host::setup_flow::OFFICE_HELP);
            Ok(())
        }
        [sub, rest @ ..] if sub == "list" => run_list(rest),
        [sub, rest @ ..] if sub == "restart" => run_restart(rest),
        _ => Err(org_host::setup_flow::OFFICE_HELP.to_owned()),
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
        let dup = run(&["list", "--login", "Owner", "--json", "--json"]).unwrap_err();
        assert!(dup.contains("more than once"), "{dup}");
    }
}
