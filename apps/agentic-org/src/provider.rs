//! `agentic-org provider create|list|bind|bindings|models` (`TASK-120`).

use org_host::setup_flow::{ProviderBindInput, ProviderCreateInput};

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", org_host::setup_flow::PROVIDER_HELP);
            Ok(())
        }
        [sub, rest @ ..] if sub == "create" => run_create(rest),
        [sub, rest @ ..] if sub == "list" => run_list(rest),
        [sub, rest @ ..] if sub == "bind" => run_bind(rest),
        [sub, rest @ ..] if sub == "bindings" => run_bindings(rest),
        [sub, rest @ ..] if sub == "models" => run_models(rest),
        _ => Err(org_host::setup_flow::PROVIDER_HELP.to_owned()),
    }
}

fn strip_flags(arguments: &[String]) -> Result<(Vec<String>, bool, bool), String> {
    let mut json = false;
    let mut include_archived = false;
    let mut rest = Vec::with_capacity(arguments.len());
    for arg in arguments {
        match arg.as_str() {
            "--json" => {
                if json {
                    return Err("--json is given more than once".into());
                }
                json = true;
            }
            "--include-archived" => {
                if include_archived {
                    return Err("--include-archived is given more than once".into());
                }
                include_archived = true;
            }
            _ => rest.push(arg.clone()),
        }
    }
    Ok((rest, json, include_archived))
}

fn require_positional<'a>(
    arguments: &'a [String],
    label: &str,
) -> Result<(&'a str, &'a [String]), String> {
    let Some((first, tail)) = arguments.split_first() else {
        return Err(format!(
            "{label} is required\n{}",
            org_host::setup_flow::PROVIDER_HELP
        ));
    };
    if first.starts_with("--") {
        return Err(format!(
            "{label} is required\n{}",
            org_host::setup_flow::PROVIDER_HELP
        ));
    }
    Ok((first.as_str(), tail))
}

fn run_create(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::PROVIDER_HELP);
        return Ok(());
    }
    let (name, tail) = require_positional(arguments, "<name>")?;
    let (rest, json, archived) = strip_flags(tail)?;
    if archived {
        return Err("unrecognized argument \"--include-archived\"".into());
    }
    let options = crate::options(
        &rest,
        &[
            "--key",
            "--provider",
            "--runtime",
            "--auth-mode",
            "--max-turns",
            "--org",
            "--login",
            "--name",
        ],
    )?;
    let key = options
        .get("--key")
        .ok_or_else(|| format!("--key is required\n{}", org_host::setup_flow::PROVIDER_HELP))?;
    let provider_type = options.get("--provider").ok_or_else(|| {
        format!(
            "--provider is required\n{}",
            org_host::setup_flow::PROVIDER_HELP
        )
    })?;
    let runtime_driver = options.get("--runtime").ok_or_else(|| {
        format!(
            "--runtime is required\n{}",
            org_host::setup_flow::PROVIDER_HELP
        )
    })?;
    let auth_mode = options
        .get("--auth-mode")
        .map(String::as_str)
        .unwrap_or("cli_subscription");
    let max_concurrent_turns = options
        .get("--max-turns")
        .map(|raw| {
            raw.parse::<u64>()
                .ok()
                .filter(|v| *v >= 1)
                .ok_or_else(|| format!("--max-turns must be a positive integer, not {raw:?}"))
        })
        .transpose()?
        .unwrap_or(2);
    let input = ProviderCreateInput {
        org_id: options.get("--org").map(String::as_str),
        name,
        key,
        provider_type,
        runtime_driver,
        auth_mode,
        max_concurrent_turns,
        json,
    };
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::provider_create(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        &input,
        &mut out,
    )
}

fn run_list(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::PROVIDER_HELP);
        return Ok(());
    }
    let (rest, json, include_archived) = strip_flags(arguments)?;
    let options = crate::options(&rest, &["--org", "--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::provider_list(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        include_archived,
        json,
        &mut out,
    )
}

fn run_bind(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::PROVIDER_HELP);
        return Ok(());
    }
    let (connection_id, tail) = require_positional(arguments, "<connection_id>")?;
    let (rest, json, archived) = strip_flags(tail)?;
    if archived {
        return Err("unrecognized argument \"--include-archived\"".into());
    }
    let options = crate::options(
        &rest,
        &[
            "--auth-ref",
            "--binary",
            "--account-ref",
            "--office",
            "--org",
            "--login",
            "--name",
        ],
    )?;
    let auth_context_ref = options.get("--auth-ref").ok_or_else(|| {
        format!(
            "--auth-ref is required\n{}",
            org_host::setup_flow::PROVIDER_HELP
        )
    })?;
    let input = ProviderBindInput {
        org_id: options.get("--org").map(String::as_str),
        office_id: options.get("--office").map(String::as_str),
        connection_id,
        auth_context_ref,
        runtime_binary_path: options.get("--binary").map(String::as_str),
        account_identity_ref: options.get("--account-ref").map(String::as_str),
        json,
    };
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::provider_bind(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        &input,
        &mut out,
    )
}

fn run_bindings(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::PROVIDER_HELP);
        return Ok(());
    }
    let (rest, json, archived) = strip_flags(arguments)?;
    if archived {
        return Err("unrecognized argument \"--include-archived\"".into());
    }
    let options = crate::options(
        &rest,
        &["--connection", "--office", "--org", "--login", "--name"],
    )?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::provider_bindings(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        options.get("--office").map(String::as_str),
        options.get("--connection").map(String::as_str),
        json,
        &mut out,
    )
}

fn run_models(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::PROVIDER_HELP);
        return Ok(());
    }
    let (rest, json, archived) = strip_flags(arguments)?;
    if archived {
        return Err("unrecognized argument \"--include-archived\"".into());
    }
    let options = crate::options(&rest, &["--office", "--org", "--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::provider_models(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        options.get("--office").map(String::as_str),
        json,
        &mut out,
    )
}

#[cfg(test)]
mod tests {
    fn run(arguments: &[&str]) -> Result<(), String> {
        super::run(
            &arguments
                .iter()
                .map(|a| (*a).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn provider_cli_validates_arguments_before_host_contact() {
        for help in [
            vec!["--help"],
            vec!["create", "--help"],
            vec!["list", "--help"],
            vec!["bind", "--help"],
            vec!["bindings", "--help"],
            vec!["models", "--help"],
        ] {
            assert_eq!(run(&help), Ok(()));
        }
        assert!(run(&["create"]).unwrap_err().contains("<name>"));
        assert!(run(&["create", "Claude"])
            .unwrap_err()
            .contains("--key is required"));
        assert!(run(&[
            "create",
            "Claude",
            "--key",
            "claude",
            "--provider",
            "anthropic",
            "--runtime",
            "claude",
            "--max-turns",
            "0"
        ])
        .unwrap_err()
        .contains("--max-turns"));
        assert!(run(&["bind"]).unwrap_err().contains("<connection_id>"));
        assert!(run(&["bind", "conn-1"])
            .unwrap_err()
            .contains("--auth-ref is required"));
    }
}
