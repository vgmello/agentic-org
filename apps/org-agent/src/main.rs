//! `org-agent`: Fast CLI for agent messaging, directory (`who`), status (`status`),
//! and message triage (`defer`, `done`, `notify`, `due`) over `org-spool`.
//!
//! `msg send | inbox | read | wait` and `who` go through the agent's office when this Mac has
//! one and it answers (see [`office`]); the spool stays the offline fallback.
//!
//! Also supports `org-agent relay <run|status>` and `argv[0]` dispatch when invoked as `org-relay`.

mod office;

use std::collections::BTreeMap;
use std::env;
use std::io::{self, Read};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use org_spool::{
    authorize_profile_edit, check_wait_timeout, format_iso8601_utc, json_quote, member_id_arg,
    parse_duration_arg, request_interrupt, resolve_caller, sanitize_inline_text, DirectoryCard,
    Envelope, InboxItem, InboxQuery, MessagePriority, MetaValue, NotifyOptions, Outgoing,
    ProfileUpdate, RelayOutcome, Spool, SpoolError, INBOX_MAX_LIMIT,
};

use office::{Failure, Route, Shown, Waited};

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
    /// From the office path: an exit code and what to say.
    Failed(Failure),
}

impl From<SpoolError> for CliError {
    fn from(value: SpoolError) -> Self {
        Self::Spool(value)
    }
}

impl From<Failure> for CliError {
    fn from(value: Failure) -> Self {
        Self::Failed(value)
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
    let session = env::var("ORG_AGENT").ok();
    resolve_caller(as_flag, session.as_deref()).map_err(CliError::Usage)
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutgoingKind {
    Send,
    Ask,
    Reply,
}

/// The flags and positionals of `msg send`, `ask` and `reply`.
#[derive(Default)]
struct OutgoingArgs {
    to: Option<String>,
    timeout: Option<Duration>,
    subject: Option<String>,
    priority: Option<MessagePriority>,
    body: Option<String>,
    meta: BTreeMap<String, MetaValue>,
    /// `--office <id>`: which office `send` goes to (`ask` and `reply` still use the spool).
    office: Option<String>,
    positional: Vec<String>,
}

impl OutgoingArgs {
    /// The message, its body from `--body`, the positionals after the first `skip` (joined),
    /// or stdin for `-`.
    fn outgoing(self, skip: usize) -> Result<Outgoing, CliError> {
        let body = read_body_arg(self.body, &self.positional[skip..])?;
        Ok(Outgoing {
            subject: self.subject,
            body,
            meta: self.meta,
            priority: self.priority,
        })
    }
}

fn parse_outgoing(args: &[String], kind: OutgoingKind) -> Result<OutgoingArgs, CliError> {
    let mut out = OutgoingArgs::default();
    let mut after_double_dash = false;
    let mut j = 0usize;
    while j < args.len() {
        if after_double_dash {
            out.positional.push(args[j].clone());
            j += 1;
            continue;
        }
        let flag = args[j].as_str();
        let mut value = || {
            j += 1;
            args.get(j)
                .cloned()
                .ok_or_else(|| CliError::Usage(format!("missing value for {flag}")))
        };
        match flag {
            "--" => after_double_dash = true,
            "--to" if kind != OutgoingKind::Reply => out.to = Some(value()?),
            "--timeout" if kind == OutgoingKind::Ask => {
                let timeout = parse_duration_arg(&value()?).map_err(CliError::Usage)?;
                // Refused as it's parsed, before the other flags and the body (stdin).
                check_wait_timeout(timeout).map_err(CliError::Usage)?;
                out.timeout = Some(timeout);
            }
            "--subject" => out.subject = Some(value()?),
            "--priority" => {
                out.priority = Some(MessagePriority::parse(&value()?).map_err(CliError::Usage)?);
            }
            "--body" => out.body = Some(value()?),
            "--meta" => {
                let (k, v) = parse_meta_kv(&value()?)?;
                out.meta.insert(k, v);
            }
            "--office" => out.office = Some(value()?),
            other if other.starts_with('-') && other != "-" => {
                return Err(CliError::Usage(format!("unknown flag '{other}'")));
            }
            other => out.positional.push(other.to_string()),
        }
        j += 1;
    }
    Ok(out)
}

fn print_envelope(env: &Envelope, json: bool) -> Result<(), CliError> {
    if json {
        let s = env.to_json_string().map_err(CliError::Usage)?;
        println!("{s}");
    } else {
        print_message_text(
            &env.id,
            env.kind.as_str(),
            &env.from,
            &env.to,
            env.subject.as_deref(),
            env.reply_to.as_deref(),
            &env.body,
        );
    }
    Ok(())
}

/// [`print_envelope`] for a message from the office: the same JSON and text.
fn print_shown(shown: &Shown, json: bool) -> Result<(), CliError> {
    if json {
        let s = serde_json::to_string(shown)
            .map_err(|_| CliError::Usage("the message can't be printed as JSON".into()))?;
        println!("{s}");
    } else {
        print_message_text(
            &shown.id,
            &shown.kind,
            &shown.from,
            &shown.to,
            shown.subject.as_deref(),
            shown.reply_to.as_deref(),
            &shown.body,
        );
    }
    Ok(())
}

/// A message as text: one header line, then the body.
fn print_message_text(
    id: &str,
    kind: &str,
    from: &str,
    to: &str,
    subject: Option<&str>,
    reply_to: Option<&str>,
    body: &str,
) {
    // The header stays one line, like the inbox TSV (the subject is untrusted, and so are
    // the kind and the addresses an office sends).
    let clean = |s: &str| sanitize_inline_text(s, 200);
    let subj = subject
        .map(|s| format!(" · {}", clean(s)))
        .unwrap_or_default();
    let reply = reply_to
        .map(|r| format!(" (reply_to: {})", clean(r)))
        .unwrap_or_default();
    println!(
        "{} [{}] {} -> {}{}{}\n{}",
        clean(id),
        clean(kind),
        clean(from),
        clean(to),
        subj,
        reply,
        body
    );
}

/// `msg inbox` rows: JSON, or one tab-separated line per message.
fn print_inbox(items: Vec<InboxItem>, include_all: bool, json: bool) -> Result<(), CliError> {
    if json {
        let s = serde_json::to_string(&items)
            .map_err(|e| CliError::Usage(format!("serialize inbox: {e}")))?;
        println!("{s}");
        return Ok(());
    }
    for item in items {
        // Keep the TSV one row per message for the scripts that parse it.
        let clean = |s: &str| sanitize_inline_text(s, 200);
        let subj = clean(item.subject.as_deref().unwrap_or("-"));
        let row = format!(
            "{}\t{}\t{}\t{}\t{}",
            clean(&item.id),
            clean(&item.kind),
            clean(&item.from),
            clean(&item.created_at),
            subj
        );
        if include_all {
            // `--all` mixes unread, snoozed, done and read: two more columns, the status and
            // (for a snooze) when it's due. Plain `inbox` keeps v1-old's five.
            let due = match item.status.as_str() {
                "snoozed" => item.snoozed_until.as_deref().unwrap_or("-"),
                _ => "-",
            };
            println!("{row}\t{}\t{}", clean(&item.status), clean(due));
        } else {
            println!("{row}");
        }
    }
    Ok(())
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
    println!("id:          {}", clean(&card.id));
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
            .map(|t| format!(" (last seen {})", clean(t)))
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
        clean(&card.id),
        clean(&card.name),
        clean(&card.kind),
        card.presence,
        card.role_title.as_deref().map(clean).unwrap_or("-".into()),
        card.status.as_deref().map(clean).unwrap_or("-".into())
    );
}

/// `who`'s output from the office: one card for `who <member>` (the office answers a member
/// it doesn't know with `not_found`), else the list.
fn print_cards(cards: &[DirectoryCard], one: bool, json: bool) -> Result<(), CliError> {
    let failed = |e: serde_json::Error| CliError::Usage(e.to_string());
    if one {
        let card = cards
            .first()
            .ok_or_else(|| CliError::Usage("no such member".into()))?;
        if json {
            println!("{}", serde_json::to_string(card).map_err(failed)?);
        } else {
            print_card(card);
        }
    } else if json {
        println!("{}", serde_json::to_string(cards).map_err(failed)?);
    } else {
        for card in cards {
            print_card_row(card);
        }
    }
    Ok(())
}

fn print_usage() {
    eprintln!(
        "\
Usage:
  org-agent whoami [--as <agent>] [--json]
  org-agent who    [<member>] [--team <team>] [--kind agent|person|owner] [--office <id>] [--json]
  org-agent status [<text> | --clear] [--ttl 8h] [--as <agent>] [--json]
  org-agent profile set [<id>] [--name <n>] [--kind agent|person] [--owner] [--role <r>] [--team <id[:name[:title]]>] [--reports-to <id[:name]>] [--office <id[:name]>] [--as <agent>] [--json]
  org-agent msg register [<agent>] [--name <n>] [--kind agent|person] [--role <r>] [--team <t>] [--reports-to <r>] [--as <agent>] [--json]
  org-agent msg deregister <agent> [--json]
  org-agent msg send   --to <agent> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--office <id>] [--as <agent>] [--json] <body | ->
  org-agent msg ask    --to <agent> [--timeout 120s] [--subject <s>] [--priority high|urgent] [--meta k=v] [--as <agent>] [--json] <body | ->
  org-agent msg reply  <id> [--subject <s>] [--priority normal|high|urgent] [--meta k=v] [--as <agent>] [--json] [--body <body> | <body | ->]
  org-agent msg defer  <id> --until <30m|2h|after-task|ISO> [--note <text>] [--as <agent>] [--json]
  org-agent msg done   <id> [--note <text>] [--as <agent>] [--json]
  org-agent msg notify [--session <id>] [--after-task] [--urgent-only] [--sweep-after 5m] [--format text|json] [--as <agent>]
  org-agent msg due    [--after-task] [--urgent-only] [--sweep-after 5m] [--as <agent>]   (JSON, exit 0)
  org-agent msg inbox  [--all] [--summary] [--from <agent>] [--thread <id>] [--limit <n>] [--office <id>] [--as <agent>] [--json]
  org-agent msg read   <id> | --next [--office <id>] [--as <agent>] [--json]
  org-agent msg wait   [--timeout 60s] [--from <agent>] [--urgent-only] [--office <id>] [--as <agent>] [--json]
  org-agent relay <run [--once] | status [--json]>"
    );
}

/// Parse `profile set` / `msg register` flags into a [`ProfileUpdate`] (its semantics live in
/// org-spool), before anything is written.
fn parse_profile_flags(args: &[String]) -> Result<ProfileUpdate, CliError> {
    let mut update = ProfileUpdate::default();
    let mut j = 0usize;
    while j < args.len() {
        let flag = args[j].as_str();
        let mut value = || {
            j += 1;
            args.get(j)
                .map(String::as_str)
                .ok_or_else(|| CliError::Usage(format!("missing value for {flag}")))
        };
        match flag {
            "--name" => update.name(value()?),
            "--kind" => update.kind(value()?).map_err(CliError::Usage)?,
            "--owner" => update.owner(),
            "--role" | "--role-title" => update.role(value()?),
            "--team" => update.team(value()?).map_err(CliError::Usage)?,
            "--reports-to" => update.reports_to(value()?).map_err(CliError::Usage)?,
            "--office" => update.office(value()?).map_err(CliError::Usage)?,
            other => {
                return Err(CliError::Usage(format!("unknown profile flag '{other}'")));
            }
        }
        j += 1;
    }
    Ok(update)
}

const ORG_RELAY_USAGE: &str = "Usage:\n  org-relay run [--once]\n  org-relay status [--json]";

/// `org-relay ...` (this binary invoked through an `org-relay` symlink): v1-old's org-relay,
/// exactly. Help works without a spool; errors are prefixed `org-relay:`; a usage error exits
/// 1 and any spool error (including no HOME/ORG_HOME) exits 4.
fn run_org_relay(args: &[String]) -> ExitCode {
    let usage_error = |msg: String| {
        eprintln!("org-relay: {msg}");
        eprintln!("{ORG_RELAY_USAGE}");
        ExitCode::from(EXIT_USAGE)
    };
    let Some(cmd) = args.first().map(String::as_str) else {
        eprintln!("{ORG_RELAY_USAGE}");
        return ExitCode::from(EXIT_USAGE);
    };
    if matches!(cmd, "-h" | "--help" | "help") {
        eprintln!("{ORG_RELAY_USAGE}");
        return ExitCode::from(EXIT_OK);
    }
    let spool = match Spool::open_from_env() {
        Ok(spool) => spool,
        Err(e) => {
            eprintln!("org-relay: {e}");
            return ExitCode::from(EXIT_IO);
        }
    };
    let (flag, flags) = match cmd {
        "run" => ("--once", &args[1..]),
        "status" => ("--json", &args[1..]),
        other => return usage_error(format!("unknown command '{other}'")),
    };
    if let Some(other) = flags.iter().find(|a| a.as_str() != flag) {
        return usage_error(format!("unknown flag '{other}'"));
    }
    let set = !flags.is_empty();
    let result = if cmd == "run" {
        run_relay_loop(&spool, set)
    } else {
        run_relay_status(&spool, set)
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(CliError::Usage(msg)) => usage_error(msg),
        Err(CliError::Spool(e)) => {
            eprintln!("org-relay: {e}");
            ExitCode::from(EXIT_IO)
        }
        // The relay never talks to an office.
        Err(CliError::Failed(failure)) => {
            if let Some(message) = failure.message {
                eprintln!("org-relay: {message}");
            }
            ExitCode::from(failure.code)
        }
    }
}

fn run(args: Vec<String>) -> Result<u8, CliError> {
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
            // `--office <id>` is accepted and ignored (one spool in Phase A), as elsewhere.
            match &filtered[1..] {
                [] => {}
                [flag, _] if flag == "--office" => {}
                [flag] if flag == "--office" => {
                    return Err(CliError::Usage("missing value for --office".into()));
                }
                _ => {
                    return Err(CliError::Usage(
                        "whoami takes no positional arguments".into(),
                    ));
                }
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
            let mut team_filter: Option<String> = None;
            let mut kind_filter: Option<String> = None;
            let mut target: Option<String> = None;
            let mut office_flag: Option<String> = None;

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
                        office_flag = Some(
                            filtered
                                .get(j)
                                .ok_or_else(|| {
                                    CliError::Usage("missing value for --office".into())
                                })?
                                .clone(),
                        );
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

            // Through the office when there is one, as the caller (it needs a binding). With
            // no identity, the spool, as before.
            if let Ok(me) = resolve_identity(as_agent.as_deref()) {
                match office::route(&me, office_flag.as_deref())? {
                    Route::Office(office) => {
                        let params = office::who_params(
                            target.as_deref(),
                            team_filter.as_deref(),
                            kind_filter.as_deref(),
                        )
                        .map_err(CliError::Usage)?;
                        let cards = office.who(&params)?;
                        print_cards(&cards, target.is_some(), json_flag)?;
                        return Ok(EXIT_OK);
                    }
                    Route::Unreachable(why) => eprintln!("{}", why.spool_notice()),
                    Route::NoOffice => {}
                }
            }
            let spool = Spool::open_from_env()?;
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
                    (
                        member_id_arg(first, "profile").map_err(CliError::Usage)?,
                        &rest[1..],
                    )
                } else {
                    (resolve_identity(as_agent.as_deref())?, rest)
                }
            } else {
                (resolve_identity(as_agent.as_deref())?, rest)
            };
            // Authorized first, then the flags (the order v1-old's users see errors in).
            let me = resolve_identity(as_agent.as_deref())?;
            authorize_profile_edit(&me, &target_id)?;
            let update = parse_profile_flags(flag_slice)?;
            let updated = spool.edit_profile(&me, &target_id, &update)?;
            spool.touch_last_seen(&me);
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
                    "--" => {
                        // Everything after `--` is status text, even if it starts with '-'.
                        text_parts.extend(filtered[j + 1..].iter().cloned());
                        break;
                    }
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

    let sub_args = &args[1..];
    // Through the office when there is one (each opens the spool only to fall back).
    match sub {
        "send" => return msg_send(sub_args, as_agent.as_deref(), json_flag),
        "inbox" => return msg_inbox(sub_args, as_agent.as_deref(), json_flag),
        "read" => return msg_read(sub_args, as_agent.as_deref(), json_flag),
        "wait" => return msg_wait(sub_args, as_agent.as_deref(), json_flag),
        _ => {}
    }
    let spool = local_spool(as_agent.as_deref())?;

    match sub {
        "register" => {
            let (id, flag_slice) = if let Some(first) = sub_args.first() {
                if !first.starts_with('-') {
                    (
                        member_id_arg(first, "register").map_err(CliError::Usage)?,
                        &sub_args[1..],
                    )
                } else {
                    (resolve_identity(as_agent.as_deref())?, sub_args)
                }
            } else {
                (resolve_identity(as_agent.as_deref())?, sub_args)
            };
            // Authorized, then flags parsed (and refused), before anything is created.
            if flag_slice.is_empty() {
                spool.register(&id)?;
            } else {
                let me = resolve_identity(as_agent.as_deref())?;
                authorize_profile_edit(&me, &id)?;
                let update = parse_profile_flags(flag_slice)?;
                spool.edit_profile(&me, &id, &update)?;
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
            let id = &member_id_arg(id, "deregister").map_err(CliError::Usage)?;
            let me = resolve_identity(as_agent.as_deref())?;
            spool.deregister_as(&me, id)?;
            if json_flag {
                println!("{{\"deregistered\":{}}}", json_quote(id));
            } else {
                println!("{id}");
            }
            Ok(EXIT_OK)
        }

        "ask" => {
            let from = resolve_identity(as_agent.as_deref())?;
            let parsed = parse_outgoing(sub_args, OutgoingKind::Ask)?;
            let to = parsed
                .to
                .clone()
                .ok_or_else(|| CliError::Usage("ask requires --to <agent>".into()))?;
            let timeout = parsed.timeout.unwrap_or(Duration::from_secs(120));
            let msg = parsed.outgoing(0)?;
            let reply = spool.ask(&from, &to, msg, timeout)?;
            if json_flag {
                print_envelope(&reply, true)?;
            } else {
                // v1-old prints only the answer's body for a plain `ask`.
                println!("{}", reply.body);
            }
            Ok(EXIT_OK)
        }

        "reply" => {
            let me = resolve_identity(as_agent.as_deref())?;
            let parsed = parse_outgoing(sub_args, OutgoingKind::Reply)?;
            let Some(orig_id) = parsed.positional.first().cloned() else {
                return Err(CliError::Usage(
                    "usage: org-agent msg reply <id> [--body <body> | <body | ->]".into(),
                ));
            };
            let msg = parsed.outgoing(1)?;
            let reply = spool.reply(&me, &orig_id, msg)?;
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

        other => Err(CliError::Usage(format!("unknown msg subcommand '{other}'"))),
    }
}

/// The local spool, with the caller's presence touched (as every spool `msg` command does).
fn local_spool(as_agent: Option<&str>) -> Result<Spool, CliError> {
    let spool = Spool::open_from_env()?;
    if let Ok(me) = resolve_identity(as_agent) {
        spool.touch_last_seen(&me);
    }
    Ok(spool)
}

/// `msg send`: through the office; to the spool when there is none, or when it doesn't answer
/// (then `--json` adds `"queued": true` and stderr says so, ADR-0003 §3.5).
fn msg_send(sub_args: &[String], as_agent: Option<&str>, json_flag: bool) -> Result<u8, CliError> {
    let from = resolve_identity(as_agent)?;
    let parsed = parse_outgoing(sub_args, OutgoingKind::Send)?;
    let to = parsed
        .to
        .clone()
        .ok_or_else(|| CliError::Usage("send requires --to <agent>".into()))?;
    let office_flag = parsed.office.clone();
    let msg = parsed.outgoing(0)?;
    let unreachable = match office::route(&from, office_flag.as_deref())? {
        Route::Office(office) => {
            let id = office.send(&to, msg)?;
            if json_flag {
                println!("{{\"id\":{}}}", json_quote(&id));
            } else {
                println!("{id}");
            }
            return Ok(EXIT_OK);
        }
        Route::Unreachable(why) => Some(why),
        Route::NoOffice => None,
    };
    let spooled = local_spool(as_agent).and_then(|spool| Ok(spool.send(&from, &to, msg)?));
    // The notice says "queued" only once the spool has the message.
    if let Some(why) = &unreachable {
        match &spooled {
            Ok(_) => eprintln!("{}", why.queued_notice()),
            Err(_) => eprintln!("{}", why.not_queued_notice()),
        }
    }
    let env = spooled?;
    if json_flag && unreachable.is_some() {
        println!("{{\"id\":{},\"queued\":true}}", json_quote(&env.id));
    } else if json_flag {
        println!("{{\"id\":{}}}", json_quote(&env.id));
    } else {
        println!("{}", env.id);
    }
    Ok(EXIT_OK)
}

/// `msg inbox`: through the office (headers only: no body or meta in `--json`), else the
/// spool's.
fn msg_inbox(sub_args: &[String], as_agent: Option<&str>, json_flag: bool) -> Result<u8, CliError> {
    let me = resolve_identity(as_agent)?;
    let mut include_all = false;
    let mut summary_only = false;
    let mut from_filter: Option<String> = None;
    let mut thread_filter: Option<String> = None;
    let mut limit: Option<usize> = None;
    let mut office_flag: Option<String> = None;

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
                limit = Some(
                    raw.parse::<usize>()
                        .map_err(|_| CliError::Usage(format!("invalid --limit '{raw}'")))?,
                );
            }
            "--thread" => {
                j += 1;
                thread_filter = Some(
                    sub_args
                        .get(j)
                        .ok_or_else(|| CliError::Usage("missing value for --thread".into()))?
                        .clone(),
                );
            }
            "--office" => {
                j += 1;
                office_flag = Some(
                    sub_args
                        .get(j)
                        .ok_or_else(|| CliError::Usage("missing value for --office".into()))?
                        .clone(),
                );
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
    // The spool's rule, checked before either path.
    if limit.is_some_and(|l| l == 0 || l > INBOX_MAX_LIMIT) {
        return Err(CliError::Usage(format!(
            "--limit must be between 1 and {INBOX_MAX_LIMIT}"
        )));
    }

    let query = InboxQuery {
        include_all,
        summary_only,
        from: from_filter,
        thread: thread_filter,
        limit,
    };
    let items = match office::route(&me, office_flag.as_deref())? {
        Route::Office(office) => office.inbox(&query)?,
        other => {
            if let Route::Unreachable(why) = other {
                eprintln!("{}", why.spool_notice());
            }
            local_spool(as_agent)?.inbox(&me, &query)?
        }
    };
    print_inbox(items, include_all, json_flag)?;
    Ok(EXIT_OK)
}

/// `msg read`: through the office (it marks the message read there), else the spool.
fn msg_read(sub_args: &[String], as_agent: Option<&str>, json_flag: bool) -> Result<u8, CliError> {
    let me = resolve_identity(as_agent)?;
    let mut next = false;
    let mut id: Option<String> = None;
    let mut office_flag: Option<String> = None;

    let mut args = sub_args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--next" => next = true,
            "--office" => {
                office_flag = Some(
                    args.next()
                        .ok_or_else(|| CliError::Usage("missing value for --office".into()))?
                        .clone(),
                );
            }
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

    match office::route(&me, office_flag.as_deref())? {
        Route::Office(office) => {
            match office.read(id.as_deref())? {
                Some(shown) => print_shown(&shown, json_flag)?,
                None if id.is_some() => {
                    return Err(CliError::Usage("the office returned no message".into()));
                }
                None if json_flag => println!("null"),
                None => {}
            }
            return Ok(EXIT_OK);
        }
        Route::Unreachable(why) => eprintln!("{}", why.spool_notice()),
        Route::NoOffice => {}
    }
    let spool = local_spool(as_agent)?;
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

/// `msg wait`: through the office (held there, not marked read), else the spool.
fn msg_wait(sub_args: &[String], as_agent: Option<&str>, json_flag: bool) -> Result<u8, CliError> {
    let me = resolve_identity(as_agent)?;
    let mut timeout = Duration::from_secs(60);
    let mut from_filter: Option<String> = None;
    let mut urgent_only = false;
    let mut office_flag: Option<String> = None;

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
                office_flag = Some(
                    sub_args
                        .get(j)
                        .ok_or_else(|| CliError::Usage("missing value for --office".into()))?
                        .clone(),
                );
            }
            other => return Err(CliError::Usage(format!("unknown flag '{other}'"))),
        }
        j += 1;
    }

    match office::route(&me, office_flag.as_deref())? {
        Route::Office(office) => {
            return match office.wait(from_filter.as_deref(), urgent_only, timeout)? {
                Waited::Message(shown) => {
                    print_shown(&shown, json_flag)?;
                    Ok(EXIT_OK)
                }
                // The spool's wording and exit codes.
                Waited::TimedOut => Err(CliError::Spool(SpoolError::Timeout(format!(
                    "timed out after {}ms waiting for message in {me}'s inbox",
                    timeout.as_millis()
                )))),
                Waited::Interrupted => Err(CliError::Spool(SpoolError::Interrupted)),
            };
        }
        Route::Unreachable(why) => eprintln!("{}", why.spool_notice()),
        Route::NoOffice => {}
    }
    let env = local_spool(as_agent)?.wait_for_inbox_filtered(
        &me,
        from_filter.as_deref(),
        urgent_only,
        timeout,
    )?;
    print_envelope(&env, json_flag)?;
    Ok(EXIT_OK)
}

fn main() -> ExitCode {
    install_signal_handlers();
    let mut argv = env::args();
    let argv0 = argv.next().unwrap_or_else(|| "org-agent".to_string());
    let args: Vec<String> = argv.collect();
    if Path::new(&argv0).file_name().and_then(|n| n.to_str()) == Some("org-relay") {
        return run_org_relay(&args);
    }
    match run(args) {
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
        Err(CliError::Spool(err @ SpoolError::UnknownRecipient(_))) => {
            // SpoolError's text: never suggests registering a reserved name.
            eprintln!("org-agent: {err}");
            ExitCode::from(EXIT_UNKNOWN_RECIPIENT)
        }
        Err(CliError::Spool(SpoolError::Io(err))) => {
            eprintln!("org-agent: {err}");
            ExitCode::from(EXIT_IO)
        }
        Err(CliError::Spool(SpoolError::Interrupted)) => ExitCode::from(EXIT_INTERRUPTED),
        Err(CliError::Failed(failure)) => {
            if let Some(message) = failure.message {
                eprintln!("org-agent: {message}");
            }
            ExitCode::from(failure.code)
        }
    }
}
