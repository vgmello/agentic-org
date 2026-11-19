use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use console_attach::agent::{format_event_human, PermissionPreset};
use console_attach::agent_server::{AgentClient, RunningAgentDaemon};
use console_attach::auth_http::{serve_auth_ui, AuthHttpConfig};
use console_attach::claude::ClaudeConfig;
use console_attach::protocol::ControlClient;
use console_attach::proxy::{run_proxy, ProxyConfig, RunningProxy};
use console_attach::pty::PtySize;

fn default_socket_path() -> PathBuf {
    env::var_os("CONSOLE_ATTACH_SOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/console-attach.sock"))
}

fn default_claude_socket_path() -> PathBuf {
    env::var_os("CONSOLE_ATTACH_CLAUDE_SOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/claude-agent.sock"))
}

fn print_usage() {
    eprintln!(
        "\
console-attach — Rust POC for (1) PTY TUI interception and (2) persistent Claude Code stream-json sessions

PTY PROXY USAGE (interactive TUIs like vi):
  console-attach run [--socket <path>] [--headless] [--rows <N>] [--cols <N>] [--term <TERM>] -- <program> [args...]
  console-attach send [--socket <path>] <command...>
  console-attach screen [--socket <path>] [--raw]
  console-attach wait-for [--socket <path>] [--timeout-ms <ms>] <substring>
  console-attach status [--socket <path>]
  console-attach demo-vi [output_file]

CLAUDE STREAM-JSON DAEMON USAGE (long-lived, keeps background commands & subagents alive):
  console-attach claude start [--socket <path>] [--cwd <dir>] [--config-dir <dir>] [--model <model>]
                              [--preset <safe_ask|read_only|workspace_edits|autonomous_sandboxed|unrestricted>]
                              [--sandbox <on|off>] [--permission-mode <mode>]
                              [--tools <list>] [--allowed-tool <spec>]
                              [--max-budget-usd <amt>] [--resume <id>]
                              [--stdio-approvals] [--auto-approve] [--partials] [--quiet]
                              [--bin <path>] [-- <extra-claude-args...>]
  console-attach claude send [--socket <path>] [--wait] [--timeout-ms <ms>] <message...>
  console-attach claude interrupt [--socket <path>]
  console-attach claude approve [--socket <path>] <request_id>
  console-attach claude deny [--socket <path>] <request_id> [reason...]
  console-attach claude status [--socket <path>] [--json]
  console-attach claude events [--socket <path>] [--since <seq>] [--raw]
  console-attach claude follow [--socket <path>] [--since <seq>] [--raw]
  console-attach claude stop [--socket <path>]

HEADLESS SSO-LIKE LOGIN WEB UI + RUST API (Claude paste-code & Codex device-code):
  console-attach auth-ui [--port <8787>] [--isolated-dir <dir> | --use-default-config]

ENVIRONMENT:
  CONSOLE_ATTACH_SOCK         Default PTY socket (fallback: /tmp/console-attach.sock)
  CONSOLE_ATTACH_CLAUDE_SOCK  Default Claude daemon socket (fallback: /tmp/claude-agent.sock)"
    );
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(subcommand) = args.next() else {
        print_usage();
        return ExitCode::from(2);
    };

    let rest: Vec<String> = args.collect();
    match subcommand.as_str() {
        "-h" | "--help" | "help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        "run" => cmd_run(&rest),
        "send" => cmd_send(&rest),
        "screen" => cmd_screen(&rest),
        "wait-for" => cmd_wait_for(&rest),
        "status" => cmd_status(&rest),
        "demo-vi" => cmd_demo_vi(&rest),
        "claude" => cmd_claude(&rest),
        "auth-ui" => cmd_auth_ui(&rest),
        other => {
            eprintln!("error: unknown subcommand '{other}'");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn exit_code_from_status(status: std::process::ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        ExitCode::from(code.clamp(0, 255) as u8)
    } else if let Some(sig) = status.signal() {
        ExitCode::from((128 + sig).clamp(0, 255) as u8)
    } else {
        ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------------------
// Headless SSO-like login HTTP API + demo page (Claude paste-code & Codex device-code)
// ---------------------------------------------------------------------------

fn cmd_auth_ui(args: &[String]) -> ExitCode {
    let mut port: u16 = 8787;
    let mut isolated_config_root: Option<PathBuf> = Some(PathBuf::from("/tmp/ca-sso-demo"));
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                i += 1;
                let Some(val) = args.get(i).and_then(|s| s.parse::<u16>().ok()) else {
                    eprintln!("error: --port requires a valid TCP port");
                    return ExitCode::from(2);
                };
                port = val;
            }
            "--isolated-dir" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --isolated-dir requires a directory path");
                    return ExitCode::from(2);
                };
                isolated_config_root = Some(PathBuf::from(val));
            }
            "--use-default-config" => {
                isolated_config_root = None;
            }
            other => {
                eprintln!("error: unknown option '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    match serve_auth_ui(AuthHttpConfig {
        port,
        isolated_config_root,
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// Claude stream-json persistent session commands
// ---------------------------------------------------------------------------

fn cmd_claude(args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        print_usage();
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    match sub.as_str() {
        "start" => cmd_claude_start(rest),
        "send" => cmd_claude_send(rest),
        "interrupt" => cmd_claude_interrupt(rest),
        "approve" => cmd_claude_approve(rest),
        "deny" => cmd_claude_deny(rest),
        "status" => cmd_claude_status(rest),
        "events" => cmd_claude_events(rest),
        "follow" => cmd_claude_follow(rest),
        "stop" => cmd_claude_stop(rest),
        "-h" | "--help" | "help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown claude subcommand '{other}'");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn cmd_claude_start(args: &[String]) -> ExitCode {
    let mut socket_path = default_claude_socket_path();
    let mut cfg = ClaudeConfig {
        echo_events: true,
        // Secure by default: workspace OS sandbox + interactive approval routing.
        preset: Some(PermissionPreset::SafeAsk),
        ..ClaudeConfig::default()
    };
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--" => {
                cfg.extra_args.extend_from_slice(&args[i + 1..]);
                break;
            }
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --socket requires a path");
                    return ExitCode::from(2);
                };
                socket_path = PathBuf::from(val);
            }
            "--cwd" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --cwd requires a directory");
                    return ExitCode::from(2);
                };
                cfg.cwd = Some(PathBuf::from(val));
            }
            "--config-dir" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --config-dir requires a directory");
                    return ExitCode::from(2);
                };
                cfg.config_dir = Some(PathBuf::from(val));
            }
            "--oauth-token" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --oauth-token requires a token");
                    return ExitCode::from(2);
                };
                cfg.oauth_token = Some(val.clone());
            }
            "--preset" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --preset requires a preset name");
                    return ExitCode::from(2);
                };
                match PermissionPreset::parse(val) {
                    Ok(p) => cfg.preset = Some(p),
                    Err(msg) => {
                        eprintln!("error: {msg}");
                        return ExitCode::from(2);
                    }
                }
            }
            "--sandbox" => {
                i += 1;
                match args.get(i).map(String::as_str) {
                    Some("on" | "true" | "1") => cfg.sandbox = Some(true),
                    Some("off" | "false" | "0") => cfg.sandbox = Some(false),
                    _ => {
                        eprintln!("error: --sandbox requires `on` or `off`");
                        return ExitCode::from(2);
                    }
                }
            }
            "--model" | "-m" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --model requires a value");
                    return ExitCode::from(2);
                };
                cfg.model = Some(val.clone());
            }
            "--permission-mode" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --permission-mode requires a value");
                    return ExitCode::from(2);
                };
                cfg.permission_mode = Some(val.clone());
            }
            "--tools" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --tools requires a value");
                    return ExitCode::from(2);
                };
                cfg.tools = Some(val.clone());
            }
            "--allowed-tool" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --allowed-tool requires a value");
                    return ExitCode::from(2);
                };
                cfg.allowed_tools.push(val.clone());
            }
            "--max-budget-usd" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --max-budget-usd requires a value");
                    return ExitCode::from(2);
                };
                cfg.max_budget_usd = Some(val.clone());
            }
            "--resume" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --resume requires a session id");
                    return ExitCode::from(2);
                };
                cfg.resume = Some(val.clone());
            }
            "--bin" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --bin requires a path");
                    return ExitCode::from(2);
                };
                cfg.binary = val.clone();
            }
            "--stdio-approvals" => cfg.stdio_approvals = true,
            "--auto-approve" => {
                cfg.stdio_approvals = true;
                cfg.auto_approve = true;
            }
            "--partials" => cfg.include_partials = true,
            "--quiet" | "-q" => cfg.echo_events = false,
            other => {
                eprintln!("error: unknown option '{other}' (put `--` before raw claude flags)");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    eprintln!(
        "console-attach claude: starting persistent `{}` stream-json session (socket: {})",
        cfg.binary,
        socket_path.display()
    );

    let daemon = match RunningAgentDaemon::start_claude(socket_path, cfg) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "console-attach claude: pid={} ready — send messages anytime via `console-attach claude send ...`",
        daemon.pid()
    );

    match daemon.wait() {
        Ok(status) => exit_code_from_status(status),
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_claude_send(args: &[String]) -> ExitCode {
    let mut socket_path = default_claude_socket_path();
    let mut wait = false;
    let mut timeout_ms: u64 = 120_000;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --socket requires a path");
                    return ExitCode::from(2);
                };
                socket_path = PathBuf::from(val);
            }
            "--wait" | "-w" => wait = true,
            "--timeout-ms" | "-t" => {
                i += 1;
                let Some(val) = args.get(i).and_then(|s| s.parse::<u64>().ok()) else {
                    eprintln!("error: --timeout-ms requires an integer");
                    return ExitCode::from(2);
                };
                timeout_ms = val;
            }
            "--" => {
                i += 1;
                break;
            }
            _ => break,
        }
        i += 1;
    }

    if i >= args.len() {
        eprintln!("error: send requires a prompt message");
        return ExitCode::from(2);
    }
    let message = args[i..].join(" ");
    let client = AgentClient::new(&socket_path);

    if wait {
        match client.send_wait(&message, Duration::from_millis(timeout_ms)) {
            Ok(ev) => {
                if let Some(text) = &ev.text {
                    println!("{text}");
                } else {
                    println!("{}", format_event_human(&ev));
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        }
    } else {
        match client.send(&message) {
            Ok(ev) => {
                println!("queued/sent seq={}", ev.seq);
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        }
    }
}

fn parse_claude_socket_opt(args: &[String]) -> Result<(PathBuf, usize), String> {
    let mut socket_path = default_claude_socket_path();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    return Err("--socket requires a path".to_string());
                };
                socket_path = PathBuf::from(val);
                i += 1;
            }
            _ => break,
        }
    }
    Ok((socket_path, i))
}

fn cmd_claude_interrupt(args: &[String]) -> ExitCode {
    let (socket_path, idx) = match parse_claude_socket_opt(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(2);
        }
    };
    if idx < args.len() {
        eprintln!("error: unexpected argument '{}'", args[idx]);
        return ExitCode::from(2);
    }
    let client = AgentClient::new(&socket_path);
    match client.interrupt() {
        Ok(val) => {
            println!("{val}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_claude_approve(args: &[String]) -> ExitCode {
    let (socket_path, idx) = match parse_claude_socket_opt(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(2);
        }
    };
    let Some(req_id) = args.get(idx) else {
        eprintln!("error: approve requires <request_id>");
        return ExitCode::from(2);
    };
    let client = AgentClient::new(&socket_path);
    match client.approve(req_id) {
        Ok(val) => {
            println!("{val}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_claude_deny(args: &[String]) -> ExitCode {
    let (socket_path, idx) = match parse_claude_socket_opt(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(2);
        }
    };
    let Some(req_id) = args.get(idx) else {
        eprintln!("error: deny requires <request_id> [reason...]");
        return ExitCode::from(2);
    };
    let reason = if idx + 1 < args.len() {
        Some(args[idx + 1..].join(" "))
    } else {
        None
    };
    let client = AgentClient::new(&socket_path);
    match client.deny(req_id, reason.as_deref()) {
        Ok(val) => {
            println!("{val}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_claude_status(args: &[String]) -> ExitCode {
    let mut socket_path = default_claude_socket_path();
    let mut as_json = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --socket requires a path");
                    return ExitCode::from(2);
                };
                socket_path = PathBuf::from(val);
            }
            "--json" => as_json = true,
            other => {
                eprintln!("error: unknown argument '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let client = AgentClient::new(&socket_path);
    match client.status() {
        Ok(status) => {
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&status).unwrap_or_default()
                );
            } else {
                println!(
                    "backend={} pid={} alive={} state={} turns={} last_seq={} session_id={}",
                    status.backend,
                    status.pid,
                    status.alive,
                    status.state,
                    status.turns_completed,
                    status.last_seq,
                    status.session_id.as_deref().unwrap_or("<none>")
                );
                for p in &status.pending_approvals {
                    println!(
                        "  pending approval: id={} tool={} input={}",
                        p.request_id, p.tool_name, p.input
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_claude_events(args: &[String]) -> ExitCode {
    let mut socket_path = default_claude_socket_path();
    let mut since_seq = 0u64;
    let mut raw = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --socket requires a path");
                    return ExitCode::from(2);
                };
                socket_path = PathBuf::from(val);
            }
            "--since" => {
                i += 1;
                let Some(val) = args.get(i).and_then(|s| s.parse::<u64>().ok()) else {
                    eprintln!("error: --since requires an integer");
                    return ExitCode::from(2);
                };
                since_seq = val;
            }
            "--raw" => raw = true,
            other => {
                eprintln!("error: unknown argument '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let client = AgentClient::new(&socket_path);
    match client.events(since_seq) {
        Ok(events) => {
            for ev in &events {
                if raw {
                    println!("{}", serde_json::to_string(ev).unwrap_or_default());
                } else {
                    println!("{}", format_event_human(ev));
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_claude_follow(args: &[String]) -> ExitCode {
    let mut socket_path = default_claude_socket_path();
    let mut since_seq = 0u64;
    let mut raw = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --socket requires a path");
                    return ExitCode::from(2);
                };
                socket_path = PathBuf::from(val);
            }
            "--since" => {
                i += 1;
                let Some(val) = args.get(i).and_then(|s| s.parse::<u64>().ok()) else {
                    eprintln!("error: --since requires an integer");
                    return ExitCode::from(2);
                };
                since_seq = val;
            }
            "--raw" => raw = true,
            other => {
                eprintln!("error: unknown argument '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let client = AgentClient::new(&socket_path);
    let mut cursor = since_seq;
    loop {
        match client.wait_events(cursor, Duration::from_secs(5)) {
            Ok(events) => {
                for ev in &events {
                    cursor = ev.seq;
                    if raw {
                        println!("{}", serde_json::to_string(ev).unwrap_or_default());
                    } else {
                        println!("{}", format_event_human(ev));
                    }
                }
                let _ = io::stdout().flush();
            }
            Err(err) => {
                eprintln!("console-attach claude follow ended: {err}");
                return ExitCode::SUCCESS;
            }
        }
    }
}

fn cmd_claude_stop(args: &[String]) -> ExitCode {
    let (socket_path, idx) = match parse_claude_socket_opt(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(2);
        }
    };
    if idx < args.len() {
        eprintln!("error: unexpected argument '{}'", args[idx]);
        return ExitCode::from(2);
    }
    let client = AgentClient::new(&socket_path);
    match client.stop() {
        Ok(val) => {
            println!("{val}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// PTY proxy commands (vi / arbitrary TUI programs)
// ---------------------------------------------------------------------------

fn cmd_run(args: &[String]) -> ExitCode {
    let mut socket_path = default_socket_path();
    let mut headless = false;
    let mut rows: Option<u16> = None;
    let mut cols: Option<u16> = None;
    let mut term: Option<String> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--" => {
                i += 1;
                break;
            }
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --socket requires a path");
                    return ExitCode::from(2);
                };
                socket_path = PathBuf::from(val);
            }
            "--headless" => headless = true,
            "--rows" => {
                i += 1;
                let Some(val) = args.get(i).and_then(|s| s.parse::<u16>().ok()) else {
                    eprintln!("error: --rows requires a positive integer");
                    return ExitCode::from(2);
                };
                rows = Some(val);
            }
            "--cols" => {
                i += 1;
                let Some(val) = args.get(i).and_then(|s| s.parse::<u16>().ok()) else {
                    eprintln!("error: --cols requires a positive integer");
                    return ExitCode::from(2);
                };
                cols = Some(val);
            }
            "--term" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --term requires a value");
                    return ExitCode::from(2);
                };
                term = Some(val.clone());
            }
            flag if flag.starts_with('-') => {
                eprintln!("error: unknown flag '{flag}' (put `--` before program flags)");
                return ExitCode::from(2);
            }
            _ => break,
        }
        i += 1;
    }

    if i >= args.len() {
        eprintln!("error: missing <program> to run");
        return ExitCode::from(2);
    }

    let program = args[i].clone();
    let prog_args = args[i + 1..].to_vec();
    let size = match (rows, cols) {
        (Some(r), Some(c)) => Some(PtySize { rows: r, cols: c }),
        (Some(r), None) => Some(PtySize { rows: r, cols: 80 }),
        (None, Some(c)) => Some(PtySize { rows: 24, cols: c }),
        (None, None) => None,
    };

    if headless {
        eprintln!(
            "console-attach: running `{}` headless (socket: {})",
            program,
            socket_path.display()
        );
    }

    let config = ProxyConfig {
        program,
        args: prog_args,
        socket_path,
        headless,
        size,
        term,
    };

    match run_proxy(config) {
        Ok(status) => exit_code_from_status(status),
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn parse_socket_opt(args: &[String]) -> Result<(PathBuf, usize), String> {
    let mut socket_path = default_socket_path();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    return Err("--socket requires a path".to_string());
                };
                socket_path = PathBuf::from(val);
                i += 1;
            }
            _ => break,
        }
    }
    Ok((socket_path, i))
}

fn cmd_send(args: &[String]) -> ExitCode {
    let (socket_path, idx) = match parse_socket_opt(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(2);
        }
    };
    if idx >= args.len() {
        eprintln!("error: send requires a command (e.g. `vi:save` or `type hello`)");
        return ExitCode::from(2);
    }
    let command = args[idx..].join(" ");
    let client = ControlClient::new(&socket_path);
    match client.send_command(&command) {
        Ok(reply) => {
            println!("{reply}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_screen(args: &[String]) -> ExitCode {
    let mut socket_path = default_socket_path();
    let mut raw = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --socket requires a path");
                    return ExitCode::from(2);
                };
                socket_path = PathBuf::from(val);
            }
            "--raw" => raw = true,
            other => {
                eprintln!("error: unknown argument '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let client = ControlClient::new(&socket_path);
    if raw {
        match client.raw() {
            Ok(bytes) => {
                let _ = io::stdout().write_all(&bytes);
                let _ = io::stdout().flush();
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        }
    } else {
        match client.screen() {
            Ok(text) => {
                print!("{text}");
                if !text.ends_with('\n') {
                    println!();
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        }
    }
}

fn cmd_wait_for(args: &[String]) -> ExitCode {
    let mut socket_path = default_socket_path();
    let mut timeout_ms: u64 = 5_000;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" | "-s" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("error: --socket requires a path");
                    return ExitCode::from(2);
                };
                socket_path = PathBuf::from(val);
            }
            "--timeout-ms" | "-t" => {
                i += 1;
                let Some(val) = args.get(i).and_then(|s| s.parse::<u64>().ok()) else {
                    eprintln!("error: --timeout-ms requires an integer");
                    return ExitCode::from(2);
                };
                timeout_ms = val;
            }
            _ => break,
        }
        i += 1;
    }
    if i >= args.len() {
        eprintln!("error: wait-for requires a <substring>");
        return ExitCode::from(2);
    }
    let needle = args[i..].join(" ");
    let client = ControlClient::new(&socket_path);
    match client.wait_for(Duration::from_millis(timeout_ms), &needle) {
        Ok(reply) => {
            println!("{reply}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_status(args: &[String]) -> ExitCode {
    let (socket_path, idx) = match parse_socket_opt(args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::from(2);
        }
    };
    if idx < args.len() {
        eprintln!("error: unexpected argument '{}'", args[idx]);
        return ExitCode::from(2);
    }
    let client = ControlClient::new(&socket_path);
    match client.status() {
        Ok(reply) => {
            println!("{reply}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_demo_vi(args: &[String]) -> ExitCode {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let pid = std::process::id();

    let file_path = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/ca-vi-demo-{pid}-{}.txt", stamp % 100_000)));
    let socket_path = PathBuf::from(format!("/tmp/ca-demo-{pid}-{}.sock", stamp % 100_000));

    println!("1. Spawning real `vi` inside an intercepted headless PTY...");
    println!("   target file : {}", file_path.display());
    println!("   control sock: {}", socket_path.display());

    let proxy = match RunningProxy::start(ProxyConfig {
        program: "vi".to_string(),
        args: vec![
            "-u".to_string(),
            "NONE".to_string(),
            "-n".to_string(),
            file_path.to_string_lossy().into_owned(),
        ],
        socket_path: socket_path.clone(),
        headless: true,
        size: Some(PtySize { rows: 24, cols: 80 }),
        term: Some("xterm-256color".to_string()),
    }) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("error starting vi in PTY: {err}");
            return ExitCode::FAILURE;
        }
    };

    if !proxy.wait_for_any_output(Duration::from_secs(3)) {
        eprintln!("error: vi produced no terminal output within 3s");
        let _ = proxy.kill_and_wait();
        return ExitCode::FAILURE;
    }
    thread::sleep(Duration::from_millis(150));

    let client = ControlClient::new(&socket_path);
    let steps = [
        (
            "vi:insert Hello from Rust PTY proxy!",
            "Enter insert mode, type line 1, ESC",
        ),
        (
            "vi:append-line Saved via intercepted TTY control socket.",
            "Open new line, type line 2, ESC",
        ),
        ("vi:save", "Translate `vi:save` -> ESC + `:w\\r`"),
        ("vi:save-quit", "Translate `vi:save-quit` -> ESC + `:wq\\r`"),
    ];

    println!("2. Driving `vi` over the Unix control socket:");
    for (cmd, desc) in steps {
        match client.send_command(cmd) {
            Ok(reply) => println!("   -> `{cmd}` ({desc}) => {reply}"),
            Err(err) => {
                eprintln!("error sending `{cmd}`: {err}");
                let _ = proxy.kill_and_wait();
                return ExitCode::FAILURE;
            }
        }
        thread::sleep(Duration::from_millis(120));
    }

    let status = match proxy.wait() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error waiting for vi: {err}");
            return ExitCode::FAILURE;
        }
    };
    println!("3. `vi` exited cleanly with status: {status}");

    match fs::read_to_string(&file_path) {
        Ok(contents) => {
            println!(
                "4. Verified file written by `vi` ({}):",
                file_path.display()
            );
            println!("---");
            print!("{contents}");
            println!("---");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error reading {}: {err}", file_path.display());
            ExitCode::FAILURE
        }
    }
}
