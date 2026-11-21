//! `agentic-org project list`, `inspect`, and `create`.

use org_host::setup_flow::ProjectCreateInput;

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", org_host::setup_flow::PROJECT_HELP);
            Ok(())
        }
        [sub, rest @ ..] if sub == "list" => run_list(rest),
        [sub, rest @ ..] if sub == "inspect" => run_inspect(rest),
        [sub, rest @ ..] if sub == "create" => run_create(rest),
        _ => Err(org_host::setup_flow::PROJECT_HELP.to_owned()),
    }
}

fn strip_flags(
    arguments: &[String],
    allow_include_archived: bool,
) -> Result<(Vec<String>, bool, bool), String> {
    let mut json = false;
    let mut include_archived = false;
    let mut rest = Vec::with_capacity(arguments.len());
    for arg in arguments {
        if arg == "--json" {
            if json {
                return Err("--json is given more than once".into());
            }
            json = true;
        } else if allow_include_archived && arg == "--include-archived" {
            if include_archived {
                return Err("--include-archived is given more than once".into());
            }
            include_archived = true;
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((rest, json, include_archived))
}

fn run_list(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::PROJECT_HELP);
        return Ok(());
    }
    let (rest, json, include_archived) = strip_flags(arguments, true)?;
    let options = crate::options(&rest, &["--login", "--org", "--name"])?;
    let login = options.get("--login").ok_or_else(|| {
        format!(
            "--login is required\n{}",
            org_host::setup_flow::PROJECT_HELP
        )
    })?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::project_list(
        options.get("--name").map(String::as_str),
        login,
        options.get("--org").map(String::as_str),
        include_archived,
        json,
        &mut out,
    )
}

fn run_inspect(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::PROJECT_HELP);
        return Ok(());
    }
    let Some((project, tail)) = arguments.split_first() else {
        return Err(format!(
            "<project> is required\n{}",
            org_host::setup_flow::PROJECT_HELP
        ));
    };
    if project.starts_with("--") {
        return Err(format!(
            "<project> is required\n{}",
            org_host::setup_flow::PROJECT_HELP
        ));
    }
    let (rest, json, _) = strip_flags(tail, false)?;
    let options = crate::options(&rest, &["--login", "--org", "--name"])?;
    let login = options.get("--login").ok_or_else(|| {
        format!(
            "--login is required\n{}",
            org_host::setup_flow::PROJECT_HELP
        )
    })?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::project_inspect(
        options.get("--name").map(String::as_str),
        login,
        options.get("--org").map(String::as_str),
        project,
        json,
        &mut out,
    )
}

fn run_create(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::PROJECT_HELP);
        return Ok(());
    }
    let Some((name, tail)) = arguments.split_first() else {
        return Err(format!(
            "<name> is required\n{}",
            org_host::setup_flow::PROJECT_HELP
        ));
    };
    if name.starts_with("--") {
        return Err(format!(
            "<name> is required\n{}",
            org_host::setup_flow::PROJECT_HELP
        ));
    }
    let (rest, json, _) = strip_flags(tail, false)?;
    let options = crate::options(
        &rest,
        &[
            "--login",
            "--org",
            "--slug",
            "--owner-team",
            "--parent-team",
            "--name",
        ],
    )?;
    let login = options.get("--login").ok_or_else(|| {
        format!(
            "--login is required\n{}",
            org_host::setup_flow::PROJECT_HELP
        )
    })?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::project_create(
        options.get("--name").map(String::as_str),
        login,
        &ProjectCreateInput {
            org_id: options.get("--org").map(String::as_str),
            name,
            slug: options.get("--slug").map(String::as_str),
            owner_team_id: options.get("--owner-team").map(String::as_str),
            parent_team_id: options.get("--parent-team").map(String::as_str),
            json,
        },
        &mut out,
    )
}

#[cfg(test)]
mod tests {
    fn run(arguments: &[&str]) -> Result<(), String> {
        super::run(&arguments.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn project_cli_validates_arguments_before_host_contact() {
        assert!(run(&["--help"]).is_ok());
        assert!(run(&["list", "--help"]).is_ok());
        assert!(run(&["inspect", "--help"]).is_ok());
        assert!(run(&["create", "--help"]).is_ok());
        assert!(run(&["list"])
            .unwrap_err()
            .starts_with("--login is required"));
        assert!(run(&["inspect", "--login", "Owner"])
            .unwrap_err()
            .starts_with("<project> is required"));
        assert!(run(&["create", "--login", "Owner"])
            .unwrap_err()
            .starts_with("<name> is required"));
        assert!(run(&["create", "Billing"])
            .unwrap_err()
            .starts_with("--login is required"));
        assert!(run(&["list", "--login", "Owner", "--json", "--json"])
            .unwrap_err()
            .contains("more than once"));
    }
}
