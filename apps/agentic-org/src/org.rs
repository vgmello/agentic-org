//! `agentic-org org create|list` (`TASK-113`).

use org_host::setup_flow::OrgCreateInput;

const DEFAULT_STATE_BRANCH: &str = "main";

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", org_host::setup_flow::ORG_HELP);
            Ok(())
        }
        [sub, rest @ ..] if sub == "create" => run_create(rest),
        [sub, rest @ ..] if sub == "list" => run_list(rest),
        _ => Err(org_host::setup_flow::ORG_HELP.to_owned()),
    }
}

fn strip_flags(arguments: &[String]) -> Result<(Vec<String>, bool, bool), String> {
    let mut json = false;
    let mut no_wait = false;
    let mut rest = Vec::with_capacity(arguments.len());
    for arg in arguments {
        match arg.as_str() {
            "--json" => {
                if json {
                    return Err("--json is given more than once".into());
                }
                json = true;
            }
            "--no-wait" => {
                if no_wait {
                    return Err("--no-wait is given more than once".into());
                }
                no_wait = true;
            }
            _ => rest.push(arg.clone()),
        }
    }
    Ok((rest, json, no_wait))
}

fn run_create(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::ORG_HELP);
        return Ok(());
    }
    let Some((name, tail)) = arguments.split_first() else {
        return Err(format!(
            "<name> is required\n{}",
            org_host::setup_flow::ORG_HELP
        ));
    };
    if name.starts_with("--") {
        return Err(format!(
            "<name> is required\n{}",
            org_host::setup_flow::ORG_HELP
        ));
    }
    let (rest, json, no_wait) = strip_flags(tail)?;
    let options = crate::options(
        &rest,
        &["--slug", "--repo", "--branch", "--login", "--name"],
    )?;
    let slug = options
        .get("--slug")
        .ok_or_else(|| format!("--slug is required\n{}", org_host::setup_flow::ORG_HELP))?;
    let repo = options
        .get("--repo")
        .ok_or_else(|| format!("--repo is required\n{}", org_host::setup_flow::ORG_HELP))?;
    let branch = options
        .get("--branch")
        .map(String::as_str)
        .unwrap_or(DEFAULT_STATE_BRANCH);
    let input = OrgCreateInput {
        name,
        slug,
        repository_remote_url: repo,
        state_branch: branch,
        no_wait,
        json,
    };
    // Validate plan before locating or contacting any host instance.
    org_host::setup_flow::validate_create_plan(&input)?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::org_create(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        &input,
        &mut out,
    )
}

fn run_list(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::ORG_HELP);
        return Ok(());
    }
    let (rest, json, no_wait) = strip_flags(arguments)?;
    if no_wait {
        return Err("unrecognized argument \"--no-wait\"".into());
    }
    let options = crate::options(&rest, &["--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::org_list(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
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
    fn org_cli_validates_arguments_before_host_contact() {
        assert!(run(&["--help"]).is_ok());
        assert!(run(&["create", "--help"]).is_ok());
        assert!(run(&["list", "--help"]).is_ok());

        let missing_name = run(&["create", "--slug", "acme"]).unwrap_err();
        assert!(
            missing_name.starts_with("<name> is required"),
            "{missing_name}"
        );
        let missing_slug = run(&["create", "Acme"]).unwrap_err();
        assert!(
            missing_slug.starts_with("--slug is required"),
            "{missing_slug}"
        );
        let missing_repo = run(&["create", "Acme", "--slug", "acme"]).unwrap_err();
        assert!(
            missing_repo.starts_with("--repo is required"),
            "{missing_repo}"
        );
        let local_repo = run(&[
            "create",
            "Acme",
            "--slug",
            "acme",
            "--repo",
            "file:///tmp/acme",
        ])
        .unwrap_err();
        assert!(
            local_repo.contains("invalid organization plan"),
            "{local_repo}"
        );
        let dup_json = run(&["list", "--json", "--json"]).unwrap_err();
        assert!(dup_json.contains("more than once"), "{dup_json}");
        let bad_flag = run(&["list", "--no-wait"]).unwrap_err();
        assert!(bad_flag.contains("--no-wait"), "{bad_flag}");
    }
}
