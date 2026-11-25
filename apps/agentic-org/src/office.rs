//! `agentic-org office list|restart|invite|invitations|approve|decline|cancel|remove|sync-status|join|remove-pending|re-enroll`.

use std::collections::BTreeMap;
use std::io::Read;

use org_host::setup_flow::OfficeInviteInput;

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", org_host::setup_flow::OFFICE_HELP);
            Ok(())
        }
        [sub, rest @ ..] if sub == "list" => run_list(rest),
        [sub, rest @ ..] if sub == "restart" => run_restart(rest),
        [sub, rest @ ..] if sub == "invite" => run_invite(rest),
        [sub, rest @ ..] if sub == "invitations" => run_invitations(rest),
        [sub, rest @ ..] if sub == "approve" => run_approve(rest),
        [sub, rest @ ..] if sub == "decline" => run_decline(rest),
        [sub, rest @ ..] if sub == "cancel" => run_cancel(rest),
        [sub, rest @ ..] if sub == "remove" => run_remove(rest),
        [sub, rest @ ..] if sub == "sync-status" => run_sync_status(rest),
        [sub, rest @ ..] if sub == "join" => run_join(rest),
        [sub, rest @ ..] if sub == "remove-pending" => run_remove_pending(rest),
        [sub, rest @ ..] if sub == "re-enroll" => run_re_enroll(rest),
        _ => Err(org_host::setup_flow::OFFICE_HELP.to_owned()),
    }
}

fn strip_flags(arguments: &[String]) -> Result<(Vec<String>, bool, bool, bool), String> {
    let mut json = false;
    let mut yes = false;
    let mut stdin_inv = false;
    let mut rest = Vec::with_capacity(arguments.len());
    for arg in arguments {
        match arg.as_str() {
            "--json" => {
                if json {
                    return Err("--json is given more than once".into());
                }
                json = true;
            }
            "--yes" => {
                if yes {
                    return Err("--yes is given more than once".into());
                }
                yes = true;
            }
            "--invitation-stdin" => {
                if stdin_inv {
                    return Err("--invitation-stdin is given more than once".into());
                }
                stdin_inv = true;
            }
            _ => rest.push(arg.clone()),
        }
    }
    Ok((rest, json, yes, stdin_inv))
}

fn read_invitation_token(
    file: Option<&str>,
    from_stdin: bool,
) -> Result<org_host::setup_flow::InvitationTokenSecret, String> {
    let mut raw = match (file, from_stdin) {
        (Some(_), true) => {
            return Err("specify either --invitation-file or --invitation-stdin, not both".into());
        }
        (None, false) => {
            return Err(format!(
                "--invitation-file <path> or --invitation-stdin is required\n{}",
                org_host::setup_flow::OFFICE_HELP
            ));
        }
        (Some(path), false) => {
            std::fs::read(path).map_err(|e| format!("cannot read invitation file `{path}`: {e}"))?
        }
        (None, true) => {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| format!("cannot read invitation token from stdin: {e}"))?;
            buf
        }
    };
    let secret = std::str::from_utf8(&raw)
        .map_err(|_| "invitation token must be valid UTF-8".to_owned())
        .and_then(|text| {
            org_host::setup_flow::InvitationTokenSecret::new(text.trim().to_owned())
                .map_err(|_| "invitation token cannot be empty".to_owned())
        });
    org_proto::zero_bytes(&mut raw);
    secret
}

fn strip_json_flag(arguments: &[String]) -> Result<(Vec<String>, bool), String> {
    let mut json = false;
    let mut rest = Vec::with_capacity(arguments.len());
    for arg in arguments {
        if arg == "--json" {
            if json {
                return Err("--json is given more than once".into());
            }
            json = true;
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((rest, json))
}

fn require_positional<'a>(
    arguments: &'a [String],
    label: &str,
) -> Result<(&'a str, &'a [String]), String> {
    let Some((first, tail)) = arguments.split_first() else {
        return Err(format!(
            "{label} is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        ));
    };
    if first.starts_with("--") {
        return Err(format!(
            "{label} is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        ));
    }
    Ok((first.as_str(), tail))
}

fn run_list(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (rest, json) = strip_json_flag(arguments)?;
    let options = crate::options(&rest, &["--login", "--org", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_list(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        json,
        &mut out,
    )
}

fn run_restart(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (office_id, tail) = require_positional(arguments, "<office_id>")?;
    let (rest, json) = strip_json_flag(tail)?;
    let options = crate::options(&rest, &["--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_restart(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        office_id,
        json,
        &mut out,
    )
}

fn parse_invite_options(
    arguments: &[String],
) -> Result<(BTreeMap<String, String>, Vec<String>), String> {
    let allowed = ["--org", "--ttl", "--re-enroll-office", "--login", "--name"];
    let mut options = BTreeMap::new();
    let mut addresses = Vec::new();
    let mut iter = arguments.iter();
    while let Some(flag) = iter.next() {
        if flag == "--address" {
            let value = iter
                .next()
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(|| "--address needs a value".to_owned())?;
            for piece in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                addresses.push(piece.to_owned());
            }
            continue;
        }
        if !allowed.contains(&flag.as_str()) {
            return Err(format!("unrecognized argument {flag:?}"));
        }
        let value = iter
            .next()
            .filter(|v| !v.starts_with("--"))
            .ok_or_else(|| format!("{flag} needs a value"))?;
        if options.insert(flag.clone(), value.clone()).is_some() {
            return Err(format!("{flag} is given more than once"));
        }
    }
    Ok((options, addresses))
}

fn run_invite(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (office_name, tail) = require_positional(arguments, "<office_name>")?;
    let (rest, json) = strip_json_flag(tail)?;
    let (options, addresses) = parse_invite_options(&rest)?;
    if addresses.is_empty() {
        return Err(format!(
            "--address <host:port> is required\n{}",
            org_host::setup_flow::OFFICE_HELP
        ));
    }
    let ttl_secs = options
        .get("--ttl")
        .map(|raw| {
            raw.parse::<u64>()
                .ok()
                .filter(|v| *v > 0)
                .ok_or_else(|| format!("--ttl must be a positive number of seconds, not {raw:?}"))
        })
        .transpose()?;
    let input = OfficeInviteInput {
        org_id: options.get("--org").map(String::as_str),
        office_name,
        hq_addresses: addresses,
        ttl_secs,
        existing_office_id: options.get("--re-enroll-office").map(String::as_str),
        json,
    };
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_invite(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        &input,
        &mut out,
    )
}

fn run_invitations(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (rest, json) = strip_json_flag(arguments)?;
    let options = crate::options(&rest, &["--org", "--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_invitations(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        json,
        &mut out,
    )
}

fn run_approve(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (invitation_id, tail) = require_positional(arguments, "<invitation_id>")?;
    let (rest, json) = strip_json_flag(tail)?;
    let options = crate::options(&rest, &["--code", "--org", "--login", "--name"])?;
    let code = options
        .get("--code")
        .ok_or_else(|| format!("--code is required\n{}", org_host::setup_flow::OFFICE_HELP))?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_approve(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        invitation_id,
        code,
        json,
        &mut out,
    )
}

fn run_decline(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (invitation_id, tail) = require_positional(arguments, "<invitation_id>")?;
    let (rest, json) = strip_json_flag(tail)?;
    let options = crate::options(&rest, &["--org", "--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_decline(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        invitation_id,
        json,
        &mut out,
    )
}

fn run_cancel(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (invitation_id, tail) = require_positional(arguments, "<invitation_id>")?;
    let (rest, json) = strip_json_flag(tail)?;
    let options = crate::options(&rest, &["--org", "--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_cancel(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        invitation_id,
        json,
        &mut out,
    )
}

fn run_remove(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (office_id, tail) = require_positional(arguments, "<office_id>")?;
    let (rest, json) = strip_json_flag(tail)?;
    let options = crate::options(&rest, &["--org", "--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_remove(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        office_id,
        json,
        &mut out,
    )
}

fn run_sync_status(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (rest, json) = strip_json_flag(arguments)?;
    let options = crate::options(&rest, &["--org", "--office", "--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_sync_status(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        options.get("--org").map(String::as_str),
        options.get("--office").map(String::as_str),
        json,
        &mut out,
    )
}

fn run_join(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (rest, json, yes, stdin_inv) = strip_flags(arguments)?;
    let options = crate::options(&rest, &["--login", "--name", "--invitation-file"])?;
    if !yes {
        return Err("--yes is required to confirm joining this host to the organization".into());
    }
    let token = read_invitation_token(
        options.get("--invitation-file").map(String::as_str),
        stdin_inv,
    )?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_join(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        token,
        yes,
        json,
        &mut out,
    )
}

fn run_remove_pending(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (office_id, tail) = require_positional(arguments, "<office_id>")?;
    let (rest, json) = strip_json_flag(tail)?;
    let options = crate::options(&rest, &["--login", "--name"])?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_remove_pending(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        office_id,
        json,
        &mut out,
    )
}

fn run_re_enroll(arguments: &[String]) -> Result<(), String> {
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{}", org_host::setup_flow::OFFICE_HELP);
        return Ok(());
    }
    let (office_id, tail) = require_positional(arguments, "<office_id>")?;
    let (rest, json, yes, stdin_inv) = strip_flags(tail)?;
    let options = crate::options(&rest, &["--login", "--name", "--invitation-file"])?;
    if !yes {
        return Err("--yes is required to confirm re-enrolling this satellite office".into());
    }
    let token = read_invitation_token(
        options.get("--invitation-file").map(String::as_str),
        stdin_inv,
    )?;
    let mut out = std::io::stdout().lock();
    org_host::setup_flow::office_re_enroll(
        options.get("--name").map(String::as_str),
        options.get("--login").map(String::as_str),
        office_id,
        token,
        yes,
        json,
        &mut out,
    )
}

#[cfg(test)]
mod tests {
    fn run(arguments: &[&str]) -> Result<(), String> {
        super::run(&arguments.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn office_cli_validates_arguments_before_host_contact() {
        for help in [
            vec!["--help"],
            vec!["list", "--help"],
            vec!["restart", "--help"],
            vec!["invite", "--help"],
            vec!["invitations", "--help"],
            vec!["approve", "--help"],
            vec!["decline", "--help"],
            vec!["cancel", "--help"],
            vec!["remove", "--help"],
            vec!["sync-status", "--help"],
            vec!["join", "--help"],
            vec!["remove-pending", "--help"],
            vec!["re-enroll", "--help"],
        ] {
            assert!(run(&help).is_ok(), "{help:?}");
        }
        let missing_office = run(&["restart", "--login", "Owner"]).unwrap_err();
        assert!(
            missing_office.starts_with("<office_id> is required"),
            "{missing_office}"
        );
        let missing_invite_name = run(&["invite", "--address", "127.0.0.1:7443"]).unwrap_err();
        assert!(
            missing_invite_name.starts_with("<office_name> is required"),
            "{missing_invite_name}"
        );
        let missing_address = run(&["invite", "Berlin"]).unwrap_err();
        assert!(
            missing_address.starts_with("--address <host:port> is required"),
            "{missing_address}"
        );
        let bad_ttl = run(&[
            "invite",
            "Berlin",
            "--address",
            "127.0.0.1:7443",
            "--ttl",
            "0",
        ])
        .unwrap_err();
        assert!(bad_ttl.contains("positive number"), "{bad_ttl}");
        let missing_inv_id = run(&["approve", "--code", "123456"]).unwrap_err();
        assert!(
            missing_inv_id.starts_with("<invitation_id> is required"),
            "{missing_inv_id}"
        );
        let missing_code = run(&["approve", "inv-1"]).unwrap_err();
        assert!(
            missing_code.starts_with("--code is required"),
            "{missing_code}"
        );
        let missing_decline_id = run(&["decline"]).unwrap_err();
        assert!(
            missing_decline_id.starts_with("<invitation_id> is required"),
            "{missing_decline_id}"
        );
        let missing_cancel_id = run(&["cancel"]).unwrap_err();
        assert!(
            missing_cancel_id.starts_with("<invitation_id> is required"),
            "{missing_cancel_id}"
        );
        let missing_remove_id = run(&["remove"]).unwrap_err();
        assert!(
            missing_remove_id.starts_with("<office_id> is required"),
            "{missing_remove_id}"
        );
        let no_yes = run(&["join", "--login", "Owner", "--invitation-stdin"]).unwrap_err();
        assert!(no_yes.contains("--yes is required"), "{no_yes}");
        let no_inv = run(&["join", "--login", "Owner", "--yes"]).unwrap_err();
        assert!(no_inv.contains("--invitation-file"), "{no_inv}");
        let no_re_office = run(&["re-enroll", "--login", "Owner"]).unwrap_err();
        assert!(
            no_re_office.starts_with("<office_id> is required"),
            "{no_re_office}"
        );
        let dup = run(&["list", "--login", "Owner", "--json", "--json"]).unwrap_err();
        assert!(dup.contains("more than once"), "{dup}");
    }
}
