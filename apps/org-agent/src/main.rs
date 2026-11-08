//! `org-agent`: Fast CLI for agent messaging, directory (`who`), status (`status`),
//! and message triage (`defer`, `done`, `notify`, `due`) over `org-spool`.
//!
//! Also supports `org-agent relay <run|status>` and `argv[0]` dispatch when invoked as `org-relay`.

use std::collections::BTreeMap;
use std::env;
use std::io::{self, Read};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use org_spool::{
    check_wait_timeout, format_iso8601_utc, json_quote, parse_duration_arg, request_interrupt,
    sanitize_inline_text, validate_agent_id, DirectoryCard, Envelope, MemberProfile,
    MessagePriority, MetaValue, NotifyOptions, RefEntry, RelayOutcome, Spool, SpoolError,
    TeamEntry, OWNER_ID,
};

const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 1;
const EXIT_TIMEOUT: u8 = 2;
const EXIT_UNKNOWN_RECIPIENT: u8 = 3;
const EXIT_IO: u8 = 4;
const EXIT_NOTIFY_NORMAL: u8 = 10;
const EXIT_NOTIFY_URGENT: u8 = 11;
const EXIT_INTERRUPTED: u8 = 130;

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

extern "C" {
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
}

extern "C" fn on_termination_signal(_sig: i32) {
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

fn run_relay_loop(spool: &Spool, once: bool) -> Result<u8, CliError> {
    match spool.run_relay(once)? {
        RelayOutcome::AlreadyRunning => {
            eprintln!("org-relay: another relay instance is already running");
            Ok(EXIT_USAGE)
        }
        RelayOutcome::Once(summary) => {
            println!("delivered={} dead={}", summary.delivered, summary.dead);
            Ok(EXIT_OK)
        }
        RelayOutcome::Stopped => Ok(EXIT_OK),
    }
}

fn run_relay_status(spool: &Spool, json: bool) -> Result<u8, CliError> {
    let (running, pid) = spool.is_relay_running();
    let st = spool.status()?;
    if json {
        let pid_str = pid.map(|p| p.to_string()).unwrap_or_else(|| "null".into());
        let agents_json: Vec<String> = st.registered_agents.iter().map(|a| json_quote(a)).collect();
        println!(
            "{{\"running\":{running},\"pid\":{pid_str},\"outbox_new\":{},\"dead\":{},\"registered_agents\":[{}]}}",
            st.outbox_new,
            st.dead,
            agents_json.join(",")
        );
    } else {
        let state = match (running, pid) {
            (true, Some(p)) => format!("running (pid {p})"),
            (true, None) => "running".to_string(),
            (false, _) => "stopped".to_string(),
        };
        println!("relay: {state}");
        println!("root: {}", st.root.display());
        println!("outbox/new: {}", st.outbox_new);
        println!("dead: {}", st.dead);
        println!(
            "registered: {}",
            if st.registered_agents.is_empty() {
                "(none)".to_string()
            } else {
                st.registered_agents.join(", ")
            }
        );
    }
    Ok(EXIT_OK)
}

fn run_relay_subcommand(spool: &Spool, args: &[String], json_flag: bool) -> Result<u8, CliError> {
    let Some(sub) = args.first().map(|s| s.as_str()) else {
        return Err(CliError::Usage(
            "usage: org-agent relay <run [--once] | status [--json]>".into(),
        ));
    };
    match sub {
        "-h" | "--help" | "help" => {
            eprintln!("Usage:\n  org-relay run [--once]\n  org-relay status [--json]");
            Ok(EXIT_OK)
        }
        "run" => {
            let mut once = false;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--once" => once = true,
                    other => {
                        return Err(CliError::Usage(format!("unknown relay run flag '{other}'")))
                    }
                }
            }
            run_relay_loop(spool, once)
        }
        "status" => {
            let mut json = json_flag;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--json" => json = true,
                    other => {
                        return Err(CliError::Usage(format!(
                            "unknown relay status flag '{other}'"
                        )))
                    }
                }
            }
            run_relay_status(spool, json)
        }
        other => Err(CliError::Usage(format!("unknown relay command '{other}'"))),
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
                CliError::Usage("identity not set: pass --as <agent-id> or set $ORG_AGENT".into())
            })?,
    };
    validate_agent_id(&raw, "identity").map_err(CliError::Usage)?;
    Ok(raw)
}

fn read_body_arg(body_flag: Option<String>, positional: &[String]) -> Result<String, CliError> {
    let raw = if let Some(b) = body_flag {
        b
    } else if !positional.is_empty() {
        positional.join(" ")
    } else {
        return Err(CliError::Usage(
            "missing message body (pass <body | -> or --body <body>)".into(),
        ));
    };
    if raw == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Spool(SpoolError::Io(e)))?;
        Ok(buf)
    } else {
        Ok(raw)
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

fn parse_team_spec(raw: &str) -> Result<TeamEntry, CliError> {
    let parts: Vec<&str> = raw.splitn(3, ':').collect();
    let id = parts[0].trim();
    if id.is_empty() {
        return Err(CliError::Usage(
            "--team requires <id[:name[:title]]>".into(),
        ));
    }
    let name = parts
        .get(1)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(id);
    let title_in_team = parts
        .get(2)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(TeamEntry {
        id: id.to_string(),
        name: name.to_string(),
        title_in_team,
    })
}

fn parse_ref_spec(raw: &str, flag: &str) -> Result<RefEntry, CliError> {
    let parts: Vec<&str> = raw.splitn(2, ':').collect();
    let id = parts[0].trim();
    if id.is_empty() {
        return Err(CliError::Usage(format!("{flag} requires <id[:name]>")));
    }
    let name = parts
        .get(1)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(id);
    Ok(RefEntry {
        id: id.to_string(),
        name: name.to_string(),
    })
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

/// A member edits its own profile; only the owner may edit someone else's (Phase A has no
/// authentication, so this guards against mistakes, not against an agent passing `--as`).
fn ensure_may_edit_profile(as_flag: Option<&str>, target: &str) -> Result<(), CliError> {
    let me = resolve_identity(as_flag)?;
    if me == target || me == OWNER_ID {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "{me} can't edit {target}'s profile (only its own; the owner can edit any)"
        )))
    }
}

fn now_iso() -> String {
    format_iso8601_utc(SystemTime::now())
}

fn print_status(id: &str, status: Option<&str>, expires_at: Option<&str>, json: bool) {
    let quote_or_null = |v: Option<&str>| v.map(json_quote).unwrap_or_else(|| "null".into());
    if json {
        println!(
            "{{\"id\":{},\"status\":{},\"expires_at\":{}}}",
            json_quote(id),
            quote_or_null(status),
            quote_or_null(expires_at)
        );
    } else {
        match status {
            Some(st) => println!("{id}: {}", sanitize_inline_text(st, 140)),
            None => println!("{id}: (no status)"),
        }
    }
}

/// One member's card for `who <member>` (text). Profile text is member-supplied, so sanitized.
fn print_card(card: &DirectoryCard) {
    let clean = |s: &str| sanitize_inline_text(s, 140);
    let teams: Vec<String> = card
        .teams
        .iter()
        .map(|t| match &t.title_in_team {
            Some(title) => format!("{} ({})", clean(&t.name), clean(title)),
            None => clean(&t.name),
        })
        .collect();
    let kind = if card.owner {
        format!("{} (owner)", card.kind)
    } else {
        card.kind.clone()
    };
    println!("id:          {}", card.id);
    println!("name:        {}", clean(&card.name));
    println!("kind:        {}", clean(&kind));
    println!(
        "role:        {}",
        card.role_title.as_deref().map(clean).unwrap_or("-".into())
    );
    println!(
        "teams:       {}",
        if teams.is_empty() {
            "-".to_string()
        } else {
            teams.join(", ")
        }
    );
    println!(
        "reports to:  {}",
        card.reports_to
            .as_ref()
            .map(|r| format!("{} ({})", clean(&r.name), clean(&r.id)))
            .unwrap_or("-".into())
    );
    println!(
        "office:      {}",
        card.office
            .as_ref()
            .map(|o| clean(&o.name))
            .unwrap_or("-".into())
    );
    println!(
        "presence:    {}{}",
        card.presence,
        card.last_seen_at
            .as_deref()
            .map(|t| format!(" (last seen {t})"))
            .unwrap_or_default()
    );
    println!(
        "status:      {}",
        card.status.as_deref().map(clean).unwrap_or("-".into())
    );
}

/// A tab-separated `who` row: id, name, kind, presence, role, status.
fn print_card_row(card: &DirectoryCard) {
    let clean = |s: &str| sanitize_inline_text(s, 140);
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        card.id,
        clean(&card.name),
        clean(&card.kind),
        card.presence,
        card.role_title.as_deref().map(clean).unwrap_or("-".into()),
        card.status.as_deref().map(clean).unwrap_or("-".into())
    );
}

fn print_usage() {
    eprintln!(
        "\
Usage:
  org-agent whoami [--as <agent>] [--json]
  org-agent who    [<member>] [--team <team>] [--kind agent|person|owner] [--json]
  org-agent status [<text> | --clear] [--ttl 8h] [--as <agent>] [--json]
  org-agent profile set [<id>] [--name <n>] [--kind agent|person] [--owner] [--role <r>] [--team <id[:name[:title]]>] [--reports-to <id[:name]>] [--office <id[:name]>] [--as <agent>] [--json]
  org-agent msg register [<agent>] [--name <n>] [--kind agent|person] [--role <r>] [--team <t>] [--reports-to <r>] [--as <agent>] [--json]
  org-agent msg deregister <agent> [--json]
  org-agent msg send   --to <agent> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--as <agent>] [--json] <body | ->
  org-agent msg ask    --to <agent> [--timeout 120s] [--subject <s>] [--priority high|urgent] [--meta k=v] [--as <agent>] [--json] <body | ->
  org-agent msg reply  <id> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--as <agent>] [--json] [--body <body> | <body | ->]
  org-agent msg defer  <id> --until <30m|2h|after-task|ISO> [--note <text>] [--as <agent>] [--json]
  org-agent msg done   <id> [--note <text>] [--as <agent>] [--json]
  org-agent msg notify [--session <id>] [--after-task] [--urgent-only] [--sweep-after 5m] [--format text|json] [--as <agent>]
  org-agent msg due    [--after-task] [--urgent-only] [--sweep-after 5m] [--as <agent>]   (JSON, exit 0)
  org-agent msg inbox  [--all] [--summary] [--from <agent>] [--thread <id>] [--limit <n>] [--as <agent>] [--json]
  org-agent msg read   <id> | --next [--as <agent>] [--json]
  org-agent msg wait   [--timeout 60s] [--from <agent>] [--urgent-only] [--as <agent>] [--json]
  org-agent relay <run [--once] | status [--json]>"
    );
}

fn apply_profile_flags(
    spool: &Spool,
    target_id: String,
    args: &[String],
) -> Result<MemberProfile, CliError> {
    let mut profile = spool
        .get_profile(&target_id)
        .unwrap_or_else(|_| MemberProfile::default_for(&target_id));
    let mut teams_overridden = false;
    let mut j = 0usize;
    while j < args.len() {
        match args[j].as_str() {
            "--name" => {
                j += 1;
                let v = args
                    .get(j)
                    .ok_or_else(|| CliError::Usage("missing value for --name".into()))?;
                profile.name = v.trim().to_string();
            }
            "--kind" => {
                j += 1;
                let v = args
                    .get(j)
                    .ok_or_else(|| CliError::Usage("missing value for --kind".into()))?
                    .trim()
                    .to_ascii_lowercase();
                if v == "owner" {
                    profile.kind = "person".to_string();
                    profile.owner = true;
                } else if v == "agent" || v == "person" {
                    profile.kind = v;
                } else {
                    return Err(CliError::Usage(
                        "--kind must be agent, person, or owner".into(),
                    ));
                }
            }
            "--owner" => {
                profile.owner = true;
                profile.kind = "person".to_string();
            }
            "--role" | "--role-title" => {
                j += 1;
                let v = args
                    .get(j)
                    .ok_or_else(|| CliError::Usage("missing value for --role".into()))?;
                profile.role_title = Some(v.trim().to_string());
            }
            "--team" => {
                j += 1;
                let v = args
                    .get(j)
                    .ok_or_else(|| CliError::Usage("missing value for --team".into()))?;
                let team = parse_team_spec(v)?;
                if !teams_overridden {
                    profile.teams.clear();
                    teams_overridden = true;
                }
                profile.teams.push(team);
            }
            "--reports-to" => {
                j += 1;
                let v = args
                    .get(j)
                    .ok_or_else(|| CliError::Usage("missing value for --reports-to".into()))?;
                profile.reports_to = Some(parse_ref_spec(v, "--reports-to")?);
            }
            "--office" => {
                j += 1;
                let v = args
                    .get(j)
                    .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                profile.office = Some(parse_ref_spec(v, "--office")?);
            }
            other => {
                return Err(CliError::Usage(format!("unknown profile flag '{other}'")));
            }
        }
        j += 1;
    }
    Ok(spool.set_profile(&profile)?)
}

fn run(argv0: &str, args: Vec<String>) -> Result<u8, CliError> {
    let bin_name = Path::new(argv0)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("org-agent");
    if bin_name == "org-relay" {
        let spool = Spool::open_from_env()?;
        return run_relay_subcommand(&spool, &args, false);
    }

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
                filtered.push("--".to_string());
                after_double_dash = true;
                i += 1;
            }
            "--json" => {
                json_flag = true;
                i += 1;
            }
            "--as" => {
                let val = args
                    .get(i + 1)
                    .ok_or_else(|| CliError::Usage("missing value for --as".into()))?;
                as_agent = Some(val.clone());
                i += 2;
            }
            other => {
                filtered.push(other.to_string());
                i += 1;
            }
        }
    }

    let Some(top) = filtered.first().map(|s| s.as_str()) else {
        print_usage();
        return Err(CliError::Usage("missing command".into()));
    };

    match top {
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(EXIT_OK)
        }
        "-V" | "--version" | "version" => {
            println!("org-agent {}", env!("CARGO_PKG_VERSION"));
            Ok(EXIT_OK)
        }
        "whoami" => {
            if filtered.len() > 1 {
                return Err(CliError::Usage(
                    "whoami takes no positional arguments".into(),
                ));
            }
            let id = resolve_identity(as_agent.as_deref())?;
            if json_flag {
                // Same shape as v1-old's `whoami --json`.
                println!("{{\"agent\":{}}}", json_quote(&id));
            } else {
                println!("{id}");
            }
            Ok(EXIT_OK)
        }
        "who" => {
            let spool = Spool::open_from_env()?;
            let mut team_filter: Option<String> = None;
            let mut kind_filter: Option<String> = None;
            let mut target: Option<String> = None;

            let mut j = 1usize;
            while j < filtered.len() {
                match filtered[j].as_str() {
                    "--team" => {
                        j += 1;
                        team_filter = Some(
                            filtered
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --team".into()))?
                                .clone(),
                        );
                    }
                    "--kind" => {
                        j += 1;
                        kind_filter = Some(
                            filtered
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --kind".into()))?
                                .clone(),
                        );
                    }
                    "--office" => {
                        j += 1;
                        let _ = filtered
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                    }
                    other if other.starts_with('-') => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => {
                        if target.is_some() {
                            return Err(CliError::Usage(
                                "who accepts at most one member name or id".into(),
                            ));
                        }
                        target = Some(other.to_string());
                    }
                }
                j += 1;
            }

            if let Some(member_query) = target {
                let card = spool.resolve_directory_member(&member_query)?;
                if json_flag {
                    println!(
                        "{}",
                        serde_json::to_string(&card).map_err(|e| CliError::Usage(e.to_string()))?
                    );
                } else {
                    print_card(&card);
                }
            } else {
                let cards = spool.list_directory(team_filter.as_deref(), kind_filter.as_deref())?;
                if json_flag {
                    println!(
                        "{}",
                        serde_json::to_string(&cards)
                            .map_err(|e| CliError::Usage(e.to_string()))?
                    );
                } else {
                    for card in &cards {
                        print_card_row(card);
                    }
                }
            }
            Ok(EXIT_OK)
        }
        "profile" => {
            let Some(sub) = filtered.get(1).map(|s| s.as_str()) else {
                return Err(CliError::Usage(
                    "usage: org-agent profile set [<id>] [options]".into(),
                ));
            };
            if sub != "set" {
                return Err(CliError::Usage(format!(
                    "unknown profile subcommand '{sub}' (expected 'set')"
                )));
            }
            let spool = Spool::open_from_env()?;
            let rest = &filtered[2..];
            let (target_id, flag_slice) = if let Some(first) = rest.first() {
                if !first.starts_with('-') {
                    validate_agent_id(first, "profile").map_err(CliError::Usage)?;
                    (first.clone(), &rest[1..])
                } else {
                    (resolve_identity(as_agent.as_deref())?, rest)
                }
            } else {
                (resolve_identity(as_agent.as_deref())?, rest)
            };
            ensure_may_edit_profile(as_agent.as_deref(), &target_id)?;
            let updated = apply_profile_flags(&spool, target_id, flag_slice)?;
            if let Ok(me) = resolve_identity(as_agent.as_deref()) {
                spool.touch_last_seen(&me);
            }
            if json_flag {
                println!(
                    "{}",
                    serde_json::to_string(&updated).map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("profile updated: {} ({})", updated.name, updated.id);
            }
            Ok(EXIT_OK)
        }
        "status" => {
            let spool = Spool::open_from_env()?;
            let me = resolve_identity(as_agent.as_deref())?;
            spool.touch_last_seen(&me);
            let mut clear = false;
            let mut ttl: Option<Duration> = None;
            let mut text_parts: Vec<String> = Vec::new();

            let mut j = 1usize;
            while j < filtered.len() {
                match filtered[j].as_str() {
                    "--clear" => clear = true,
                    "--ttl" => {
                        j += 1;
                        let raw = filtered
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --ttl".into()))?;
                        ttl = Some(parse_duration_arg(raw).map_err(CliError::Usage)?);
                    }
                    "--office" => {
                        j += 1;
                        let _ = filtered
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                    }
                    other if other.starts_with('-') => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => text_parts.push(other.to_string()),
                }
                j += 1;
            }

            if clear {
                spool.clear_status(&me)?;
                if json_flag {
                    println!(
                        "{{\"id\":{},\"status\":null,\"expires_at\":null}}",
                        json_quote(&me)
                    );
                } else {
                    println!("status cleared: {me}");
                }
                return Ok(EXIT_OK);
            }

            if text_parts.is_empty() {
                let status = spool.get_active_status(&me, &now_iso());
                print_status(
                    &me,
                    status.as_ref().map(|s| s.status.as_str()),
                    status.as_ref().map(|s| s.expires_at.as_str()),
                    json_flag,
                );
                return Ok(EXIT_OK);
            }

            let text = text_parts.join(" ");
            let st = spool.set_status(&me, &text, ttl)?;
            if json_flag {
                println!(
                    "{{\"id\":{},\"status\":{},\"updated_at\":{},\"expires_at\":{}}}",
                    json_quote(&me),
                    json_quote(&st.status),
                    json_quote(&st.updated_at),
                    json_quote(&st.expires_at)
                );
            } else {
                println!("status set: {} (expires {})", st.status, st.expires_at);
            }
            Ok(EXIT_OK)
        }
        "relay" => {
            let spool = Spool::open_from_env()?;
            run_relay_subcommand(&spool, &filtered[1..], json_flag)
        }
        "msg" => run_msg(&filtered[1..], as_agent, json_flag),
        other => Err(CliError::Usage(format!("unknown command '{other}'"))),
    }
}

fn run_msg(args: &[String], as_agent: Option<String>, json_flag: bool) -> Result<u8, CliError> {
    let Some(sub) = args.first().map(|s| s.as_str()) else {
        print_usage();
        return Err(CliError::Usage("missing msg subcommand".into()));
    };
    if matches!(sub, "-h" | "--help" | "help") {
        print_usage();
        return Ok(EXIT_OK);
    }

    let spool = Spool::open_from_env()?;
    let sub_args = &args[1..];
    if let Ok(me) = resolve_identity(as_agent.as_deref()) {
        spool.touch_last_seen(&me);
    }

    match sub {
        "register" => {
            let (id, flag_slice) = if let Some(first) = sub_args.first() {
                if !first.starts_with('-') {
                    validate_agent_id(first, "register").map_err(CliError::Usage)?;
                    (first.clone(), &sub_args[1..])
                } else {
                    (resolve_identity(as_agent.as_deref())?, sub_args)
                }
            } else {
                (resolve_identity(as_agent.as_deref())?, sub_args)
            };
            if !flag_slice.is_empty() {
                ensure_may_edit_profile(as_agent.as_deref(), &id)?;
            }
            spool.register(&id)?;
            if !flag_slice.is_empty() {
                let _ = apply_profile_flags(&spool, id.clone(), flag_slice)?;
            }
            if let Ok(me) = resolve_identity(as_agent.as_deref()) {
                spool.touch_last_seen(&me);
            }
            if json_flag {
                println!("{{\"registered\":{}}}", json_quote(&id));
            } else {
                println!("{id}");
            }
            Ok(EXIT_OK)
        }

        "deregister" => {
            // `<agent>` is required (as in v1-old): never fall back to $ORG_AGENT, which would
            // delete the caller's own inbox.
            let [id] = sub_args else {
                return Err(CliError::Usage(
                    "usage: org-agent msg deregister <agent>".into(),
                ));
            };
            validate_agent_id(id, "deregister").map_err(CliError::Usage)?;
            spool.deregister(id)?;
            if json_flag {
                println!("{{\"deregistered\":{}}}", json_quote(id));
            } else {
                println!("{id}");
            }
            Ok(EXIT_OK)
        }

        "send" => {
            let from = resolve_identity(as_agent.as_deref())?;
            let mut to: Option<String> = None;
            let mut subject: Option<String> = None;
            let mut priority: Option<MessagePriority> = None;
            let mut body_flag: Option<String> = None;
            let mut meta: BTreeMap<String, MetaValue> = BTreeMap::new();
            let mut pos: Vec<String> = Vec::new();
            let mut after_double_dash = false;

            let mut j = 0usize;
            while j < sub_args.len() {
                if after_double_dash {
                    pos.push(sub_args[j].clone());
                    j += 1;
                    continue;
                }
                match sub_args[j].as_str() {
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
                    "--priority" => {
                        j += 1;
                        let raw = sub_args.get(j).ok_or_else(|| {
                            CliError::Usage("missing value for --priority".into())
                        })?;
                        priority = Some(MessagePriority::parse(raw).map_err(CliError::Usage)?);
                    }
                    "--body" => {
                        j += 1;
                        body_flag = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --body".into()))?
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
                    "--office" => {
                        j += 1;
                        let _ = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                    }
                    other if other.starts_with('-') && other != "-" => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => pos.push(other.to_string()),
                }
                j += 1;
            }

            let to = to.ok_or_else(|| CliError::Usage("send requires --to <agent>".into()))?;
            let body = read_body_arg(body_flag, &pos)?;

            if !spool.is_registered(&to) {
                return Err(CliError::Spool(SpoolError::UnknownRecipient(to)));
            }

            let mut env =
                Envelope::new_msg(from, to, subject, body, meta).map_err(CliError::Usage)?;
            if let Some(p) = priority {
                env.set_priority(p);
            }
            spool.enqueue(&env)?;
            spool.nudge_relay();

            if json_flag {
                println!("{{\"id\":{}}}", json_quote(&env.id));
            } else {
                println!("{}", env.id);
            }
            Ok(EXIT_OK)
        }

        "ask" => {
            let from = resolve_identity(as_agent.as_deref())?;
            let mut to: Option<String> = None;
            let mut subject: Option<String> = None;
            let mut priority: Option<MessagePriority> = None;
            let mut body_flag: Option<String> = None;
            let mut timeout = Duration::from_secs(120);
            let mut meta: BTreeMap<String, MetaValue> = BTreeMap::new();
            let mut pos: Vec<String> = Vec::new();
            let mut after_double_dash = false;

            let mut j = 0usize;
            while j < sub_args.len() {
                if after_double_dash {
                    pos.push(sub_args[j].clone());
                    j += 1;
                    continue;
                }
                match sub_args[j].as_str() {
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
                    "--timeout" => {
                        j += 1;
                        let raw = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --timeout".into()))?;
                        timeout = parse_duration_arg(raw).map_err(CliError::Usage)?;
                        check_wait_timeout(timeout).map_err(CliError::Usage)?;
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
                    "--priority" => {
                        j += 1;
                        let raw = sub_args.get(j).ok_or_else(|| {
                            CliError::Usage("missing value for --priority".into())
                        })?;
                        priority = Some(MessagePriority::parse(raw).map_err(CliError::Usage)?);
                    }
                    "--body" => {
                        j += 1;
                        body_flag = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --body".into()))?
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
                    "--office" => {
                        j += 1;
                        let _ = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                    }
                    other if other.starts_with('-') && other != "-" => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => pos.push(other.to_string()),
                }
                j += 1;
            }

            let to = to.ok_or_else(|| CliError::Usage("ask requires --to <agent>".into()))?;
            let body = read_body_arg(body_flag, &pos)?;

            if !spool.is_registered(&to) {
                return Err(CliError::Spool(SpoolError::UnknownRecipient(to)));
            }

            // Build and size-check the request before any side effect (inbox, wake socket).
            let mut req = Envelope::new_request(&from, to, subject, body, meta, timeout)
                .map_err(CliError::Usage)?;
            if let Some(p) = priority {
                req.set_priority(p);
            }
            req.to_json_bytes().map_err(CliError::Usage)?;

            spool.register(&from)?;
            let listener = spool.bind_wake_listener(&from)?;
            spool.enqueue(&req)?;
            spool.nudge_relay();

            let reply = spool.wait_for_reply_with_listener(&from, &req.id, &listener, timeout)?;
            if json_flag {
                print_envelope(&reply, true)?;
            } else {
                // v1-old prints only the answer's body for a plain `ask`.
                println!("{}", reply.body);
            }
            Ok(EXIT_OK)
        }

        "inbox" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut include_all = false;
            let mut summary_only = false;
            let mut from_filter: Option<String> = None;
            let mut thread_filter: Option<String> = None;
            let mut limit: Option<usize> = None;

            let mut j = 0usize;
            while j < sub_args.len() {
                match sub_args[j].as_str() {
                    "--all" => include_all = true,
                    "--summary" => summary_only = true,
                    "--from" => {
                        j += 1;
                        from_filter = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --from".into()))?
                                .clone(),
                        );
                    }
                    "--limit" => {
                        j += 1;
                        let raw = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --limit".into()))?;
                        let parsed = raw
                            .parse::<usize>()
                            .map_err(|_| CliError::Usage(format!("invalid --limit '{raw}'")))?;
                        if parsed == 0 || parsed > 1000 {
                            return Err(CliError::Usage(
                                "--limit must be between 1 and 1000".into(),
                            ));
                        }
                        limit = Some(parsed);
                    }
                    "--thread" => {
                        j += 1;
                        thread_filter = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| {
                                    CliError::Usage("missing value for --thread".into())
                                })?
                                .clone(),
                        );
                    }
                    "--office" => {
                        j += 1;
                        let _ = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                    }
                    "--cursor" => {
                        return Err(CliError::Usage(
                            "--cursor is not supported by the spool (Phase A); use --limit".into(),
                        ));
                    }
                    other => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                }
                j += 1;
            }

            let effective_limit = limit.or(if summary_only { Some(50) } else { None });
            let mut items = spool.list_inbox_items(&me, include_all, summary_only, None)?;
            if let Some(ref f) = from_filter {
                items.retain(|i| i.from.eq_ignore_ascii_case(f));
            }
            if let Some(ref t) = thread_filter {
                // Phase A threads: the message itself and direct replies to it.
                items.retain(|i| &i.id == t || i.reply_to.as_deref() == Some(t.as_str()));
            }
            if let Some(lim) = effective_limit {
                items.truncate(lim);
            }

            if json_flag {
                let s = serde_json::to_string(&items)
                    .map_err(|e| CliError::Usage(format!("serialize inbox: {e}")))?;
                println!("{s}");
            } else {
                for item in items {
                    // Keep the TSV one row per message for the scripts that parse it.
                    let subj = sanitize_inline_text(item.subject.as_deref().unwrap_or("-"), 200);
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        item.id, item.kind, item.from, item.created_at, subj
                    );
                }
            }
            Ok(EXIT_OK)
        }

        "read" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut next = false;
            let mut id: Option<String> = None;

            for arg in sub_args {
                match arg.as_str() {
                    "--next" => next = true,
                    other if other.starts_with('-') => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => {
                        if id.is_some() {
                            return Err(CliError::Usage("read takes a single <id>".into()));
                        }
                        id = Some(other.to_string());
                    }
                }
            }

            if next && id.is_some() {
                return Err(CliError::Usage("cannot combine --next with <id>".into()));
            }
            if !next && id.is_none() {
                return Err(CliError::Usage("read requires <id> or --next".into()));
            }

            if next {
                if let Some(env) = spool.claim_next(&me)? {
                    print_envelope(&env, json_flag)?;
                } else if json_flag {
                    println!("null");
                }
            } else if let Some(target_id) = id {
                let env = spool.claim(&me, &target_id)?;
                print_envelope(&env, json_flag)?;
            }
            Ok(EXIT_OK)
        }

        "reply" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut subject: Option<String> = None;
            let mut priority: Option<MessagePriority> = None;
            let mut body_flag: Option<String> = None;
            let mut meta: BTreeMap<String, MetaValue> = BTreeMap::new();
            let mut pos: Vec<String> = Vec::new();
            let mut after_double_dash = false;

            let mut j = 0usize;
            while j < sub_args.len() {
                if after_double_dash {
                    pos.push(sub_args[j].clone());
                    j += 1;
                    continue;
                }
                match sub_args[j].as_str() {
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
                    "--priority" => {
                        j += 1;
                        let raw = sub_args.get(j).ok_or_else(|| {
                            CliError::Usage("missing value for --priority".into())
                        })?;
                        priority = Some(MessagePriority::parse(raw).map_err(CliError::Usage)?);
                    }
                    "--body" => {
                        j += 1;
                        body_flag = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --body".into()))?
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
                    "--office" => {
                        j += 1;
                        let _ = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
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
                    "usage: org-agent msg reply <id> [--body <body> | <body | ->]".into(),
                ));
            };
            let body = read_body_arg(body_flag, &pos[1..])?;
            let orig = spool.find_for_reply(&me, &orig_id)?;

            if !spool.is_registered(&orig.from) {
                return Err(CliError::Spool(SpoolError::UnknownRecipient(
                    orig.from.clone(),
                )));
            }

            let mut reply = orig
                .new_reply(&me, subject, body, meta)
                .map_err(CliError::Usage)?;
            if let Some(p) = priority {
                reply.set_priority(p);
            }
            spool.enqueue(&reply)?;
            spool.nudge_relay();
            // Answering a message marks it read (only once the reply is safely queued).
            let _ = spool.claim(&me, &orig_id);

            if json_flag {
                println!("{{\"id\":{}}}", json_quote(&reply.id));
            } else {
                println!("{}", reply.id);
            }
            Ok(EXIT_OK)
        }

        "defer" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut id: Option<String> = None;
            let mut until_arg: Option<String> = None;
            let mut note: Option<String> = None;

            let mut j = 0usize;
            while j < sub_args.len() {
                match sub_args[j].as_str() {
                    "--until" => {
                        j += 1;
                        until_arg = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --until".into()))?
                                .clone(),
                        );
                    }
                    "--note" => {
                        j += 1;
                        note = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --note".into()))?
                                .clone(),
                        );
                    }
                    "--office" => {
                        j += 1;
                        let _ = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                    }
                    other if other.starts_with('-') => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => {
                        if id.is_some() {
                            return Err(CliError::Usage("defer takes a single <id>".into()));
                        }
                        id = Some(other.to_string());
                    }
                }
                j += 1;
            }

            let id = id.ok_or_else(|| {
                CliError::Usage(
                    "usage: org-agent msg defer <id> --until <30m|2h|after-task|ISO> [--note <text>]"
                        .into(),
                )
            })?;
            let until_arg = until_arg.ok_or_else(|| {
                CliError::Usage("defer requires --until <30m|2h|after-task|ISO>".into())
            })?;
            let outcome = spool.defer_message(&me, &id, &until_arg, note)?;
            if json_flag {
                println!(
                    "{}",
                    serde_json::to_string(&outcome).map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!(
                    "snoozed {} until {} (triage_gen={})",
                    outcome.id, outcome.snoozed_until, outcome.triage_gen
                );
            }
            Ok(EXIT_OK)
        }

        "done" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut id: Option<String> = None;
            let mut note: Option<String> = None;

            let mut j = 0usize;
            while j < sub_args.len() {
                match sub_args[j].as_str() {
                    "--note" => {
                        j += 1;
                        note = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| CliError::Usage("missing value for --note".into()))?
                                .clone(),
                        );
                    }
                    "--office" => {
                        j += 1;
                        let _ = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                    }
                    other if other.starts_with('-') => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                    other => {
                        if id.is_some() {
                            return Err(CliError::Usage("done takes a single <id>".into()));
                        }
                        id = Some(other.to_string());
                    }
                }
                j += 1;
            }

            let id = id.ok_or_else(|| {
                CliError::Usage("usage: org-agent msg done <id> [--note <text>]".into())
            })?;
            let outcome = spool.done_message(&me, &id, note)?;
            if json_flag {
                println!(
                    "{}",
                    serde_json::to_string(&outcome).map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("done {}", outcome.id);
            }
            Ok(EXIT_OK)
        }

        "notify" | "due" => {
            // `due` is the session-less JSON alias of `notify` (ADR-0003 §2.3): no --session, no
            // --format, and it exits 0 whatever it finds.
            let is_notify = sub == "notify";
            let me = resolve_identity(as_agent.as_deref())?;
            let mut opts = NotifyOptions::default();
            let mut format_json = json_flag || !is_notify;

            let mut j = 0usize;
            while j < sub_args.len() {
                match sub_args[j].as_str() {
                    "--after-task" => opts.after_task = true,
                    "--urgent-only" | "--urgent" => opts.urgent_only = true,
                    "--session" if !is_notify => {
                        return Err(CliError::Usage(
                            "msg due has no session dedup; use msg notify --session <id>".into(),
                        ));
                    }
                    "--format" if !is_notify => {
                        return Err(CliError::Usage(
                            "msg due always prints JSON; use msg notify --format text".into(),
                        ));
                    }
                    "--session" => {
                        j += 1;
                        opts.session = Some(
                            sub_args
                                .get(j)
                                .ok_or_else(|| {
                                    CliError::Usage("missing value for --session".into())
                                })?
                                .clone(),
                        );
                    }
                    "--sweep-after" => {
                        j += 1;
                        let raw = sub_args.get(j).ok_or_else(|| {
                            CliError::Usage("missing value for --sweep-after".into())
                        })?;
                        opts.sweep_after = parse_duration_arg(raw).map_err(CliError::Usage)?;
                    }
                    "--format" => {
                        j += 1;
                        let fmt = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --format".into()))?
                            .trim()
                            .to_ascii_lowercase();
                        match fmt.as_str() {
                            "text" => format_json = false,
                            "json" => format_json = true,
                            other => {
                                return Err(CliError::Usage(format!(
                                    "invalid --format '{other}' (expected text or json)"
                                )))
                            }
                        }
                    }
                    "--office" => {
                        j += 1;
                        let _ = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                    }
                    other => {
                        return Err(CliError::Usage(format!("unknown flag '{other}'")));
                    }
                }
                j += 1;
            }

            let report = spool.notify(&me, &opts)?;
            if format_json {
                println!(
                    "{}",
                    serde_json::to_string(&report).map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else if !report.text.is_empty() {
                println!("{}", report.text);
            }

            if is_notify {
                if report.count == 0 {
                    Ok(EXIT_OK)
                } else if report.urgent {
                    Ok(EXIT_NOTIFY_URGENT)
                } else {
                    Ok(EXIT_NOTIFY_NORMAL)
                }
            } else {
                Ok(EXIT_OK)
            }
        }

        "wait" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let mut timeout = Duration::from_secs(60);
            let mut from_filter: Option<String> = None;
            let mut urgent_only = false;

            let mut j = 0usize;
            while j < sub_args.len() {
                match sub_args[j].as_str() {
                    "--timeout" => {
                        j += 1;
                        let raw = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --timeout".into()))?;
                        timeout = parse_duration_arg(raw).map_err(CliError::Usage)?;
                        check_wait_timeout(timeout).map_err(CliError::Usage)?;
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
                    "--urgent-only" | "--urgent" => urgent_only = true,
                    "--office" => {
                        j += 1;
                        let _ = sub_args
                            .get(j)
                            .ok_or_else(|| CliError::Usage("missing value for --office".into()))?;
                    }
                    other => return Err(CliError::Usage(format!("unknown flag '{other}'"))),
                }
                j += 1;
            }

            let env =
                spool.wait_for_inbox_filtered(&me, from_filter.as_deref(), urgent_only, timeout)?;
            print_envelope(&env, json_flag)?;
            Ok(EXIT_OK)
        }

        other => Err(CliError::Usage(format!("unknown msg subcommand '{other}'"))),
    }
}

fn main() -> ExitCode {
    install_signal_handlers();
    let mut argv = env::args();
    let argv0 = argv.next().unwrap_or_else(|| "org-agent".to_string());
    let args: Vec<String> = argv.collect();
    match run(&argv0, args) {
        Ok(code) => ExitCode::from(code),
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
