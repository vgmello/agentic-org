//! `agentic-org service start|stop|status [--name <instance>]`: the registered service of an
//! instance setup created, through the same platform adapter setup used.
use org_host::setup_flow::ServiceAction;

pub(crate) const HELP: &str = "agentic-org service <start|stop|status> [--name <instance>]\n\
Use when: controlling or checking the OS background service registered by `agentic-org setup` (use `agentic-org serve` instead for foreground or container runs).\n\n\
Subcommands:\n  \
start [--name <instance>]\n      \
Start the instance's registered OS service\n      \
Use when: starting a stopped host service managed by launchd/systemd.\n  \
stop [--name <instance>]\n      \
Stop the instance's registered OS service\n      \
Use when: cleanly shutting down the host background service for maintenance.\n  \
status [--name <instance>]\n      \
Show whether the instance's registered OS service is running\n      \
Use when: checking host service health before running organization or office commands.";

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    let (action, rest) = match arguments {
        [action, rest @ ..] => (action.as_str(), rest),
        [] => return Err(HELP.into()),
    };
    let action = match action {
        "start" => ServiceAction::Start,
        "stop" => ServiceAction::Stop,
        "status" => ServiceAction::Status,
        "--help" | "-h" => {
            println!("{HELP}");
            return Ok(());
        }
        _ => return Err(HELP.into()),
    };
    if matches!(rest, [flag] if flag == "--help" || flag == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    let options = crate::options(rest, &["--name"])?;
    let platform = crate::setup::platform(false)?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::service(
        options.get("--name").map(String::as_str),
        &*platform.service,
        action,
        &mut out,
    )
}

#[cfg(test)]
mod tests {
    fn run(arguments: &[&str]) -> Result<(), String> {
        super::run(&arguments.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn service_help_covers_every_subcommand_with_use_when() {
        for args in [
            vec!["--help"],
            vec!["start", "--help"],
            vec!["stop", "--help"],
            vec!["status", "--help"],
        ] {
            assert!(run(&args).is_ok(), "{args:?}");
        }
        assert!(
            super::HELP.matches("Use when:").count() >= 4,
            "expected command + 3 subcommand Use when cues:\n{}",
            super::HELP
        );
    }
}
