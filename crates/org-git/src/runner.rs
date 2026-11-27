//! Hardened `git` process runner (`doc-35` §Hardening every office Git command).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::bounded::{capture, BoundedOutput, CaptureError, OutputLimits};
use crate::GitError;

/// Minimum supported Git version (supports `GIT_CONFIG_COUNT` and `safe.directory`).
pub const MIN_GIT_VERSION: (u32, u32, u32) = (2, 35, 2);

/// Single transport protocol permitted for one Git invocation (`protocol.allow=never` otherwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowedProtocol {
    None,
    Https,
    Ssh,
    File,
}

impl AllowedProtocol {
    fn config_flag(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Https => Some("protocol.https.allow=always"),
            Self::Ssh => Some("protocol.ssh.allow=always"),
            Self::File => Some("protocol.file.allow=always"),
        }
    }
}

/// Office-local Git credential configuration supplied only to one remote invocation.
#[derive(Clone, Default)]
pub enum GitCredential {
    /// Local repository or unauthenticated public remote.
    #[default]
    None,
    /// Office-owned SSH key and strict known-hosts file outside `durable/` and `runtime/`.
    Ssh {
        identity_file: PathBuf,
        known_hosts_file: PathBuf,
    },
    /// Office-controlled HTTPS credential helper executable (reads secret via office channel),
    /// strictly scoped to `scope_url` (`doc-35`).
    HttpsHelper {
        helper_program: PathBuf,
        scope_url: String,
    },
}

impl std::fmt::Debug for GitCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "GitCredential::None"),
            Self::Ssh { .. } => write!(f, "GitCredential::Ssh(<redacted>)"),
            Self::HttpsHelper { .. } => write!(f, "GitCredential::HttpsHelper(<redacted>)"),
        }
    }
}

/// Validated remote specification for fetch, ls-remote, push, and cleanup.
#[derive(Debug, Clone)]
pub struct RemoteSpec {
    pub url: String,
    pub protocol: AllowedProtocol,
    pub credential: GitCredential,
}

impl RemoteSpec {
    pub fn remote(url: &str, credential: GitCredential) -> Result<Self, GitError> {
        let protocol = validate_remote_url(url)?;
        Ok(Self {
            url: url.trim().to_owned(),
            protocol,
            credential,
        })
    }

    pub fn local(origin_path: &Path) -> Result<Self, GitError> {
        let canonical = validate_local_origin_path(origin_path)?;
        let url = canonical
            .to_str()
            .ok_or_else(|| GitError::InvalidInput("origin_path must be valid UTF-8".into()))?
            .to_owned();
        Ok(Self {
            url,
            protocol: AllowedProtocol::File,
            credential: GitCredential::None,
        })
    }
}

/// Captured output of a hardened Git invocation.
#[derive(Debug, Clone)]
pub struct GitCommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Single hardened Git runner shared by all office Git operations.
#[derive(Debug, Clone)]
pub struct GitRunner {
    git_bin: PathBuf,
    clean_home: PathBuf,
    fixed_path: String,
}

impl GitRunner {
    /// Resolves host `git`, verifies version >= [`MIN_GIT_VERSION`], and prepares an office-owned
    /// empty `HOME` directory so no user `~/.gitconfig` or `~/.ssh` is ever read.
    pub fn new(clean_home: impl Into<PathBuf>) -> Result<Self, GitError> {
        let clean_home = clean_home.into();
        // The office's empty `HOME` for Git: never shared, owner-only whatever the mask.
        crate::create_git_dirs(&clean_home, false)?;
        let fixed_path = fixed_system_path();
        let git_bin = resolve_git_binary(&fixed_path)?;
        verify_git_version(&git_bin, &clean_home, &fixed_path)?;
        Ok(Self {
            git_bin,
            clean_home,
            fixed_path,
        })
    }

    pub fn git_bin(&self) -> &Path {
        &self.git_bin
    }

    /// Runs `git` with a clean environment, mandatory hardening flags, explicit `GIT_DIR` /
    /// optional `GIT_WORK_TREE`, and single allowed protocol.
    pub fn run(
        &self,
        git_dir: Option<&Path>,
        work_tree: Option<&Path>,
        protocol: AllowedProtocol,
        credential: &GitCredential,
        args: &[&str],
    ) -> Result<GitCommandOutput, GitError> {
        let mut cmd = self.command(git_dir, work_tree, protocol, credential, args)?;
        cmd.stdin(Stdio::null());
        let output = cmd.output().map_err(|e| GitError::io(&self.git_bin, e))?;
        let status = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Ok(GitCommandOutput {
            status,
            stdout,
            stderr,
        })
    }

    /// Runs a local-only `git` command (no transport, no credential) under `limits`: keeps at
    /// most `limits.max_stdout` raw stdout bytes (killing Git once it writes more), at most
    /// [`crate::MAX_BOUNDED_STDERR_BYTES`] of stderr, and kills Git when it outlives
    /// `limits.timeout` ([`GitError::CommandFailed`] with status `-1`).
    pub fn run_local_bounded(
        &self,
        git_dir: Option<&Path>,
        work_tree: Option<&Path>,
        args: &[&str],
        limits: OutputLimits,
    ) -> Result<BoundedOutput, GitError> {
        let mut cmd = self.command(
            git_dir,
            work_tree,
            AllowedProtocol::None,
            &GitCredential::None,
            args,
        )?;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cmd.spawn().map_err(|e| GitError::io(&self.git_bin, e))?;
        capture(child, limits).map_err(|e| match e {
            CaptureError::Io(source) => GitError::io(&self.git_bin, source),
            CaptureError::TimedOut => GitError::CommandFailed {
                status: -1,
                stderr: format!("git timed out after {} s", limits.timeout.as_secs()),
            },
        })
    }

    /// Builds the hardened `git` command: clean environment, mandatory non-disableable config,
    /// explicit `GIT_DIR` / optional `GIT_WORK_TREE`, one allowed protocol and its credential.
    fn command(
        &self,
        git_dir: Option<&Path>,
        work_tree: Option<&Path>,
        protocol: AllowedProtocol,
        credential: &GitCredential,
        args: &[&str],
    ) -> Result<Command, GitError> {
        let mut cmd = Command::new(&self.git_bin);
        cmd.env_clear();
        cmd.env("PATH", &self.fixed_path);
        cmd.env("HOME", &self.clean_home);
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
        cmd.env("GIT_ATTR_NOSYSTEM", "1");
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        cmd.env("GIT_NO_REPLACE_OBJECTS", "1");
        cmd.env("LC_ALL", "C");
        if let Some(dir) = git_dir {
            cmd.env("GIT_DIR", dir);
            cmd.arg("-c")
                .arg(format!("safe.directory={}", dir.display()));
        }
        if let Some(wt) = work_tree {
            cmd.env("GIT_WORK_TREE", wt);
            cmd.arg("-c")
                .arg(format!("safe.directory={}", wt.display()));
            cmd.current_dir(wt);
        } else {
            cmd.current_dir(&self.clean_home);
        }

        // Mandatory non-disableable hardening config (`doc-35`).
        for kv in [
            "core.hooksPath=/dev/null",
            "core.fsmonitor=false",
            "credential.helper=",
            "gc.auto=0",
            "maintenance.auto=false",
            "submodule.recurse=false",
            "protocol.allow=never",
            "http.sslVerify=true",
        ] {
            cmd.arg("-c").arg(kv);
        }
        if let Some(allow) = protocol.config_flag() {
            cmd.arg("-c").arg(allow);
        }
        apply_credential(&mut cmd, protocol, credential)?;
        cmd.args(args);
        Ok(cmd)
    }

    /// Runs `git` and maps non-zero exit codes to [`GitError::CommandFailed`].
    pub fn run_checked(
        &self,
        git_dir: Option<&Path>,
        work_tree: Option<&Path>,
        protocol: AllowedProtocol,
        credential: &GitCredential,
        args: &[&str],
    ) -> Result<String, GitError> {
        let out = self.run(git_dir, work_tree, protocol, credential, args)?;
        if out.status == 0 {
            Ok(out.stdout)
        } else {
            Err(GitError::CommandFailed {
                status: out.status,
                stderr: out.stderr.trim().to_owned(),
            })
        }
    }
}

fn apply_credential(
    cmd: &mut Command,
    protocol: AllowedProtocol,
    credential: &GitCredential,
) -> Result<(), GitError> {
    match (protocol, credential) {
        (_, GitCredential::None) => Ok(()),
        (
            AllowedProtocol::Ssh,
            GitCredential::Ssh {
                identity_file,
                known_hosts_file,
            },
        ) => {
            let ssh_cmd = format!(
                "ssh -F /dev/null -o BatchMode=yes -o StrictHostKeyChecking=yes \
                 -o IdentitiesOnly=yes -o UserKnownHostsFile={} -i {}",
                shell_quote(known_hosts_file)?,
                shell_quote(identity_file)?
            );
            cmd.env("GIT_SSH_COMMAND", ssh_cmd);
            Ok(())
        }
        (
            AllowedProtocol::Https,
            GitCredential::HttpsHelper {
                helper_program,
                scope_url,
            },
        ) => {
            if !helper_program.is_absolute() {
                return Err(GitError::InvalidInput(
                    "HTTPS helper path must be absolute".into(),
                ));
            }
            if validate_remote_url(scope_url)? != AllowedProtocol::Https {
                return Err(GitError::SecurityViolation(
                    "HTTPS credential helper scope_url must be a valid https:// URL".into(),
                ));
            }
            let quoted = shell_quote(helper_program)?;
            cmd.arg("-c")
                .arg(format!("credential.{scope_url}.helper=!{quoted}"));
            Ok(())
        }
        _ => Err(GitError::SecurityViolation(
            "credential kind does not match allowed Git transport protocol".into(),
        )),
    }
}

fn shell_quote(path: &Path) -> Result<String, GitError> {
    let s = path
        .to_str()
        .ok_or_else(|| GitError::InvalidInput("path is not valid UTF-8".into()))?;
    if s.contains('\0') || s.contains('\n') || s.contains('\r') {
        return Err(GitError::InvalidInput(
            "path contains control characters".into(),
        ));
    }
    Ok(format!("'{}'", s.replace('\'', "'\\''")))
}

fn valid_host_component(host: &str) -> bool {
    !host.is_empty() && !host.starts_with('-') && !host.contains(['/', '?', '#', '@', '\\'])
}

fn valid_user_component(user: &str) -> bool {
    !user.is_empty() && !user.starts_with('-') && !user.contains([':', '/', '?', '#', '@', '\\'])
}

fn validate_https_url(rest: &str) -> Result<AllowedProtocol, GitError> {
    let authority = rest.split('/').next().unwrap_or("");
    if !valid_host_component(authority) {
        return Err(GitError::InvalidInput(
            "https remote_url must have a valid host and must not embed credentials".into(),
        ));
    }
    Ok(AllowedProtocol::Https)
}

fn validate_ssh_scheme_url(rest: &str) -> Result<AllowedProtocol, GitError> {
    let Some((authority, repo_path)) = rest.split_once('/') else {
        return Err(GitError::InvalidInput(
            "ssh remote_url must include a host and repository path".into(),
        ));
    };
    if authority.is_empty() || repo_path.is_empty() || repo_path.starts_with('-') {
        return Err(GitError::InvalidInput(
            "ssh remote_url is missing host or valid repository path".into(),
        ));
    }
    let host_port = match authority.split_once('@') {
        Some((userinfo, host_part)) => {
            if !valid_user_component(userinfo) {
                return Err(GitError::InvalidInput(
                    "ssh remote_url has invalid userinfo or embedded password".into(),
                ));
            }
            host_part
        }
        None => authority,
    };
    let host = host_port.split_once(':').map_or(host_port, |(h, _)| h);
    if !valid_host_component(host) {
        return Err(GitError::InvalidInput(
            "ssh remote_url host is invalid or starts with `-`".into(),
        ));
    }
    Ok(AllowedProtocol::Ssh)
}

fn validate_scp_ssh_url(url: &str) -> Option<AllowedProtocol> {
    if url.contains("://") || url.contains("::") {
        return None;
    }
    let (user_host, repo_path) = url.split_once(':')?;
    if user_host.contains(['/', '?', '#', '\\'])
        || repo_path.is_empty()
        || repo_path.starts_with('-')
    {
        return None;
    }
    let (user, host) = user_host.split_once('@')?;
    (valid_user_component(user) && valid_host_component(host)).then_some(AllowedProtocol::Ssh)
}

/// Validates a registered remote repository URL (`doc-35`: `https` and `ssh` only, no embedded
/// password/token, no option-injection prefix, no `http`/`git`/`ext`/`fd`/`file`).
pub fn validate_remote_url(raw: &str) -> Result<AllowedProtocol, GitError> {
    let url = raw.trim();
    if url.is_empty()
        || url.starts_with('-')
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(GitError::InvalidInput(
            "remote_url must be a non-empty URL without whitespace, control chars, or leading `-`"
                .into(),
        ));
    }
    if let Some(rest) = url.strip_prefix("https://") {
        return validate_https_url(rest);
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        return validate_ssh_scheme_url(rest);
    }
    if let Some(proto) = validate_scp_ssh_url(url) {
        return Ok(proto);
    }
    Err(GitError::InvalidInput(
        "only `https://` and `ssh` remote repository URLs are allowed".into(),
    ))
}

/// Validates a local-only repository `origin_path` (must be an existing plain absolute directory).
pub fn validate_local_origin_path(path: &Path) -> Result<PathBuf, GitError> {
    if !path.is_absolute() {
        return Err(GitError::InvalidInput(
            "local repository origin_path must be absolute".into(),
        ));
    }
    let meta = fs::metadata(path).map_err(|e| GitError::io(path, e))?;
    if !meta.is_dir() {
        return Err(GitError::InvalidInput(
            "local repository origin_path must be a directory".into(),
        ));
    }
    fs::canonicalize(path).map_err(|e| GitError::io(path, e))
}

fn fixed_system_path() -> String {
    "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".to_owned()
}

fn resolve_git_binary(fixed_path: &str) -> Result<PathBuf, GitError> {
    for dir in fixed_path.split(':') {
        let candidate = Path::new(dir).join("git");
        if candidate.is_file() {
            return fs::canonicalize(&candidate).map_err(|e| GitError::io(candidate, e));
        }
    }
    Err(GitError::UnsupportedGit(
        "git binary not found in system PATH".into(),
    ))
}

fn verify_git_version(git_bin: &Path, clean_home: &Path, fixed_path: &str) -> Result<(), GitError> {
    let out = Command::new(git_bin)
        .env_clear()
        .env("PATH", fixed_path)
        .env("HOME", clean_home)
        .arg("--version")
        .output()
        .map_err(|e| GitError::io(git_bin, e))?;
    if !out.status.success() {
        return Err(GitError::UnsupportedGit("`git --version` failed".into()));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let parsed = parse_git_version(&text).ok_or_else(|| {
        GitError::UnsupportedGit(format!("unparseable git version: {}", text.trim()))
    })?;
    if parsed < MIN_GIT_VERSION {
        return Err(GitError::UnsupportedGit(format!(
            "git {}.{}.{} is older than minimum {}.{}.{}",
            parsed.0, parsed.1, parsed.2, MIN_GIT_VERSION.0, MIN_GIT_VERSION.1, MIN_GIT_VERSION.2
        )));
    }
    Ok(())
}

pub(crate) fn parse_git_version(raw: &str) -> Option<(u32, u32, u32)> {
    let token = raw.split_whitespace().nth(2)?;
    let mut parts = token.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch_str = parts.next().unwrap_or("0");
    let patch_digits: String = patch_str
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let patch = patch_digits.parse::<u32>().unwrap_or(0);
    Some((major, minor, patch))
}
