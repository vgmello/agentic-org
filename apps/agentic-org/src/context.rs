//! `agentic-org context show|use|clear` (A5 / SES-8).

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", org_host::setup_flow::CONTEXT_HELP);
            Ok(())
        }
        [sub] if sub == "show" => {
            let mut out = std::io::stdout().lock();
            org_host::setup_flow::context_show(&mut out)
        }
        [sub] if sub == "clear" => {
            let mut out = std::io::stdout().lock();
            org_host::setup_flow::context_clear(&mut out)
        }
        [sub, flag]
            if matches!(sub.as_str(), "show" | "clear")
                && matches!(flag.as_str(), "--help" | "-h") =>
        {
            println!("{}", org_host::setup_flow::CONTEXT_HELP);
            Ok(())
        }
        [sub, rest @ ..] if sub == "use" => {
            if matches!(rest, [flag] if flag == "--help" || flag == "-h") {
                println!("{}", org_host::setup_flow::CONTEXT_HELP);
                return Ok(());
            }
            let options = crate::options(rest, &["--office", "--name"])?;
            let office = options.get("--office").ok_or_else(|| {
                format!(
                    "--office is required\n{}",
                    org_host::setup_flow::CONTEXT_HELP
                )
            })?;
            let mut out = std::io::stdout().lock();
            org_host::setup_flow::context_use(
                options.get("--name").map(String::as_str),
                office,
                &mut out,
            )
        }
        _ => Err(org_host::setup_flow::CONTEXT_HELP.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    fn run(args: &[&str]) -> Result<(), String> {
        super::run(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn context_cli_validates_arguments_before_host_contact() {
        assert!(run(&["--help"]).is_ok());
        assert!(run(&["-h"]).is_ok());
        assert!(run(&["show", "--help"]).is_ok());
        assert!(run(&["show", "-h"]).is_ok());
        assert!(run(&["clear", "--help"]).is_ok());
        assert!(run(&["clear", "-h"]).is_ok());
        assert!(run(&["use", "--help"]).is_ok());
        assert!(run(&["use", "-h"]).is_ok());
        assert!(run(&["use"])
            .unwrap_err()
            .starts_with("--office is required"));
        assert!(run(&["bogus"]).is_err());
    }
}
