//! `agentic-org service start|stop|status [--name <instance>]`: the registered service of an
//! instance setup created, through the same platform adapter setup used.
use org_host::setup_flow::ServiceAction;

pub(crate) const HELP: &str = "agentic-org service <start|stop|status> [--name <instance>]";

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
    let options = crate::options(rest, &["--name"])?;
    let platform = crate::setup::platform()?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::service(
        options.get("--name").map(String::as_str),
        &*platform.service,
        action,
        &mut out,
    )
}
