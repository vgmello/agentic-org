//! `agentic-org sign-in` (A5 / FC-173 / SIGN-40 / SES-8): browser-approved terminal sign-in
//! with optional `-- <org|team|person|agent|project ...>` command execution over the approved
//! grant-bound host channel.

use org_agent_cli::dispatch::{hint_for, OfficeTransport, TransportOffice};
use org_command::Refusal;
use org_host::setup_flow::ApprovedOfficeChannel;
use serde_json::Value;

struct GrantTransport(ApprovedOfficeChannel);

impl OfficeTransport for GrantTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.0
            .call_method(method, params)
            .map_err(|api| Refusal::new(api.code, api.message, hint_for(api.code)))
    }
}

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::SIGN_IN_HELP);
        return Ok(());
    }
    let (sign_in_args, command_args) = match arguments.iter().position(|a| a == "--") {
        Some(idx) => (&arguments[..idx], Some(&arguments[idx + 1..])),
        None => (arguments, None),
    };
    let options = crate::options(sign_in_args, &["--name", "--office"])?;
    let instance = options.get("--name").map(String::as_str);
    let office = options.get("--office").map(String::as_str);

    if let Some(cmd) = command_args {
        if cmd.is_empty() {
            return Err(format!(
                "missing command after `--`\n{}",
                org_host::setup_flow::SIGN_IN_HELP
            ));
        }
        let mut err = std::io::stderr().lock();
        let channel = org_host::setup_flow::open_for_command(instance, office, &mut err)?;
        let mut transport = TransportOffice(GrantTransport(channel));
        let output = org_agent_cli::execute_with_office("agentic-org", cmd, &mut transport);
        print!("{}", output.stdout);
        if output.exit_code == 0 {
            Ok(())
        } else {
            Err(format!("command exited with status {}", output.exit_code))
        }
    } else {
        let mut out = std::io::stdout().lock();
        org_host::setup_flow::sign_in_only(instance, office, &mut out)
    }
}

#[cfg(test)]
mod tests {
    fn run(args: &[&str]) -> Result<(), String> {
        super::run(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn sign_in_cli_validates_arguments_before_host_contact() {
        assert!(run(&["--help"]).is_ok());
        assert!(run(&["--unknown", "x"]).is_err());
        assert!(run(&["--"]).unwrap_err().contains("missing command"));
    }
}
