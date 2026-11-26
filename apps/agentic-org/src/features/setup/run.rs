//! `agentic-org setup [--name <instance>] [--port <port>]`: composes the platform adapters and
//! runs [`org_host::setup_flow`]. Satellite flags are refused before anything else.
//!
//! `agentic-org setup --non-interactive --config <file> [--name <instance>]` takes the whole
//! setup input from a strict JSON file ([`setup_flow::configuration`] documents it), asks
//! nothing, fetches nothing and prints no link. With `"service": "foreground"` in the file
//! nothing is registered or started: `agentic-org serve --setup <file>` runs that setup
//! when the instance is not set up yet and then serves it (`crate::host::configured`).
//!
//! Its own argument grammar is kept as it is: the order of its refusals (satellite flags,
//! then root, then the options) is a tested contract.

use std::io::{BufRead, IsTerminal, Write};

use org_host::setup_flow::{self, Request};

use crate::host;

pub(crate) const HELP: &str = "Set up this computer's instance and register its service:\n  \
agentic-org setup [--name <instance>] [--port <port>] [--federation-bind <IP:port>] [--federation-advertise <host,...>]\n\
Use when: initializing a new Agentic Org instance on this computer, registering its OS background service, or enabling HQ mTLS federation for remote satellites (do not use setup flags to enroll a satellite; run `setup` first, then `agentic-org office join`).\n\
Without --name the instance is \"default\" on port 7420; a named instance takes the first free port from 7421.\n\
Pass --federation-bind (for example 0.0.0.0:7443) to enable HQ mTLS federation for remote satellites;\n\
--federation-advertise adds comma-separated DNS/IP SANs (hostnames or IPs without ports) to HQ's federation cert.\n\
Without a terminal, from a file (service mode, data folder, credential directory, \
address, optional federation):\n  agentic-org setup --non-interactive --config <file> [--name <instance>]\n\
Use when: automating headless, CI, or container instance setup from a strict JSON config file.";

/// What a `setup` run did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SetUp {
    /// `--help`: the caller prints [`HELP`].
    Help,
    /// Setup ran; its report went to `out`.
    Done,
}

pub(crate) fn run(arguments: &[String], out: &mut dyn Write) -> Result<SetUp, String> {
    run_as(arguments, org_host::Config::refuse_root, out)
}

/// [`run`], with the check of who may own an instance and its service given by the caller
/// (the composition passes the host's root refusal; a test passes its own).
pub(super) fn run_as(
    arguments: &[String],
    may_run_the_service: impl FnOnce() -> Result<(), String>,
    out: &mut dyn Write,
) -> Result<SetUp, String> {
    // Never write a fake HQ or enroll a satellite from here.
    if arguments.iter().any(|arg| {
        matches!(arg.as_str(), "--hq" | "--satellite" | "--invitation")
            || arg.starts_with("--hq=")
            || arg.starts_with("--invitation=")
    }) {
        return Err("setup creates HQ only; use HQ's Add office flow for satellites".into());
    }
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        return Ok(SetUp::Help);
    }
    // Setup reserves the instance, makes its custody and registers a service that runs as
    // the user who ran it, and the service refuses root (ADR-0017 decision 10). So root is
    // refused here, before anything is read or made, with the service's own words. Nothing
    // above reads more than the arguments. (`admin reset` run by root for the service user
    // is another command and is not judged here.)
    may_run_the_service()?;
    let non_interactive = arguments.iter().any(|arg| arg == "--non-interactive");
    let arguments: Vec<String> = arguments
        .iter()
        .filter(|arg| *arg != "--non-interactive")
        .cloned()
        .collect();
    let options = crate::host::options(
        &arguments,
        &[
            "--name",
            "--port",
            "--config",
            "--federation-bind",
            "--federation-advertise",
        ],
    )?;
    match (non_interactive, options.get("--config")) {
        (true, Some(file)) => {
            if options.contains_key("--port") {
                return Err("--port is not used with --config: the address is in the file".into());
            }
            if options.contains_key("--federation-bind")
                || options.contains_key("--federation-advertise")
            {
                return Err(
                    "--federation-bind and --federation-advertise are not used with --config: \
                     set \"federation\" in the file"
                        .into(),
                );
            }
            return host::configured(
                std::path::Path::new(file),
                options.get("--name").map(String::as_str),
                out,
            )
            .map(|()| SetUp::Done);
        }
        (true, None) => return Err("--non-interactive needs --config <file>".into()),
        (false, Some(_)) => {
            return Err("--config is read only with --non-interactive".into());
        }
        (false, None) => {}
    }
    let port = options
        .get("--port")
        .map(|p| {
            p.parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or_else(|| format!("--port must be a number from 1 to 65535, not {p:?}"))
        })
        .transpose()?;
    let federation = parse_federation_flags(
        options.get("--federation-bind").map(String::as_str),
        options.get("--federation-advertise").map(String::as_str),
    )?;
    let platform = host::platform(false)?;
    let program = host::own_program()?;
    let mut ask = ask_port;
    let terminal = std::io::stdout().is_terminal();
    let interactive = std::io::stdin().is_terminal() && terminal;
    let request = Request {
        name: options.get("--name").cloned(),
        port,
        prompt: if interactive { Some(&mut ask) } else { None },
        named_port_base: host::named_port_base()?,
        program,
        env: host::service_environment()?,
        terminal,
        development_keys: host::development_keys()?,
        configuration: None,
        federation,
    };
    setup_flow::run(&platform, request, out).map(|_| SetUp::Done)
}

fn parse_federation_flags(
    bind: Option<&str>,
    advertise: Option<&str>,
) -> Result<Option<setup_flow::FederationConfig>, String> {
    match (bind, advertise) {
        (None, None) => Ok(None),
        (None, Some(_)) => {
            Err("--federation-advertise requires --federation-bind <IP:port>".into())
        }
        (Some(bind), adv) => {
            let entries: Vec<String> = match adv {
                None => Vec::new(),
                Some(raw) => {
                    if raw.trim().is_empty() {
                        return Err(
                            "--federation-advertise needs at least one hostname or IP address"
                                .into(),
                        );
                    }
                    raw.split(',').map(str::to_owned).collect()
                }
            };
            setup_flow::FederationConfig::parse(bind, entries).map(Some)
        }
    }
}

/// Asks for the port on the terminal: the question goes to standard error, so standard
/// output carries only setup's report.
fn ask_port(address: &str) -> Result<Option<u16>, String> {
    let mut err = std::io::stderr();
    write!(
        err,
        "Address: {address}  (Enter to accept, or type a port) "
    )
    .and_then(|()| err.flush())
    .map_err(|e| e.to_string())?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    match line.trim() {
        "" => Ok(None),
        port => port
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .map(Some)
            .ok_or_else(|| format!("{port:?} is not a port from 1 to 65535; nothing was saved")),
    }
}
