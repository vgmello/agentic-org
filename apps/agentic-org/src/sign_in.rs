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

struct LazyGrantTransport {
    instance: Option<String>,
    office: Option<String>,
    channel: Option<ApprovedOfficeChannel>,
}

impl LazyGrantTransport {
    fn new(instance: Option<String>, office: Option<String>) -> Self {
        Self {
            instance,
            office,
            channel: None,
        }
    }

    fn channel_mut(&mut self) -> Result<&mut ApprovedOfficeChannel, Refusal> {
        if self.channel.is_none() {
            let mut err = std::io::stderr().lock();
            let opened = org_host::setup_flow::open_for_command(
                self.instance.as_deref(),
                self.office.as_deref(),
                &mut err,
            )
            .map_err(|msg| {
                Refusal::new(
                    org_proto::ErrorCode::Unavailable,
                    msg,
                    "Run `agentic-org sign-in` or check `--name` / `--office`.",
                )
            })?;
            self.channel = Some(opened);
        }
        Ok(self.channel.as_mut().expect("channel initialized"))
    }
}

impl OfficeTransport for LazyGrantTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.channel_mut()?
            .call_method(method, params)
            .map_err(|api| Refusal::new(api.code, api.message, hint_for(api.code)))
    }
}

struct OfficeCommandSelectors {
    instance: Option<String>,
    office: Option<String>,
    forwarded: Vec<String>,
}

fn set_selector(slot: &mut Option<String>, flag: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{flag} needs a value"));
    }
    if slot.replace(value.to_owned()).is_some() {
        return Err(format!("{flag} is given more than once"));
    }
    Ok(())
}

fn flag_takes_value(forwarded: &[String], flag_name: &str) -> bool {
    if matches!(
        flag_name,
        "format"
            | "fields"
            | "limit"
            | "cursor"
            | "json"
            | "input-file"
            | "input-format"
            | "idempotency-key"
            | "if-match"
    ) {
        return true;
    }
    let (resource, verb) = match forwarded {
        [a, b, v, ..] if a == "task" && b == "note" => ("task-note", v.as_str()),
        [r, v, ..] => (r.as_str(), v.as_str()),
        _ => return false,
    };
    let table = org_agent_cli::table::runtime_table();
    let Some(cmd) = table.find(resource, verb) else {
        return false;
    };
    cmd.input
        .iter()
        .any(|f| f.is_flag() && f.flag() == flag_name && f.ty != org_command::Type::Boolean)
}

fn command_accepts_flag(forwarded: &[String], flag_name: &str) -> bool {
    let (resource, verb) = match forwarded {
        [a, b, v, ..] if a == "task" && b == "note" => ("task-note", v.as_str()),
        [r, v, ..] => (r.as_str(), v.as_str()),
        _ => return false,
    };
    org_agent_cli::table::runtime_table()
        .find(resource, verb)
        .is_some_and(|cmd| {
            cmd.input
                .iter()
                .any(|f| f.is_flag() && f.flag() == flag_name)
        })
}

fn try_consume_selector<'a, I: Iterator<Item = &'a String>>(
    arg: &str,
    forwarded: &[String],
    iter: &mut I,
    instance: &mut Option<String>,
    office: &mut Option<String>,
) -> Result<bool, String> {
    let name_is_command_flag = command_accepts_flag(forwarded, "name");
    if let Some(val) = arg.strip_prefix("--instance=") {
        set_selector(instance, "--instance", val)?;
        return Ok(true);
    }
    if !name_is_command_flag {
        if let Some(val) = arg.strip_prefix("--name=") {
            set_selector(instance, "--name", val)?;
            return Ok(true);
        }
    }
    if let Some(val) = arg.strip_prefix("--office=") {
        set_selector(office, "--office", val)?;
        return Ok(true);
    }
    if arg == "--instance" || (!name_is_command_flag && arg == "--name") || arg == "--office" {
        let value = iter
            .next()
            .filter(|v| !v.starts_with("--"))
            .ok_or_else(|| format!("{arg} needs a value"))?;
        let slot = if arg == "--office" { office } else { instance };
        set_selector(slot, arg, value)?;
        return Ok(true);
    }
    Ok(false)
}

fn split_office_selectors(args: &[String]) -> Result<OfficeCommandSelectors, String> {
    let mut instance = None;
    let mut office = None;
    let mut forwarded = Vec::with_capacity(args.len());
    let mut iter = args.iter();
    let mut expect_value = false;
    let mut after_terminator = false;
    while let Some(arg) = iter.next() {
        if expect_value {
            forwarded.push(arg.clone());
            expect_value = false;
            continue;
        }
        if after_terminator {
            forwarded.push(arg.clone());
            continue;
        }
        if arg == "--" {
            after_terminator = true;
            forwarded.push(arg.clone());
            continue;
        }
        if try_consume_selector(arg, &forwarded, &mut iter, &mut instance, &mut office)? {
            continue;
        }
        if let Some(flag_name) = arg.strip_prefix("--") {
            if !flag_name.contains('=') {
                expect_value = flag_takes_value(&forwarded, flag_name);
            }
        }
        forwarded.push(arg.clone());
    }
    Ok(OfficeCommandSelectors {
        instance,
        office,
        forwarded,
    })
}

pub(crate) fn run_office_command(cmd: &[String]) -> Result<(), String> {
    let selectors = split_office_selectors(cmd)?;
    let mut transport = TransportOffice(LazyGrantTransport::new(
        selectors.instance,
        selectors.office,
    ));
    let output =
        org_agent_cli::execute_with_office("agentic-org", &selectors.forwarded, &mut transport);
    print!("{}", output.stdout);
    if output.exit_code == 0 {
        Ok(())
    } else {
        Err(format!("command exited with status {}", output.exit_code))
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

    #[test]
    fn office_command_help_and_selectors_work_without_host_contact() {
        let help_args = vec!["task".to_owned(), "--help".to_owned()];
        assert!(super::run_office_command(&help_args).is_ok());
        let parsed = super::split_office_selectors(&[
            "task".to_owned(),
            "list".to_owned(),
            "--name".to_owned(),
            "hq".to_owned(),
            "--office".to_owned(),
            "A".to_owned(),
        ])
        .unwrap();
        assert_eq!(parsed.instance.as_deref(), Some("hq"));
        assert_eq!(parsed.office.as_deref(), Some("A"));
        assert_eq!(parsed.forwarded, vec!["task", "list"]);

        let eq = super::split_office_selectors(&[
            "task".to_owned(),
            "create".to_owned(),
            "--name=hq".to_owned(),
            "--office=B".to_owned(),
            "--title".to_owned(),
            "--name".to_owned(),
        ])
        .unwrap();
        assert_eq!(eq.instance.as_deref(), Some("hq"));
        assert_eq!(eq.office.as_deref(), Some("B"));
        assert_eq!(eq.forwarded, vec!["task", "create", "--title", "--name"]);

        let after_bool = super::split_office_selectors(&[
            "checklist".to_owned(),
            "add".to_owned(),
            "--task".to_owned(),
            "T-A1".to_owned(),
            "--text".to_owned(),
            "Tests".to_owned(),
            "--required".to_owned(),
            "--office".to_owned(),
            "A".to_owned(),
        ])
        .unwrap();
        assert_eq!(after_bool.office.as_deref(), Some("A"));
        assert_eq!(
            after_bool.forwarded,
            vec![
                "checklist",
                "add",
                "--task",
                "T-A1",
                "--text",
                "Tests",
                "--required"
            ]
        );

        let after_undone = super::split_office_selectors(&[
            "checklist".to_owned(),
            "check".to_owned(),
            "--task".to_owned(),
            "T-A1".to_owned(),
            "--step-id".to_owned(),
            "step".to_owned(),
            "--undone".to_owned(),
            "--name=hq".to_owned(),
        ])
        .unwrap();
        assert_eq!(after_undone.instance.as_deref(), Some("hq"));

        // Subcommands that define their own `--name` flag (like `hr hire` and `team create`)
        // must keep `--name` in forwarded args while `--instance` selects the host instance.
        let hire_with_name = super::split_office_selectors(&[
            "hr".to_owned(),
            "hire".to_owned(),
            "--name".to_owned(),
            "Worker EU".to_owned(),
            "--instance".to_owned(),
            "hq".to_owned(),
        ])
        .unwrap();
        assert_eq!(hire_with_name.instance.as_deref(), Some("hq"));
        assert_eq!(
            hire_with_name.forwarded,
            vec!["hr", "hire", "--name", "Worker EU"]
        );

        let dash_val = super::split_office_selectors(&[
            "task".to_owned(),
            "create".to_owned(),
            "--title".to_owned(),
            "--name".to_owned(),
            "--office".to_owned(),
            "A".to_owned(),
        ])
        .unwrap();
        assert_eq!(dash_val.office.as_deref(), Some("A"));
        assert_eq!(
            dash_val.forwarded,
            vec!["task", "create", "--title", "--name"]
        );

        let term_val = super::split_office_selectors(&[
            "task".to_owned(),
            "create".to_owned(),
            "--title".to_owned(),
            "--".to_owned(),
            "--office".to_owned(),
            "A".to_owned(),
        ])
        .unwrap();
        assert_eq!(term_val.office.as_deref(), Some("A"));
        assert_eq!(term_val.forwarded, vec!["task", "create", "--title", "--"]);
    }
}
