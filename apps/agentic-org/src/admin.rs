//! `agentic-org admin reset [--name <instance>]`: after Touch ID or the login password at the
//! service host, a new administrator registration link; the previous one is cancelled.
pub(crate) const HELP: &str = "agentic-org admin reset [--name <instance>]\nAsks for Touch ID \
or your login password, then prints a new administrator registration link. The previous link \
stops working.";

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [command, rest @ ..] if command == "reset" => {
            if matches!(rest, [flag] if flag == "--help" || flag == "-h") {
                println!("{HELP}");
                return Ok(());
            }
            let options = crate::options(rest, &["--name"])?;
            let mut out = std::io::stdout().lock();
            org_host::setup_flow::admin_reset(options.get("--name").map(String::as_str), &mut out)
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{HELP}");
            Ok(())
        }
        _ => Err(HELP.into()),
    }
}
