//! The operator CLI's result contracts (REQ-CLI-01..08), process-level and hermetic: every
//! run has its own home, account base, configuration and cache directories, a clean
//! environment (no ambient color, completion or log variables) and needs no running host.
//!
//! Covered: the five canaries (missing flags, a failure under `--json`, JSON in a pipe, `-q`,
//! a destructive command without `--yes`), a closed standard output, escaping of untrusted
//! text in every mode, the color matrix, the five-tier instance/office selection and the
//! strict `agentic-org.toml`, `--no-input` and Ctrl+C on a real terminal, and dynamic
//! completion with its cache under concurrent writers.

use std::fs;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_agentic-org");
const OFFICE: &str = "0193a5c0-0000-7000-8000-000000000001";
const OTHER_OFFICE: &str = "0193a5c0-0000-7000-8000-000000000002";
const INSTANCE_ID: &str = "0193a5c0-0000-7000-8000-0000000000aa";

struct Sandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for sub in ["home", "base", "cache", "config", "work"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        Self { _dir: dir, root }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(BIN);
        command
            .args(args)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("ORG_DEV_ACCOUNT_BASE", self.root.join("base"))
            .env("ORG_OFFICE_DEV_KEY_DIR", self.root.join("keys"))
            .env("AGENTIC_ORG_CACHE_DIR", self.root.join("cache"))
            .env("AGENTIC_ORG_CONFIG_DIR", self.root.join("config"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .current_dir(self.root.join("work"))
            .stdin(Stdio::null());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// The saved context: under `AGENTIC_ORG_CONFIG_DIR`, which wins over the account base.
    fn context_file(&self) -> PathBuf {
        self.root.join("config/cli-context.json")
    }

    fn save_context(&self, instance: Option<&str>, office: &str) {
        let mut saved = serde_json::json!({"instance_id": INSTANCE_ID, "office_id": office});
        if let Some(name) = instance {
            saved["instance_name"] = Value::from(name);
        }
        fs::write(self.context_file(), saved.to_string()).unwrap();
    }

    fn cache_file(&self, partition: &[&str], kind: &str, names: &[&str]) {
        let mut dir = self.root.join("cache/completion");
        for part in partition {
            dir.push(part);
        }
        fs::create_dir_all(&dir).unwrap();
        let list: Vec<Value> = names
            .iter()
            .map(|name| serde_json::json!({"name": name}))
            .collect();
        fs::write(
            dir.join(format!("{kind}.json")),
            Value::from(list).to_string(),
        )
        .unwrap();
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The one JSON document on standard error.
fn error_envelope(output: &Output) -> Value {
    let stderr = text(&output.stderr);
    assert_eq!(
        stderr.trim_end().lines().count(),
        1,
        "one line in a pipe: {stderr}"
    );
    serde_json::from_str(stderr.trim_end()).unwrap_or_else(|e| panic!("{e}: {stderr}"))
}

fn assert_no_raw_controls(what: &str, shown: &str) {
    for c in ['\u{1b}', '\u{9b}', '\u{7}', '\u{7f}', '\u{2028}'] {
        assert!(!shown.contains(c), "{what}: raw {c:?} in {shown:?}");
    }
}

#[test]
fn canary_missing_required_flags_is_a_usage_error() {
    let sandbox = Sandbox::new();
    let human = sandbox.run(&["org", "create", "Acme"]);
    assert_eq!(human.status.code(), Some(2));
    assert!(human.stdout.is_empty());
    assert!(
        text(&human.stderr).contains("--slug"),
        "{}",
        text(&human.stderr)
    );
    let json = sandbox.run(&["--json", "org", "create", "Acme"]);
    assert_eq!(json.status.code(), Some(2));
    assert!(json.stdout.is_empty());
    let envelope = error_envelope(&json);
    assert_eq!(envelope["error"]["code"], "cli::usage");
    assert_eq!(envelope["error"]["exit_code"], 2);
}

#[test]
fn canary_a_failure_under_json_is_one_escaped_envelope_on_stderr() {
    let sandbox = Sandbox::new();
    let hostile = format!(
        "{}/missing\u{9b}31m\u{1b}]0;owned\u{7}.tar.gz",
        sandbox.root.display()
    );
    let store = sandbox.root.join("store");
    let store = store.to_str().unwrap();
    let failed = sandbox.run(&["--json", "release", "install", &hostile, "--store", store]);
    assert_eq!(failed.status.code(), Some(1), "{}", text(&failed.stderr));
    assert!(failed.stdout.is_empty());
    assert_no_raw_controls("stderr", &text(&failed.stderr));
    let envelope = error_envelope(&failed);
    assert_eq!(envelope["error"]["exit_code"], 1);
    assert!(envelope["error"]["code"].as_str().unwrap().contains("::"));
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("missing\u{9b}31m"),
        "the value reads back: {message:?}"
    );

    let unknown = sandbox.run(&["--json", "org", "list", "--name", "ghost"]);
    assert_eq!(unknown.status.code(), Some(1), "{}", text(&unknown.stderr));
    assert!(unknown.stdout.is_empty());
    let envelope = error_envelope(&unknown);
    assert_ne!(envelope["error"]["code"], "cli::not_ready");
}

#[test]
fn canary_json_in_a_pipe_is_one_compact_line_without_color() {
    let sandbox = Sandbox::new();
    // A debug build cannot verify a release store it would open: a store that does not
    // exist yet is the answer every build gives.
    let store = sandbox.root.join("no-store-yet");
    for args in [
        vec![
            "release",
            "status",
            "--store",
            store.to_str().unwrap(),
            "--json",
        ],
        vec!["--json", "--color", "always", "status"],
    ] {
        let output = sandbox.run(&args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            text(&output.stderr)
        );
        let stdout = text(&output.stdout);
        assert_eq!(stdout.lines().count(), 1, "{args:?}: {stdout}");
        assert!(
            stdout.ends_with('\n') && !stdout.contains('\u{1b}'),
            "{stdout:?}"
        );
        serde_json::from_str::<Value>(&stdout).unwrap();
    }
}

#[test]
fn canary_quiet_prints_only_the_ids() {
    let sandbox = Sandbox::new();
    sandbox.save_context(None, OFFICE);
    let shown = sandbox.run(&["context", "show", "-q"]);
    assert_eq!(text(&shown.stdout), format!("{OFFICE}\n"));
    assert!(shown.stderr.is_empty(), "{}", text(&shown.stderr));
    let status = sandbox.run(&["status", "--quiet"]);
    assert_eq!(text(&status.stdout), "default\n");
    let conflict = sandbox.run(&["status", "-q", "--json"]);
    assert_eq!(conflict.status.code(), Some(2));
}

#[test]
fn canary_destructive_commands_need_yes_without_a_terminal() {
    let sandbox = Sandbox::new();
    sandbox.save_context(None, OFFICE);
    for args in [
        &["office", "remove", OFFICE, "--org", OTHER_OFFICE][..],
        &["office", "rm", OFFICE],
        &["office", "cancel", "inv-1", "--org", OTHER_OFFICE],
        &["office", "decline", "inv-1"],
        &["office", "restart", OFFICE],
        &["office", "remove-pending", OFFICE],
        &["context", "clear"],
        &["service", "stop"],
    ] {
        let refused = sandbox.run(args);
        assert_eq!(
            refused.status.code(),
            Some(2),
            "{args:?}: {}",
            text(&refused.stderr)
        );
        let stderr = text(&refused.stderr);
        assert!(
            stderr.contains("confirmation required") && stderr.contains("--yes"),
            "{stderr}"
        );
        let mut json_args = vec!["--json"];
        json_args.extend_from_slice(args);
        let envelope = error_envelope(&sandbox.run(&json_args));
        assert_eq!(
            envelope["error"]["code"], "cli::confirmation_required",
            "{args:?}"
        );
    }
    assert!(
        sandbox.context_file().exists(),
        "a refused clear changed nothing"
    );
    let preview = sandbox.run(&["--dry-run", "context", "clear"]);
    assert_eq!(preview.status.code(), Some(0));
    assert!(
        text(&preview.stderr).contains("[DRY-RUN]"),
        "{}",
        text(&preview.stderr)
    );
    assert!(sandbox.context_file().exists(), "a dry run changed nothing");
    let cleared = sandbox.run(&["context", "clear", "--yes"]);
    assert_eq!(cleared.status.code(), Some(0), "{}", text(&cleared.stderr));
    assert!(!sandbox.context_file().exists());
}

/// B-01 (REQ-CLI-06): `AGENTIC_ORG_CONFIG_DIR` alone decides where the saved context lives:
/// `context show`, `context clear` and `status` read and clear it there without
/// `ORG_DEV_ACCOUNT_BASE`, and a relative `AGENTIC_ORG_CONFIG_DIR` or `AGENTIC_ORG_CACHE_DIR`
/// is refused (exit 1, `cli::config::invalid`) before anything is read or made.
#[test]
fn the_config_dir_variable_isolates_the_saved_context_without_a_dev_base() {
    let sandbox = Sandbox::new();
    // `status` also reports the instance's service, which opens the account's own instance
    // folders. Where the system lets variables move them (Linux: XDG), they are moved into
    // the sandbox and `status` runs without any development base too; where none can
    // (macOS), `status` gets a decoy development base holding another context instead, which
    // must not be the one it shows.
    let moved = org_platform::dirs::location_variables().unwrap();
    let decoy = sandbox.root.join("decoy");
    fs::create_dir_all(&decoy).unwrap();
    fs::write(
        decoy.join("cli-context.json"),
        serde_json::json!({"instance_id": INSTANCE_ID, "office_id": OTHER_OFFICE}).to_string(),
    )
    .unwrap();
    let run = |args: &[&str], extra: &[(&str, &str)]| {
        let mut command = sandbox.command(args);
        command.env_remove("ORG_DEV_ACCOUNT_BASE");
        for name in moved {
            command.env(name, sandbox.root.join("xdg").join(name));
        }
        if moved.is_empty() && args.first() == Some(&"status") {
            command.env("ORG_DEV_ACCOUNT_BASE", &decoy);
        }
        command.envs(extra.iter().copied());
        command.output().unwrap()
    };
    let json = |output: &Output| -> Value {
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
        serde_json::from_slice(&output.stdout).unwrap()
    };
    assert_eq!(json(&run(&["context", "show", "--json"], &[])), Value::Null);
    sandbox.save_context(Some("lab"), OFFICE);
    let shown = json(&run(&["context", "show", "--json"], &[]));
    assert_eq!(
        (shown["office_id"].as_str(), shown["instance_name"].as_str()),
        (Some(OFFICE), Some("lab"))
    );
    let status = json(&run(&["status", "--json"], &[]));
    assert_eq!(status["office"], OFFICE, "{status}");
    assert_eq!(status["office_source"], "saved", "{status}");
    assert_eq!(status["instance"], "lab", "{status}");
    assert_eq!(status["saved_context"]["office_id"], OFFICE, "{status}");
    let cleared = json(&run(&["context", "clear", "--yes", "--json"], &[]));
    assert_eq!(cleared, serde_json::json!({"cleared": true}));
    assert!(!sandbox.context_file().exists());
    assert!(
        decoy.join("cli-context.json").exists(),
        "only the config dir is used"
    );
    assert!(!sandbox.root.join("base/cli-context.json").exists());
    assert!(!sandbox.root.join("home/cli-context.json").exists());
    // A relative directory variable is refused, never read against the working directory.
    sandbox.save_context(Some("lab"), OFFICE);
    for (variable, value) in [
        ("AGENTIC_ORG_CONFIG_DIR", "relative/config"),
        ("AGENTIC_ORG_CACHE_DIR", "relative/cache"),
    ] {
        for args in [
            &["context", "show"][..],
            &["context", "clear", "--yes"],
            &["context", "use", "--office", OFFICE],
            &["status"],
        ] {
            let refused = run(args, &[(variable, value)]);
            let stderr = text(&refused.stderr);
            assert_eq!(
                refused.status.code(),
                Some(1),
                "{variable} {args:?}: {stderr}"
            );
            assert!(stderr.contains(variable), "{variable} {args:?}: {stderr}");
            assert!(refused.stdout.is_empty(), "{variable} {args:?}");
        }
        let envelope = error_envelope(&run(&["--json", "status"], &[(variable, value)]));
        assert_eq!(
            envelope["error"]["code"], "cli::config::invalid",
            "{envelope}"
        );
        assert_eq!(envelope["error"]["exit_code"], 1, "{envelope}");
    }
    assert!(
        sandbox.context_file().exists(),
        "a refused clear changed nothing"
    );
    assert!(
        fs::read_dir(sandbox.root.join("work"))
            .unwrap()
            .next()
            .is_none(),
        "something was made relative to the working directory"
    );
}

/// B-06: a saved context edited to carry a control or bidirectional character in its
/// instance name fails closed (exit 1) for the commands that select by it, `context clear`
/// still removes it, and the name is never printed.
#[test]
fn a_poisoned_saved_context_fails_closed_and_can_be_cleared() {
    let sandbox = Sandbox::new();
    for name in ["lab\u{1b}[2J", "lab\u{202e}gnp"] {
        sandbox.save_context(Some(name), OFFICE);
        for args in [
            &["status"][..],
            &["context", "show"],
            &["service", "status"],
        ] {
            let refused = sandbox.run(args);
            let stderr = text(&refused.stderr);
            assert_eq!(refused.status.code(), Some(1), "{args:?}: {stderr}");
            assert!(
                !stderr.contains('\u{1b}') && !stderr.contains('\u{202e}'),
                "{stderr:?}"
            );
            assert!(refused.stdout.is_empty(), "{args:?}");
        }
        let cleared = sandbox.run(&["context", "clear", "--yes"]);
        assert_eq!(cleared.status.code(), Some(0), "{}", text(&cleared.stderr));
        assert!(!sandbox.context_file().exists());
    }
}

#[test]
fn a_closed_standard_output_exits_0_quietly() {
    let sandbox = Sandbox::new();
    sandbox.save_context(None, OFFICE);
    let store = sandbox.root.join("no-store-yet");
    let store = store.to_str().unwrap();
    for args in [
        &["--help"][..],
        &["--version"],
        &["status"],
        &["status", "--json"],
        &["completion", "zsh"],
        &["release", "status", "--store", store],
        &["context", "show"],
        &["org", "create", "--help"],
        &["task", "list", "--help"],
        &["help", "office"],
    ] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let output = sandbox.command(args).stdout(writer).output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
        assert!(
            output.stderr.is_empty(),
            "{args:?}: {}",
            text(&output.stderr)
        );
    }
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let completion = sandbox
        .command(&["--", "agentic-org", "o"])
        .env("COMPLETE", "fish")
        .stdout(writer)
        .output()
        .unwrap();
    assert_eq!(completion.status.code(), Some(0), "{completion:?}");
}

#[test]
fn untrusted_text_is_escaped_in_tables_cards_json_errors_and_completions() {
    let sandbox = Sandbox::new();
    // A store that does not exist yet: its path is shown back as given.
    let hostile = sandbox.root.join("store\u{1b}[31m\u{9b}2J\u{2028}x");
    let hostile = hostile.to_str().unwrap();
    let card = sandbox.run(&["release", "status", "--store", hostile]);
    let shown = text(&card.stdout);
    assert_no_raw_controls("card", &shown);
    assert!(
        shown.contains("store\\u{1b}[31m\\u{9b}2J\\u{2028}x"),
        "{shown}"
    );
    let json = sandbox.run(&["release", "status", "--store", hostile, "--json"]);
    let stdout = text(&json.stdout);
    assert_no_raw_controls("json", &stdout);
    assert!(
        stdout.contains("\\u009b") && stdout.contains("\\u2028"),
        "{stdout}"
    );
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["store"], hostile);
    let error = sandbox.run(&["release", "status", "--store", "relative\u{1b}[2J\u{9b}"]);
    assert_eq!(error.status.code(), Some(2));
    assert_no_raw_controls("error", &text(&error.stderr));
    assert!(
        text(&error.stderr).contains("\\u{1b}[2J"),
        "{}",
        text(&error.stderr)
    );

    sandbox.cache_file(&[], "instances", &["good", "bad\u{1b}[31m", "worse\u{9b}"]);
    let completion = sandbox
        .command(&["--", "agentic-org", "status", "--name", ""])
        .env("COMPLETE", "fish")
        .output()
        .unwrap();
    let offered = text(&completion.stdout);
    assert!(
        offered.lines().any(|line| line.starts_with("good")),
        "{offered}"
    );
    assert_no_raw_controls("completion", &offered);
    assert!(
        !offered.contains("bad") && !offered.contains("worse"),
        "{offered}"
    );
}

#[test]
fn the_color_matrix_decides_ansi_for_each_stream() {
    let sandbox = Sandbox::new();
    let base = ["release", "status", "--store", "relative"];
    for (flags, env, colored) in [
        (&[][..], &[][..], false),
        (&["--color", "always"], &[], true),
        (&[], &[("FORCE_COLOR", "1")], true),
        (&[], &[("CLICOLOR_FORCE", "1")], true),
        (&[], &[("FORCE_COLOR", "0")], false),
        (&[], &[("FORCE_COLOR", "1"), ("NO_COLOR", "1")], false),
        (&["--no-color"], &[("FORCE_COLOR", "1")], false),
        (&["--color", "never"], &[("FORCE_COLOR", "1")], false),
        (&["--color", "always", "--json"], &[], false),
    ] {
        let mut args: Vec<&str> = flags.to_vec();
        args.extend(base);
        let mut command = sandbox.command(&args);
        command.envs(env.iter().copied());
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{flags:?} {env:?}");
        let stderr = text(&output.stderr);
        assert_eq!(
            stderr.contains('\u{1b}'),
            colored,
            "{flags:?} {env:?}: {stderr:?}"
        );
        assert!(output.stdout.is_empty());
    }
    // NB-06: `agentic-org` alone prints its help on standard output, colored as standard
    // output is (a pipe here: only `--color always` or a forcing variable colors it).
    for (flags, env, colored) in [
        (&["--color", "always"][..], &[][..], true),
        (&[], &[("FORCE_COLOR", "1")], true),
        (&[], &[], false),
        (&["--color", "never"], &[("FORCE_COLOR", "1")], false),
    ] {
        let mut command = sandbox.command(flags);
        command.envs(env.iter().copied());
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{flags:?} {env:?}");
        let stdout = text(&output.stdout);
        assert!(stdout.contains("Usage"), "{stdout:?}");
        assert_eq!(
            stdout.contains('\u{1b}'),
            colored,
            "{flags:?} {env:?}: {stdout:?}"
        );
    }
}

fn selected(sandbox: &Sandbox, args: &[&str], env: &[(&str, &str)]) -> Value {
    let mut all = vec!["status", "--json"];
    all.extend_from_slice(args);
    let mut command = sandbox.command(&all);
    command.envs(env.iter().copied());
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{args:?}: {}",
        text(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn instance_and_office_are_selected_in_five_tiers() {
    let sandbox = Sandbox::new();
    let pick = |view: &Value| {
        (
            view["instance"].as_str().unwrap().to_owned(),
            view["instance_source"].as_str().unwrap().to_owned(),
            view["office"].as_str().map(str::to_owned),
            view["office_source"].as_str().unwrap().to_owned(),
        )
    };
    let tiers = |instance: &str, source: &str, office: Option<&str>, office_source: &str| {
        (
            instance.to_owned(),
            source.to_owned(),
            office.map(str::to_owned),
            office_source.to_owned(),
        )
    };
    assert_eq!(
        pick(&selected(&sandbox, &[], &[])),
        tiers("default", "default", None, "default")
    );
    sandbox.save_context(Some("saved"), OFFICE);
    assert_eq!(
        pick(&selected(&sandbox, &[], &[])),
        tiers("saved", "saved", Some(OFFICE), "saved")
    );
    fs::create_dir_all(sandbox.root.join("work/nested")).unwrap();
    fs::write(
        sandbox.root.join("work/agentic-org.toml"),
        format!("instance = \"manifest\"\noffice = \"{OTHER_OFFICE}\"\n"),
    )
    .unwrap();
    let mut nested = sandbox.command(&["status", "--json"]);
    nested.current_dir(sandbox.root.join("work/nested"));
    let view: Value = serde_json::from_slice(&nested.output().unwrap().stdout).unwrap();
    assert_eq!(
        pick(&view),
        tiers("manifest", "manifest", Some(OTHER_OFFICE), "manifest")
    );
    let env = [
        ("AGENTIC_ORG_INSTANCE", "env"),
        ("AGENTIC_ORG_OFFICE", OFFICE),
    ];
    assert_eq!(
        pick(&selected(&sandbox, &[], &env)),
        tiers("env", "env", Some(OFFICE), "env")
    );
    let flags = ["--name", "flag", "--office", OTHER_OFFICE];
    assert_eq!(
        pick(&selected(&sandbox, &flags, &env)),
        tiers("flag", "flag", Some(OTHER_OFFICE), "flag")
    );
    // An empty variable is unset.
    let empty = [("AGENTIC_ORG_INSTANCE", "")];
    assert_eq!(pick(&selected(&sandbox, &[], &empty)).1, "manifest");
}

#[test]
fn a_malformed_manifest_is_refused_without_echoing_it_and_never_blocks_completion() {
    let sandbox = Sandbox::new();
    for manifest in [
        "instance = \"ok\"\napi_token = \"s3cr3t-do-not-print\"\n",
        "instance = \"bell\\u0007s3cr3t-do-not-print\"\n",
        "instance = s3cr3t-do-not-print\n",
    ] {
        fs::write(sandbox.root.join("work/agentic-org.toml"), manifest).unwrap();
        for json in [false, true] {
            let args: &[&str] = if json {
                &["--json", "status"]
            } else {
                &["status"]
            };
            let refused = sandbox.run(args);
            assert_eq!(refused.status.code(), Some(1), "{manifest:?}");
            let stderr = text(&refused.stderr);
            assert!(stderr.contains("cli::config::invalid"), "{stderr}");
            assert!(
                !stderr.contains("s3cr3t"),
                "the manifest's text was echoed: {stderr}"
            );
            assert!(refused.stdout.is_empty());
        }
        let completion = sandbox.run(&["completion", "bash"]);
        assert_eq!(completion.status.code(), Some(0));
        assert!(!completion.stdout.is_empty());
    }
}

/// A child on a new pseudo-terminal that is its controlling terminal (standard input,
/// output and error are the terminal), and one reader of everything it shows.
struct Terminal {
    child: std::process::Child,
    master: std::fs::File,
    shown: std::sync::mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
}

impl Terminal {
    fn start(mut command: Command) -> Self {
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: openpty only writes the two descriptors; no name, termios or size.
        let opened = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(opened, 0, "openpty: {}", std::io::Error::last_os_error());
        // SAFETY: both descriptors are new and owned only here.
        let (master, slave) =
            unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        // SAFETY: a plain flag change on a descriptor this function owns.
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
        command
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        // SAFETY: between fork and exec only async-signal-safe calls: a new session, and
        // the terminal on standard input as its controlling terminal.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().unwrap();
        drop(command);
        let master = std::fs::File::from(master);
        let (send, shown) = std::sync::mpsc::channel();
        let mut reader = master.try_clone().unwrap();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(n @ 1..) = reader.read(&mut buffer) {
                if send.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            master,
            shown,
            seen: Vec::new(),
        }
    }

    /// Everything shown so far, after waiting until `until` is in it (or `deadline`).
    fn read_until(&mut self, until: &str, deadline: Duration) -> String {
        let start = Instant::now();
        while start.elapsed() < deadline && !String::from_utf8_lossy(&self.seen).contains(until) {
            if let Ok(chunk) = self.shown.recv_timeout(Duration::from_millis(50)) {
                self.seen.extend(chunk);
            }
        }
        String::from_utf8_lossy(&self.seen).into_owned()
    }
}

fn wait(child: &mut std::process::Child) -> Option<i32> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    panic!("the child did not end");
}

#[test]
fn no_input_never_prompts_on_a_terminal() {
    let sandbox = Sandbox::new();
    sandbox.save_context(None, OFFICE);
    for (args, env) in [
        (&["context", "clear", "--no-input"][..], None),
        (&["context", "clear"], Some(("AGENTIC_ORG_NO_INPUT", "1"))),
        (&["office", "remove", OFFICE, "--no-input"], None),
    ] {
        let mut command = sandbox.command(args);
        command.envs(env);
        let mut terminal = Terminal::start(command);
        let shown = terminal.read_until("--yes", Duration::from_secs(10));
        assert_eq!(wait(&mut terminal.child), Some(2), "{args:?}: {shown}");
        assert!(shown.contains("confirmation required"), "{shown}");
        assert!(sandbox.context_file().exists());
    }
}

#[test]
fn ctrl_c_at_a_confirmation_exits_130_and_restores_the_cursor() {
    let sandbox = Sandbox::new();
    sandbox.save_context(None, OFFICE);
    let mut terminal = Terminal::start(sandbox.command(&["context", "clear"]));
    let prompt = terminal.read_until("Clear the saved CLI context", Duration::from_secs(10));
    assert!(prompt.contains("Clear the saved CLI context"), "{prompt:?}");
    let hidden = prompt
        .rfind("\u{1b}[?25l")
        .expect("the prompt hides the cursor");
    terminal.master.write_all(b"\x03").unwrap();
    assert_eq!(wait(&mut terminal.child), Some(130), "{prompt:?}");
    let shown = terminal.read_until("\u{1b}[?25h", Duration::from_secs(5));
    assert!(
        shown[hidden..].contains("\u{1b}[?25h"),
        "the cursor is shown again after Ctrl+C: {shown:?}"
    );
    assert!(
        sandbox.context_file().exists(),
        "a cancelled clear changed nothing"
    );
}

fn complete(sandbox: &Sandbox, words: &[&str]) -> Vec<String> {
    let mut args = vec!["--", "agentic-org"];
    args.extend_from_slice(words);
    let output = sandbox
        .command(&args)
        .env("COMPLETE", "fish")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    text(&output.stdout)
        .lines()
        .map(|line| line.split('\t').next().unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn dynamic_completion_reads_the_typed_instances_partition() {
    let sandbox = Sandbox::new();
    sandbox.cache_file(&[], "instances", &["default", "lab"]);
    sandbox.cache_file(&["default"], "orgs", &["org-default"]);
    sandbox.cache_file(&["lab"], "orgs", &["org-lab"]);
    assert_eq!(
        complete(&sandbox, &["office", "list", "--org", ""]),
        ["org-default"]
    );
    for typed in [
        &["office", "list", "--name", "lab", "--org", ""][..],
        &["office", "list", "--name=lab", "--org", ""],
        &["office", "list", "--instance", "lab", "--org", ""],
    ] {
        assert_eq!(complete(&sandbox, typed), ["org-lab"], "{typed:?}");
    }
    assert_eq!(
        complete(&sandbox, &["status", "--name", ""]),
        ["default", "lab"]
    );
    assert!(complete(&sandbox, &["off"]).contains(&"office".to_owned()));
}

#[test]
fn thirty_concurrent_cache_writers_never_show_a_torn_list() {
    let sandbox = Sandbox::new();
    let cache = org_cli_shell::cache::CompletionCache::new(sandbox.root.join("cache"));
    std::thread::scope(|scope| {
        for writer in 0..30 {
            let cache = &cache;
            scope.spawn(move || {
                let names: Vec<_> = (0..40)
                    .map(|n| org_cli_shell::cache::CachedCandidate {
                        name: format!("w{writer}-{n}"),
                        help: None,
                    })
                    .collect();
                cache.store(&["default"], "orgs", &names).unwrap();
            });
        }
        for _ in 0..5 {
            let offered = complete(&sandbox, &["office", "list", "--org", ""]);
            if let Some(first) = offered.first() {
                let writer = first.split('-').next().unwrap();
                assert_eq!(offered.len(), 40, "{offered:?}");
                assert!(offered
                    .iter()
                    .all(|name| name.starts_with(&format!("{writer}-"))));
            }
        }
    });
    let offered = complete(&sandbox, &["office", "list", "--org", ""]);
    assert_eq!(offered.len(), 40);
    let leftovers: Vec<_> = fs::read_dir(sandbox.root.join("cache/completion/default"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.ends_with(".json") && !name.ends_with(".lock"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
}

#[test]
fn global_flags_a_command_cannot_honour_are_refused_and_previews_change_nothing() {
    let sandbox = Sandbox::new();
    for args in [
        &["--dry-run", "setup"][..],
        &["setup", "--json"],
        &["--quiet", "serve"],
        &["--dry-run", "recovery", "status"],
        &["sign-in", "--dry-run"],
    ] {
        let refused = sandbox.run(args);
        assert_eq!(
            refused.status.code(),
            Some(2),
            "{args:?}: {}",
            text(&refused.stderr)
        );
        assert!(
            text(&refused.stderr).contains("does not take"),
            "{}",
            text(&refused.stderr)
        );
    }
    assert!(
        fs::read_dir(sandbox.root.join("base"))
            .unwrap()
            .next()
            .is_none(),
        "a refused command made something"
    );
    let preview = sandbox.run(&[
        "--dry-run",
        "--json",
        "office",
        "approve",
        "inv-1",
        "--code",
        "1234",
    ]);
    assert_eq!(preview.status.code(), Some(0), "{}", text(&preview.stderr));
    let plan: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(plan["dry_run"], true);
    assert_eq!(plan["action"], "office_approve");
    let zero_ttl = sandbox.run(&[
        "office",
        "invite",
        "Berlin",
        "--address",
        "hq:7443",
        "--ttl",
        "0",
    ]);
    assert_eq!(zero_ttl.status.code(), Some(2));
}

#[test]
fn a_failing_office_command_keeps_its_exit_code_on_a_closed_pipe() {
    let sandbox = Sandbox::new();
    let open = sandbox.run(&["task", "list"]);
    assert_eq!(open.status.code(), Some(1), "{}", text(&open.stderr));
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let closed = sandbox
        .command(&["task", "list"])
        .stdout(writer)
        .output()
        .unwrap();
    assert_eq!(closed.status.code(), Some(1), "{closed:?}");
}

#[test]
fn a_diagnostic_of_several_lines_keeps_its_lines() {
    let sandbox = Sandbox::new();
    let refused = sandbox.run(&["recovery", "frobnicate"]);
    assert_eq!(refused.status.code(), Some(2), "{}", text(&refused.stderr));
    let stderr = text(&refused.stderr);
    assert!(stderr.lines().count() > 2, "{stderr}");
    assert!(!stderr.contains("\\u{a}"), "{stderr}");
}

/// P0-1: a global `--dry-run` whose office command ends in a flag left without its value, or
/// writes flags after `--`, is a usage error (exit 2) before any instance or office is
/// reached (which, with no host here, would be exit 1): the added `--dry-run` can never
/// become that value and turn the preview into a live write.
#[test]
fn a_dry_run_with_a_flag_missing_its_value_is_refused_before_any_office_is_asked() {
    let sandbox = Sandbox::new();
    sandbox.save_context(None, OFFICE);
    for (args, named) in [
        (
            &["--dry-run", "msg", "send", "--to", "ada", "--body"][..],
            Some("--body"),
        ),
        (&["--dry-run", "task", "create", "--title"], Some("--title")),
        (&["task", "create", "--title"], Some("--title")),
        (
            &[
                "--dry-run",
                "msg",
                "send",
                "--to",
                "ada",
                "--",
                "--body",
                "x",
            ],
            None,
        ),
    ] {
        for json in [false, true] {
            let mut all = if json { vec!["--json"] } else { vec![] };
            all.extend_from_slice(args);
            let refused = sandbox.run(&all);
            let stderr = text(&refused.stderr);
            assert_eq!(refused.status.code(), Some(2), "{all:?}: {stderr}");
            if let Some(flag) = named {
                let shown = format!("{stderr}{}", text(&refused.stdout));
                assert!(shown.contains(flag), "{all:?}: {shown}");
            }
        }
    }
    let base = fs::read_dir(sandbox.root.join("base")).unwrap().count();
    assert_eq!(base, 0, "a refused command reached the instance");
}

/// `sign-in -- <command>` reads the command before it signs in: a flag left without its value
/// (even after a `--dry-run` the caller wrote) or flags after `--` are a usage error (exit 2)
/// before any instance or office is reached. With no host here, reaching one would fail
/// instead (exit 1), as a whole line does.
#[test]
fn sign_in_refuses_a_malformed_command_before_reaching_the_host() {
    let sandbox = Sandbox::new();
    sandbox.save_context(None, OFFICE);
    for (command, named) in [
        (
            &["msg", "send", "--to", "ada", "--dry-run", "--body"][..],
            Some("--body"),
        ),
        (&["task", "create", "--title"], Some("--title")),
        (&["msg", "send", "--to", "ada", "--", "--body", "x"], None),
    ] {
        for json in [false, true] {
            let mut all = if json { vec!["--json"] } else { vec![] };
            all.extend(["sign-in", "--"]);
            all.extend_from_slice(command);
            let refused = sandbox.run(&all);
            let shown = format!("{}{}", text(&refused.stderr), text(&refused.stdout));
            assert_eq!(refused.status.code(), Some(2), "{all:?}: {shown}");
            if let Some(flag) = named {
                assert!(shown.contains(flag), "{all:?}: {shown}");
            }
        }
    }
    let base = fs::read_dir(sandbox.root.join("base")).unwrap().count();
    assert_eq!(base, 0, "a refused command reached the instance");
    let reached = sandbox.run(&["sign-in", "--", "task", "list"]);
    assert_eq!(
        reached.status.code(),
        Some(1),
        "a whole line goes on to the host: {}",
        text(&reached.stderr)
    );
}

/// P1: under `--json` standard error carries only an error envelope: a successful mutation,
/// a preview and a read write nothing there.
#[test]
fn a_successful_json_run_writes_nothing_on_standard_error() {
    let sandbox = Sandbox::new();
    sandbox.save_context(Some("lab"), OFFICE);
    for args in [
        &["--json", "--dry-run", "context", "clear"][..],
        &["context", "show", "--json"],
        &["context", "clear", "--yes", "--json"],
        &["context", "clear", "--yes", "--json"],
    ] {
        let ran = sandbox.run(args);
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{args:?}: {}",
            text(&ran.stderr)
        );
        assert!(ran.stderr.is_empty(), "{args:?}: {}", text(&ran.stderr));
        serde_json::from_slice::<Value>(&ran.stdout).unwrap();
    }
    assert!(!sandbox.context_file().exists());
}

/// A saved context that is a symbolic link (here dangling) is refused when read and
/// removed (the link itself) by `context clear`, never followed.
#[test]
fn a_linked_saved_context_is_refused_and_can_be_cleared() {
    let sandbox = Sandbox::new();
    let target = sandbox.root.join("elsewhere.json");
    std::os::unix::fs::symlink(&target, sandbox.context_file()).unwrap();
    let shown = sandbox.run(&["context", "show", "--json"]);
    assert_eq!(shown.status.code(), Some(1), "{}", text(&shown.stderr));
    let cleared = sandbox.run(&["context", "clear", "--yes", "--json"]);
    assert_eq!(cleared.status.code(), Some(0), "{}", text(&cleared.stderr));
    let answer: Value = serde_json::from_slice(&cleared.stdout).unwrap();
    assert_eq!(answer["cleared"], true);
    assert!(fs::symlink_metadata(sandbox.context_file()).is_err());
    assert!(!target.exists());
}

/// TASK-155: `release install` takes `--no-restart`, and its dry run lists what it would do
/// to each registered service (none in a fresh account) without making the store.
#[test]
fn release_install_dry_run_lists_the_registered_services_it_would_move() {
    let sandbox = Sandbox::new();
    let bundle = sandbox.root.join("bundle");
    fs::create_dir(&bundle).unwrap();
    let store = sandbox.root.join("store");
    let preview = sandbox.run(&[
        "--dry-run",
        "--json",
        "release",
        "install",
        bundle.to_str().unwrap(),
        "--store",
        store.to_str().unwrap(),
        "--no-restart",
    ]);
    assert_eq!(preview.status.code(), Some(0), "{}", text(&preview.stderr));
    assert!(preview.stderr.is_empty(), "{}", text(&preview.stderr));
    let plan: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(plan["dry_run"], true);
    assert_eq!(plan["action"], "release_install");
    assert_eq!(plan["kind"], "directory");
    assert_eq!(plan["services"], serde_json::json!([]));
    assert_eq!(plan["restart"], false, "--no-restart is in the plan");
    assert!(plan.get("services_warning").is_none(), "{plan}");
    assert!(!store.exists(), "a dry run makes no store");
}

/// TASK-155 review B3: when the registered services cannot be read, `release install
/// --json` says so in its output (`services_warning`) and never as prose on standard
/// error: nothing there on a success, only the error's envelope on a failure. A person
/// still sees the warning.
#[test]
fn release_install_json_reports_unread_services_in_its_output_only() {
    let sandbox = Sandbox::new();
    let bundle = sandbox.root.join("bundle");
    fs::create_dir(&bundle).unwrap();
    let store = sandbox.root.join("store");
    let install = |flags: &[&str]| {
        let mut args = flags.to_vec();
        args.extend([
            "release",
            "install",
            bundle.to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ]);
        // A development service adapter that cannot be composed (it has no home to be
        // placed in), so the services are not read.
        sandbox
            .command(&args)
            .env("ORG_DEV_SERVICE_PROGRAMS", sandbox.root.join("programs"))
            .env_remove("HOME")
            .output()
            .unwrap()
    };

    let preview = install(&["--dry-run", "--json"]);
    assert_eq!(preview.status.code(), Some(0), "{}", text(&preview.stderr));
    assert!(preview.stderr.is_empty(), "{}", text(&preview.stderr));
    let plan: Value = serde_json::from_slice(&preview.stdout).unwrap();
    let warning = plan["services_warning"].as_str().unwrap_or_default();
    assert!(warning.contains("were not checked"), "{plan}");
    assert!(warning.contains("HOME is not set"), "{plan}");
    assert_eq!(plan["services"], serde_json::json!([]));
    assert_eq!(plan["restart"], true);

    // An empty folder is no release: the install fails after the services were inspected.
    let failed = install(&["--json"]);
    assert_eq!(failed.status.code(), Some(1), "{}", text(&failed.stderr));
    assert!(failed.stdout.is_empty(), "{}", text(&failed.stdout));
    let envelope = error_envelope(&failed);
    assert!(envelope["error"]["code"].as_str().unwrap().contains("::"));

    let human = install(&["--dry-run"]);
    assert_eq!(human.status.code(), Some(0), "{}", text(&human.stderr));
    let said = text(&human.stderr);
    assert!(
        said.contains("! The instances' registered services were not checked"),
        "{said}"
    );
    assert!(said.contains("[DRY-RUN]"), "{said}");
}
