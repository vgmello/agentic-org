//! `org-agent`: Fast CLI for agent-to-agent messaging over the local spool.
//!
//! Coupling metrics (per `msg/architecture-rules.md`):
//! - Efferent coupling (`Ce`): 1 (`org_msg`)
//! - Afferent coupling (`Ca`): 0 (Binary entrypoint)
//! - Instability (`I`): 1.00

use std::collections::BTreeMap;
use std::env;
use std::io::{self, Read};
use std::process::ExitCode;
use std::time::Duration;

use org_msg::{
    json_quote, parse_duration_arg, request_interrupt, validate_agent_id, Envelope, MetaValue,
    Spool, SpoolError,
};

const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 1;
const EXIT_TIMEOUT: u8 = 2;
const EXIT_UNKNOWN_RECIPIENT: u8 = 3;
const EXIT_IO: u8 = 4;
const EXIT_INTERRUPTED: u8 = 130;

extern "C" {
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
}

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

extern "C" fn on_termination_signal(_sig: i32) {
    // Async-signal-safe: sets an AtomicBool so any blocking wait unwinds cleanly
    // and runs `Drop` on active `WakeListener` guards on the main thread.
    request_interrupt();
}

fn install_signal_handlers() {
    unsafe {
        signal(SIGINT, on_termination_signal);
        signal(SIGTERM, on_termination_signal);
    }
}

enum CliError {
    Usage(String),
    Spool(SpoolError),
}

impl From<SpoolError> for CliError {
    fn from(value: SpoolError) -> Self {
        Self::Spool(value)
    }
}

fn resolve_identity(as_flag: Option<&str>) -> Result<String, CliError> {
    let raw = match as_flag {
        Some(val) if !val.trim().is_empty() => val.trim().to_string(),
        _ => env::var("ORG_AGENT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CliError::Usage(
                    "identity not set: pass --as <agent-id> or set $ORG_AGENT".into(),
                )
            })?,
    };
    validate_agent_id(&raw, "identity").map_err(CliError::Usage)?;
    Ok(raw)
}

fn read_body_arg(positional: &[String]) -> Result<String, CliError> {
    if positional.is_empty() {
        return Err(CliError::Usage(
            "missing message body (pass <body | ->)".into(),
        ));
    }
    let joined = positional.join(" ");
    if joined == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Spool(SpoolError::Io(e)))?;
        Ok(buf)
    } else {
        Ok(joined)
    }
}

fn parse_meta_kv(raw: &str) -> Result<(String, MetaValue), CliError> {
    let Some((k, v)) = raw.split_once('=') else {
        return Err(CliError::Usage(format!(
            "invalid --meta '{raw}' (expected key=value)"
        )));
    };
    let key = k.trim();
    if key.is_empty() {
        return Err(CliError::Usage("--meta key must not be empty".into()));
    }
    Ok((key.to_string(), MetaValue::String(v.to_string())))
}

fn print_envelope(env: &Envelope, json: bool) -> Result<(), CliError> {
    if json {
        let s = env.to_json_string().map_err(CliError::Usage)?;
        println!("{s}");
    } else {
        let subj = env
            .subject
            .as_deref()
            .map(|s| format!(" · {s}"))
            .unwrap_or_default();
        let reply = env
            .reply_to
            .as_deref()
            .map(|r| format!(" (reply_to: {r})"))
            .unwrap_or_default();
        println!(
            "{} [{}] {} -> {}{}{}\n{}",
            env.id,
            env.kind.as_str(),
            env.from,
            env.to,
            subj,
            reply,
            env.body
        );
    }
    Ok(())
}

fn print_usage() {
    eprintln!(
        "\
Usage:
  org-agent whoami [--as <agent>] [--json]
  org-agent msg register [<agent>] [--as <agent>] [--json]
  org-agent msg send --to <agent> [--subject <s>] [--meta k=v] [--as <agent>] [--json] <body | ->
  org-agent msg ask  --to <agent> [--timeout 120s] [--subject <s>] [--meta k=v] [--as <agent>] [--json] <body | ->
  org-agent msg inbox [--json] [--all] [--as <agent>]
  org-agent msg read  <id> | --next [--json] [--as <agent>]
  org-agent msg reply <id> [--subject <s>] [--meta k=v] [--as <agent>] [--json] <body | ->
  org-agent msg wait  [--timeout 60s] [--from <agent>] [--as <agent>] [--json]"
    );
}

fn run(args: Vec<String>) -> Result<(), CliError> {
    let mut as_agent: Option<String> = None;
    let mut json_flag = false;
    let mut filtered = Vec::with_capacity(args.len());
    let mut after_double_dash = false;

    let mut i = 0usize;
    while i < args.len() {
        if after_double_dash {
            filtered.push(args[i].clone());
            i += 1;
            continue;
        }
        match args[i].as_str() {
            "--" => {
                after_double_dash = true;
                filtered.push("--".to_string());
            }
            "--as" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| CliError::Usage("missing value for --as".into()))?;
                as_agent = Some(val.clone());
            }
            "--json" => {
                json_flag = true;
            }
            "-h" | "--help" | "help" if filtered.is_empty() => {
                print_usage();
                return Ok(());
            }
            other => filtered.push(other.to_string()),
        }
        i += 1;
    }

    let Some(first) = filtered.first().map(|s| s.as_str()) else {
        print_usage();
        return Err(CliError::Usage("missing command".into()));
    };

    if first == "whoami" {
        let me = resolve_identity(as_agent.as_deref())?;
        if json_flag {
            println!("{{\"agent\":{}}}", json_quote(&me));
        } else {
            println!("{me}");
        }
        return Ok(());
    }

    if first != "msg" {
        return Err(CliError::Usage(format!(
            "unknown command group '{first}' (expected 'msg' or 'whoami')"
        )));
    }

    let Some(sub) = filtered.get(1).map(|s| s.as_str()) else {
        print_usage();
        return Err(CliError::Usage("missing msg subcommand".into()));
    };
    let sub_args = &filtered[2..];
    let spool = Spool::open_from_env()?;

    match sub {
        "register" => {
            let target = match sub_args.first() {
                Some(a) if !a.starts_with('-') => a.clone(),
                None => resolve_identity(as_agent.as_deref())?,
                Some(flag) => {
                    return Err(CliError::Usage(format!("unexpected flag '{flag}'")))
                }
            };
            spool.register(&target)?;
            if json_flag {
                println!("{{\"registered\":{}}}", json_quote(&target));
            } else {
                println!("{target}");
            }
            Ok(())
        }

        "deregister" => {
            let Some(target) = sub_args.first() else {
                return Err(CliError::Usage(
                    "usage: org-agent msg deregister <agent>".into(),
                ));
            };
            spool.deregister(target)?;
            if json_flag {
                println!("{{\"deregistered\":{}}}", json_quote(target));
            } else {
                println!("{target}");
            }
            Ok(())
        }

        "send" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut to: Option<String> = None;
            let mut subject: Option<String> = None;
            let mut meta = BTreeMap::new();
            let mut pos = Vec::new();
            let mut after_double_dash = false;

            let mut j = 0usize;
            while j < sub_args.len() {
                let arg = &sub_args[j];
                if after_double_dash {
                    pos.push(arg.clone());
                    j += 1;
                    continue;
                }
                match arg.as_str() {
                    "--" => after_double_dash = true,
                    "--to" => {
                        j += 1;
                        to = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --to".into()))?
                                .clone(),
                        );
                    }
                    "--subject" => {
                        j += 1;
                        subject = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| {
                                    CliError::Usage("missing value for --subject".into())
                                })?
                                .clone(),
                        );
                    }
                    "--meta" => {
                        j += 1;
                        let kv = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --meta".into()))?;
                        let (k, v) = parse_meta_kv(kv)?;
                        meta.insert(k, v);
                    }
                    other if other.starts_with('-') && other != "-" => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => pos.push(other.to_string()),
                }
                j += 1;
            }

            let recipient =
                to.ok_or_else(|| CliError::Usage("missing required --to <agent>".into()))?;
            let body = read_body_arg(&pos)?;

            // Decision 0004 §3: check registration BEFORE writing to outbox.
            if !spool.is_registered(&recipient) {
                return Err(CliError::Spool(SpoolError::UnknownRecipient(recipient)));
            }

            let env =
                Envelope::new_msg(me, recipient, subject, body, meta).map_err(CliError::Usage)?;
            spool.enqueue(&env)?;
            spool.nudge_relay();

            if json_flag {
                println!("{{\"id\":{}}}", json_quote(&env.id));
            } else {
                println!("{}", env.id);
            }
            Ok(())
        }

        "ask" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut to: Option<String> = None;
            let mut subject: Option<String> = None;
            let mut timeout = Duration::from_secs(120);
            let mut meta = BTreeMap::new();
            let mut pos = Vec::new();
            let mut after_double_dash = false;

            let mut j = 0usize;
            while j < sub_args.len() {
                let arg = &sub_args[j];
                if after_double_dash {
                    pos.push(arg.clone());
                    j += 1;
                    continue;
                }
                match arg.as_str() {
                    "--" => after_double_dash = true,
                    "--to" => {
                        j += 1;
                        to = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --to".into()))?
                                .clone(),
                        );
                    }
                    "--subject" => {
                        j += 1;
                        subject = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| {
                                    CliError::Usage("missing value for --subject".into())
                                })?
                                .clone(),
                        );
                    }
                    "--timeout" => {
                        j += 1;
                        let raw = sub_args.get(j).ok_or_else(|| {
                            CliError::Usage("missing value for --timeout".into())
                        })?;
                        timeout = parse_duration_arg(raw).map_err(CliError::Usage)?;
                    }
                    "--meta" => {
                        j += 1;
                        let kv = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --meta".into()))?;
                        let (k, v) = parse_meta_kv(kv)?;
                        meta.insert(k, v);
                    }
                    other if other.starts_with('-') && other != "-" => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => pos.push(other.to_string()),
                }
                j += 1;
            }

            let recipient =
                to.ok_or_else(|| CliError::Usage("missing required --to <agent>".into()))?;
            let body = read_body_arg(&pos)?;

            // Decision 0004 §3: check registration BEFORE writing to outbox.
            if !spool.is_registered(&recipient) {
                return Err(CliError::Spool(SpoolError::UnknownRecipient(recipient)));
            }

            // Ensure caller's own inbox & wake listener exist before sending request.
            spool.register(&me)?;
            let listener = spool.bind_wake_listener(&me)?;

            let req = Envelope::new_request(&me, recipient, subject, body, meta, timeout)
                .map_err(CliError::Usage)?;
            spool.enqueue(&req)?;
            spool.nudge_relay();

            let reply = spool.wait_for_reply_with_listener(&me, &req.id, &listener, timeout)?;
            if json_flag {
                let s = reply.to_json_string().map_err(CliError::Usage)?;
                println!("{s}");
            } else {
                println!("{}", reply.body);
            }
            Ok(())
        }

        "inbox" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut include_all = false;
            for arg in sub_args {
                match arg.as_str() {
                    "--all" => include_all = true,
                    other => return Err(CliError::Usage(format!("unknown flag '{other}'"))),
                }
            }
            let list = spool.list_inbox(&me, include_all)?;
            if json_flag {
                let s = Envelope::list_to_json_string(&list).map_err(CliError::Usage)?;
                println!("{s}");
            } else {
                for env in &list {
                    let subj = env.subject.as_deref().unwrap_or("-");
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        env.id,
                        env.kind.as_str(),
                        env.from,
                         env.created_at,
                        subj
                    );
                }
            }
            Ok(())
        }

        "read" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let Some(target) = sub_args.first() else {
                return Err(CliError::Usage(
                    "usage: org-agent msg read <id> | --next".into(),
                ));
            };
            if target == "--next" {
                match spool.claim_next(&me)? {
                    Some(env) => print_envelope(&env, json_flag)?,
                    None => {
                        if json_flag {
                            println!("null");
                        }
                    }
                }
            } else {
                let env = spool.claim(&me, target)?;
                print_envelope(&env, json_flag)?;
            }
            Ok(())
        }

        "reply" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut subject: Option<String> = None;
            let mut meta = BTreeMap::new();
            let mut pos = Vec::new();
            let mut after_double_dash = false;

            let mut j = 0usize;
            while j < sub_args.len() {
                let arg = &sub_args[j];
                if after_double_dash {
                    pos.push(arg.clone());
                    j += 1;
                    continue;
                }
                match arg.as_str() {
                    "--" => after_double_dash = true,
                    "--subject" => {
                        j += 1;
                        subject = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| {
                                    CliError::Usage("missing value for --subject".into())
                                })?
                                .clone(),
                        );
                    }
                    "--meta" => {
                        j += 1;
                        let kv = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --meta".into()))?;
                        let (k, v) = parse_meta_kv(kv)?;
                        meta.insert(k, v);
                    }
                    other if other.starts_with('-') && other != "-" => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => pos.push(other.to_string()),
                }
                j += 1;
            }

            let Some(orig_id) = pos.first().cloned() else {
                return Err(CliError::Usage(
                    "usage: org-agent msg reply <id> <body | ->".into(),
                ));
            };
            let body = read_body_arg(&pos[1..])?;
            let orig = spool.find_for_reply(&me, &orig_id)?;

            if !spool.is_registered(&orig.from) {
                return Err(CliError::Spool(SpoolError::UnknownRecipient(
                    orig.from.clone(),
                )));
            }

            let reply = orig
                .new_reply(me, subject, body, meta)
                .map_err(CliError::Usage)?;
            spool.enqueue(&reply)?;
            spool.nudge_relay();

            if json_flag {
                println!("{{\"id\":{}}}", json_quote(&reply.id));
            } else {
                println!("{}", reply.id);
            }
            Ok(())
        }

        "wait" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut timeout = Duration::from_secs(60);
            let mut from_filter: Option<String> = None;

            let mut j = 0usize;
            while j < sub_args.len() {
                match sub_args[j].as_str() {
                    "--timeout" => {
                        j += 1;
                        let raw = sub_args.get(j).ok_or_else(|| {
                            CliError::Usage("missing value for --timeout".into())
                        })?;
                        timeout = parse_duration_arg(raw).map_err(CliError::Usage)?;
                    }
                    "--from" => {
                        j += 1;
                        from_filter = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --from".into()))?
                                .clone(),
                        );
                    }
                    other => return Err(CliError::Usage(format!("unknown flag '{other}'"))),
                }
                j += 1;
            }

            let env = spool.wait_for_inbox(&me, from_filter.as_deref(), timeout)?;
            print_envelope(&env, json_flag)?;
            Ok(())
        }

        other => Err(CliError::Usage(format!("unknown msg subcommand '{other}'"))),
    }
}

fn main() -> ExitCode {
    install_signal_handlers();
    let args: Vec<String> = env::args().skip(1).collect();
    match run(args) {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(CliError::Usage(msg)) => {
            eprintln!("org-agent: {msg}");
            ExitCode::from(EXIT_USAGE)
        }
        Err(CliError::Spool(SpoolError::InvalidInput(msg))) => {
            eprintln!("org-agent: {msg}");
            ExitCode::from(EXIT_USAGE)
        }
        Err(CliError::Spool(SpoolError::NotFound(msg))) => {
            eprintln!("org-agent: {msg}");
            ExitCode::from(EXIT_USAGE)
        }
        Err(CliError::Spool(SpoolError::Timeout(msg))) => {
            eprintln!("org-agent: {msg}");
            ExitCode::from(EXIT_TIMEOUT)
        }
        Err(CliError::Spool(SpoolError::UnknownRecipient(agent))) => {
            eprintln!(
                "org-agent: unknown recipient {agent} (register it with: org-agent msg register {agent})"
            );
            ExitCode::from(EXIT_UNKNOWN_RECIPIENT)
        }
        Err(CliError::Spool(SpoolError::Io(err))) => {
            eprintln!("org-agent: {err}");
            ExitCode::from(EXIT_IO)
        }
        Err(CliError::Spool(SpoolError::Interrupted)) => ExitCode::from(EXIT_INTERRUPTED),
    }
}
