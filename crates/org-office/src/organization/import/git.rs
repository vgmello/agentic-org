//! Running `git` for an import: the private temporary directory, the source check, and
//! bounded, timed, credential-scrubbed `git` invocations.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::ImportError;

/// Maximum bytes read from `git` stderr.
pub(super) const MAX_GIT_STDERR_BYTES: u64 = 64 * 1024;

/// Maximum bytes read from `git ls-tree` / `rev-parse` stdout.
const MAX_GIT_META_STDOUT_BYTES: u64 = 4 * 1024 * 1024;

/// Wall-clock deadline for each `git` invocation.
pub(super) const GIT_TIMEOUT: Duration = Duration::from_secs(45);

/// RAII private temporary directory created atomically with mode `0700` and deleted on drop.
pub(crate) struct PrivateTempDir(PathBuf);

impl PrivateTempDir {
    pub(crate) fn create(prefix: &str) -> Result<Self, ImportError> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        let base = std::env::temp_dir();
        for attempt in 0u32..32 {
            let path = base.join(format!("{prefix}-{pid}-{nonce}-{attempt}"));
            match org_platform::fs::create_owner_only_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(ImportError::Storage(format!("creating temp dir: {e}"))),
            }
        }
        Err(ImportError::Storage(
            "could not allocate temporary directory".into(),
        ))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Validates a Git remote URL (`https://...`, `ssh://...`, or absolute `file:///...`) and branch
/// name before invoking `git`. Matches `PlanV1` strict remote/branch syntax and refuses unknown
/// schemes (`ext::`), credentials, queries/fragments, and leading `-` option injection.
pub fn check_git_source(remote_url: &str, state_branch: &str) -> Result<(), ImportError> {
    let valid_url = if remote_url.starts_with("https://") || remote_url.starts_with("ssh://") {
        org_proto::selected_bootstrap::valid_remote_url(remote_url)
    } else if let Some(path) = remote_url.strip_prefix("file://") {
        path.starts_with('/')
            && path.len() > 1
            && remote_url.len() <= 2048
            && !remote_url
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            && !remote_url.contains(['?', '#', '\\'])
    } else {
        false
    };
    if !valid_url || remote_url.starts_with('-') {
        return Err(ImportError::InvalidInput(
            "repository_remote_url must be a valid https://, ssh://, or file:/// Git URL".into(),
        ));
    }
    if !org_proto::selected_bootstrap::valid_state_branch(state_branch) {
        return Err(ImportError::InvalidInput(
            "state_branch must be a valid Git branch name".into(),
        ));
    }
    Ok(())
}

pub(super) fn base_git_command() -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        .args([
            "-c",
            "core.symlinks=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "submodule.recurse=false",
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "protocol.ssh.allow=always",
            "-c",
            "protocol.file.allow=always",
        ]);
    cmd
}

pub(super) fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, bool> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(true);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(false);
            }
        }
    }
}

pub(super) fn sanitize_git_stderr(stderr: &str, staging_dir: &Path, fallback: &str) -> String {
    let chosen = stderr
        .lines()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with("fatal:") || t.starts_with("error:")
        })
        .unwrap_or(fallback)
        .trim();
    let redacted = chosen.replace(&staging_dir.display().to_string(), "<staging>");
    let sanitized = strip_url_credentials(&redacted);
    if sanitized.is_empty() {
        fallback.to_owned()
    } else {
        sanitized.chars().take(240).collect()
    }
}

fn strip_url_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("://") {
        let after_scheme = &rest[pos + 3..];
        let seg_end = after_scheme
            .find(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == '/')
            .unwrap_or(after_scheme.len());
        let authority = &after_scheme[..seg_end];
        if let Some(at) = authority.rfind('@') {
            out.push_str(&rest[..pos + 3]);
            out.push_str(&authority[at + 1..]);
            rest = &after_scheme[seg_end..];
        } else {
            out.push_str(&rest[..pos + 3]);
            rest = after_scheme;
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn run_git_capture(
    args: &[&str],
    cwd: Option<&Path>,
    staging: &Path,
    fallback: &str,
) -> Result<Vec<u8>, ImportError> {
    let mut cmd = base_git_command();
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| ImportError::GitFailed(format!("failed to start git: {e}")))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout {
            let _ = s.take(MAX_GIT_META_STDOUT_BYTES).read_to_end(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr {
            let _ = s.take(MAX_GIT_STDERR_BYTES).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });
    let status = match wait_with_timeout(&mut child, GIT_TIMEOUT) {
        Ok(s) => s,
        Err(true) => return Err(ImportError::GitFailed("git operation timed out".into())),
        Err(false) => return Err(ImportError::GitFailed(fallback.into())),
    };
    let out = out_thread.join().unwrap_or_default();
    let err_text = err_thread.join().unwrap_or_default();
    if !status.success() {
        return Err(ImportError::GitFailed(sanitize_git_stderr(
            &err_text, staging, fallback,
        )));
    }
    Ok(out)
}
