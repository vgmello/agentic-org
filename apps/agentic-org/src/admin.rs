//! `agentic-org admin reset [--name <instance>]`: after Touch ID or the login password at the
//! service host, a new administrator registration link; the previous one is cancelled.
//! `agentic-org admin tls set`: replaces an https instance's TLS pair
//! (`org_host::setup_flow::admin_tls_set`).
//!
//! On Linux the local authority is root (SIGN-33): `sudo agentic-org admin reset`, or, with
//! no `sudo` (a container), `agentic-org admin reset --user <service user>` run as root. Root
//! acts on that user's instance and writes nothing into its folders.
pub(crate) const HELP: &str = "agentic-org admin reset [--name <instance>] [--user <service \
user>]\n\
Use when: issuing a fresh one-time administrator registration or password-reset link after local \
OS confirmation (cancels any previous unused link).\n\
On a Mac: asks for Touch ID or your login password, then prints a new administrator \
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
        [tls, set, rest @ ..] if tls == "tls" && set == "set" => {
            if matches!(rest, [flag] if flag == "--help" || flag == "-h") {
                println!("{}", org_host::setup_flow::ADMIN_TLS_SET_HELP);
                return Ok(());
            }
            let options = crate::options(rest, &["--cert", "--key", "--login", "--name"])?;
            let required = |flag: &str| {
                options.get(flag).ok_or_else(|| {
                    format!(
                        "{flag} is required\n{}",
                        org_host::setup_flow::ADMIN_TLS_SET_HELP
                    )
                })
            };
            let (cert, key, login) = (
                required("--cert")?,
                required("--key")?,
                required("--login")?,
            );
            let mut out = std::io::stdout().lock();
            org_host::setup_flow::admin_tls_set(
                options.get("--name").map(String::as_str),
                std::path::Path::new(cert),
                std::path::Path::new(key),
                login,
                &mut out,
            )
        }
        [tls, flag] if tls == "tls" && matches!(flag.as_str(), "--help" | "-h") => {
            println!("{}", org_host::setup_flow::ADMIN_TLS_SET_HELP);
            Ok(())
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{HELP}\n\n{}", org_host::setup_flow::ADMIN_TLS_SET_HELP);
            Ok(())
        }
        _ => Err(format!(
            "{HELP}\n\n{}",
            org_host::setup_flow::ADMIN_TLS_SET_HELP
        )),
    }
}

#[cfg(test)]
mod tests {
    fn run(arguments: &[&str]) -> Result<(), String> {
        super::run(&arguments.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    /// `admin tls set` takes its files and login as options, each once, and nothing else: a
    /// missing one is named before any file is read or the service is asked.
    #[test]
    fn admin_tls_set_names_what_it_needs_before_doing_anything() {
        let missing = run(&["tls", "set", "--key", "/k.pem", "--login", "Owner"]).unwrap_err();
        assert!(missing.starts_with("--cert is required"), "{missing}");
        let missing = run(&["tls", "set", "--cert", "/c.pem", "--key", "/k.pem"]).unwrap_err();
        assert!(missing.starts_with("--login is required"), "{missing}");
        let unknown = run(&["tls", "set", "--password", "secret"]).unwrap_err();
        assert!(unknown.contains("unrecognized argument"), "{unknown}");
        assert!(!unknown.contains("secret"), "{unknown}");
        let twice = run(&["tls", "set", "--cert", "/a", "--cert", "/b"]).unwrap_err();
        assert!(twice.contains("more than once"), "{twice}");
        assert!(run(&["tls", "--help"]).is_ok());
        assert!(run(&["tls", "-h"]).is_ok());
        assert!(run(&["tls", "set", "--help"]).is_ok());
        assert!(run(&["tls"]).unwrap_err().contains("admin tls set"));
        assert!(super::HELP.contains("Use when:"), "{}", super::HELP);
        assert!(
            org_host::setup_flow::ADMIN_TLS_SET_HELP.contains("Use when:"),
            "{}",
            org_host::setup_flow::ADMIN_TLS_SET_HELP
        );
    }
}
