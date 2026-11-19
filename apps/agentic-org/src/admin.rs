//! `agentic-org admin reset [--name <instance>]`: after Touch ID or the login password at the
//! service host, a new administrator registration link; the previous one is cancelled.
//!
//! On Linux the local authority is root (SIGN-33): `sudo agentic-org admin reset`, or, with
//! no `sudo` (a container), `agentic-org admin reset --user <service user>` run as root. Root
//! acts on that user's instance and writes nothing into its folders.
pub(crate) const HELP: &str = "agentic-org admin reset [--name <instance>] [--user <service \
user>]\nOn a Mac: asks for Touch ID or your login password, then prints a new administrator \
registration link. On Linux: run it as root (sudo, or --user <service user> where there is no \
sudo, as in a container: docker exec -it -u 0 <container> agentic-org admin reset --user \
<service user>). For an instance set up with XDG_DATA_HOME, XDG_CACHE_HOME or \
XDG_STATE_HOME set, give root the same values: sudo env XDG_DATA_HOME=<dir> \
XDG_CACHE_HOME=<dir> agentic-org admin reset --user <service user>. The previous link stops \
working.";

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [command, rest @ ..] if command == "reset" => {
            if matches!(rest, [flag] if flag == "--help" || flag == "-h") {
                println!("{HELP}");
                return Ok(());
            }
            let options = crate::options(rest, &["--name", "--user"])?;
            use std::io::IsTerminal;
            let terminal = std::io::stdout().is_terminal();
            let mut out = std::io::stdout().lock();
            org_host::setup_flow::admin_reset(
                options.get("--name").map(String::as_str),
                options.get("--user").map(String::as_str),
                terminal,
                &mut out,
            )
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{HELP}");
            Ok(())
        }
        _ => Err(HELP.into()),
    }
}
