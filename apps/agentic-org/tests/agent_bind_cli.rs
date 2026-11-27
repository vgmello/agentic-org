//! Process-level CLI contract tests for `agentic-org agent bind` and
//! `agentic-org hr hire ... --bind-workspace <dir>` (`TASK-211`).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_agentic-org");

struct Sandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for sub in ["home", "base", "cache", "config", "state", "work"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        Self { _dir: dir, root }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("ORG_DEV_ACCOUNT_BASE", self.root.join("base"))
            .env("ORG_OFFICE_DEV_KEY_DIR", self.root.join("keys"))
            .env("AGENTIC_ORG_CACHE_DIR", self.root.join("cache"))
            .env("AGENTIC_ORG_CONFIG_DIR", self.root.join("config"))
            .env("AGENT_OS_STATE_DIR", self.root.join("state"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .current_dir(self.root.join("work"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn init_git_repo(dir: &std::path::Path) {
    fs::create_dir_all(dir).unwrap();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git init failed");
    fs::write(dir.join("README.md"), "hello\n").unwrap();
    let status = Command::new("git")
        .args(["add", "README.md"])
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git add failed");
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-q",
            "-m",
            "init",
        ])
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git commit failed");
}

#[test]
fn agent_bind_help_usage_errors_and_dry_run_keep_git_repo_clean() {
    let sandbox = Sandbox::new();
    let repo = sandbox.root.join("repo");
    init_git_repo(&repo);

    // Help via both `--help` and `help agent bind`.
    for args in [&["agent", "bind", "--help"][..], &["help", "agent", "bind"]] {
        let out = sandbox.run(args);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        let stdout = text(&out.stdout);
        assert!(stdout.contains("Use when:"), "{stdout}");
        assert!(stdout.contains("Examples:"), "{stdout}");
        assert!(stdout.contains("--member"), "{stdout}");
        assert!(stdout.contains("--workspace"), "{stdout}");
        assert!(stdout.contains("--takeover"), "{stdout}");
    }

    // `agent --help` lists `bind` and `hr hire --help` documents `--bind-workspace`.
    let agent_help = sandbox.run(&["agent", "--help"]);
    assert_eq!(agent_help.status.code(), Some(0));
    assert!(text(&agent_help.stdout).contains("\n  bind "));
    let hire_help = sandbox.run(&["hr", "hire", "--help"]);
    assert_eq!(hire_help.status.code(), Some(0));
    assert!(text(&hire_help.stdout).contains("--bind-workspace"));

    // Missing required flags -> exit 2 (with JSON error envelope on stderr when `--json` is passed
    // either before or after `agent bind`).
    for args in [
        &["--json", "agent", "bind", "--member", "@ada"][..],
        &["agent", "bind", "--member", "@ada", "--json"],
    ] {
        let out = sandbox.run(args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            text(&out.stderr)
        );
        assert!(out.stdout.is_empty());
        let err: Value = serde_json::from_str(text(&out.stderr).trim()).unwrap();
        assert_eq!(err["error"]["code"], "cli::usage");
    }

    // Dry-run works with `--json` either before or after `agent bind`, touches neither Git nor state.
    let repo_str = repo.to_str().unwrap();
    for args in [
        vec![
            "agent",
            "bind",
            "--member",
            "@ada",
            "--workspace",
            repo_str,
            "--takeover",
            "--dry-run",
            "--json",
        ],
        vec![
            "--json",
            "--dry-run",
            "agent",
            "bind",
            "--member",
            "@ada",
            "--workspace",
            repo_str,
            "--takeover",
        ],
    ] {
        let out = sandbox.run(&args);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            text(&out.stderr)
        );
        let preview: Value = serde_json::from_str(text(&out.stdout).trim()).unwrap();
        assert_eq!(preview["action"], "agent_bind");
        assert_eq!(preview["member"], "@ada");
        assert_eq!(preview["takeover"], true);
        assert_eq!(preview["workspace_root"], repo_str);
    }
    assert_eq!(fs::read_dir(sandbox.root.join("state")).unwrap().count(), 0);

    // Empty or control-char `--member` fails exit 2 even with `--dry-run`.
    for bad_member in ["", "   ", "bad\nid"] {
        let out = sandbox.run(&[
            "agent",
            "bind",
            "--member",
            bad_member,
            "--workspace",
            repo_str,
            "--dry-run",
            "--json",
        ]);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{bad_member:?}: {}",
            text(&out.stderr)
        );
        let err: Value = serde_json::from_str(text(&out.stderr).trim()).unwrap();
        assert_eq!(err["error"]["code"], "cli::usage");
    }

    // `agentic-org sign-in -- agent bind ...` routes cleanly, honors outer `--office`, and
    // rejects conflicting selectors or invalid `--member` with exit 2.
    let out = sandbox.run(&[
        "sign-in",
        "--office",
        "off-1",
        "--",
        "agent",
        "bind",
        "--member",
        "@ada",
        "--workspace",
        repo_str,
        "--dry-run",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let preview: Value = serde_json::from_str(text(&out.stdout).trim()).unwrap();
    assert_eq!(preview["action"], "agent_bind");
    assert_eq!(preview["office_id"], "off-1");
    assert_eq!(preview["workspace_root"], repo_str);

    for bad_sign_in in [
        vec![
            "sign-in",
            "--office",
            "off-1",
            "--",
            "agent",
            "bind",
            "--office",
            "off-2",
            "--member",
            "@ada",
            "--workspace",
            repo_str,
            "--dry-run",
        ],
        vec![
            "sign-in",
            "--",
            "agent",
            "bind",
            "--member",
            "",
            "--workspace",
            repo_str,
            "--dry-run",
        ],
    ] {
        let out = sandbox.run(&bad_sign_in);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{bad_sign_in:?}: {}",
            text(&out.stderr)
        );
    }

    // Missing directory fails closed before reaching any host.
    let missing = sandbox.root.join("does-not-exist");
    let out = sandbox.run(&[
        "agent",
        "bind",
        "--member",
        "@ada",
        "--workspace",
        missing.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let err: Value = serde_json::from_str(text(&out.stderr).trim()).unwrap();
    assert_eq!(err["error"]["code"], "office::not_found");

    // Real Git working tree remains 100% clean.
    let git_status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(git_status.status.success());
    assert!(
        git_status.stdout.is_empty(),
        "dirty git tree: {}",
        text(&git_status.stdout)
    );
}

#[test]
fn hr_hire_bind_workspace_rejects_invalid_combinations_before_contacting_host() {
    let sandbox = Sandbox::new();
    let repo = sandbox.root.join("repo");
    fs::create_dir(&repo).unwrap();
    let repo_str = repo.to_str().unwrap();

    for args in [
        vec![
            "hr",
            "hire",
            "--team",
            "T1",
            "--name",
            "Ada",
            "--handle",
            "ada",
            "--reason",
            "r",
            "--bind-workspace",
        ],
        vec![
            "hr",
            "hire",
            "--draft",
            "--team",
            "T1",
            "--name",
            "Ada",
            "--handle",
            "ada",
            "--reason",
            "r",
            "--bind-workspace",
            repo_str,
        ],
        vec![
            "hr",
            "hire",
            "--id",
            "0193a5c0-0000-7000-8000-000000000050",
            "--if-match",
            "1",
            "--withdraw",
            "--closed-reason",
            "no",
            "--bind-workspace",
            repo_str,
        ],
        vec![
            "--dry-run",
            "hr",
            "hire",
            "--team",
            "T1",
            "--name",
            "Ada",
            "--handle",
            "ada",
            "--reason",
            "r",
            "--bind-workspace",
            repo_str,
        ],
        vec![
            "hr",
            "hire",
            "--team",
            "T1",
            "--name",
            "Ada",
            "--handle",
            "ada",
            "--reason",
            "r",
            "--office-id",
            "sat-1",
            "--bind-workspace",
            repo_str,
        ],
        vec![
            "hr",
            "hire",
            "--team",
            "T1",
            "--name",
            "Ada",
            "--handle",
            "ada",
            "--reason",
            "r",
            "--format",
            "bogus",
            "--bind-workspace",
            repo_str,
        ],
    ] {
        let out = sandbox.run(&args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}
