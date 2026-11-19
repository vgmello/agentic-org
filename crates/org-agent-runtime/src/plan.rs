//! What an adapter wants spawned: [`LaunchPlan`] and [`ProbePlan`]. `org-supervise` carries
//! them out; the adapter never spawns.
//!
//! The enums here are instructions to the supervisor, so they're **exhaustive** on purpose
//! ([`FileLocation`], [`Stdio`], [`StopStep`], [`StopSignal`], [`SignalTarget`]): a new
//! variant is a breaking change that fails the supervisor's build until it's handled, never a
//! wildcard arm that guesses.

use std::path::PathBuf;

use crate::{EnvName, InvalidValue, Line, RelPath, SessionId, SpecPath};

/// How to start one runtime process, and how to stop and clean up after it.
///
/// The environment the child gets is the office's allowlist (built by the supervisor) plus
/// [`env_add`](Self::env_add); the adapter never sees or sets the office's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// The binary to run: [`crate::LaunchContext::runtime_path`], never a bare name.
    pub program: PathBuf,
    /// The arguments, without the program. Never a secret: argv shows in `ps`.
    pub args: Vec<String>,
    /// The working directory: the spec's workspace.
    pub cwd: SpecPath,
    /// The adapter's own switches, added to the office's environment (e.g.
    /// `DISABLE_AUTOUPDATER=1`). They never override a variable the office sets.
    pub env_add: Vec<(EnvName, String)>,
    /// Directories to create before the spawn, in order, before any file.
    pub dirs: Vec<PlannedDir>,
    /// Files to write before the spawn, in order, after the directories. Rewritten on every
    /// start, so an agent's edits never survive.
    pub files: Vec<PlannedFile>,
    /// Pipes (headless) or a terminal (take-over).
    pub stdio: Stdio,
    /// The runtime's session id, when it's known before the process starts (Claude
    /// `--session-id` / `--resume`). Session ids are runtime text: an adapter puts one in argv
    /// only in a form that can't read as a flag (`--resume=<id>`, or after a value-taking
    /// flag that never treats its value as an option). Otherwise it arrives as
    /// [`crate::SessionEvent::SessionStarted`].
    pub runtime_session_id: Option<SessionId>,
    /// How to stop the process gracefully.
    pub stop: StopPlan,
    /// Paths to remove once the process has exited (things the runtime leaves behind when it's
    /// killed). The supervisor removes them **while the child is an unreaped zombie**, so its
    /// pid can't have been reused, and only then reaps it.
    pub cleanup_after_exit: Vec<CleanupPath>,
    /// Names of secret variables the **runtime itself** injects into its tools' environment
    /// (e.g. `CLAUDE_CODE_MESSAGING_TOKEN`). The office doesn't know their values, so it
    /// redacts `NAME=value` wherever one of these names appears.
    pub secret_env_names: Vec<EnvName>,
}

/// A file the supervisor writes before the spawn, mode `0600`.
///
/// ## How the supervisor writes it (both locations)
/// The workspace is the agent's own, so it may hold symlinks the agent planted; the runtime
/// dir gets the same treatment so there's one rule. The supervisor:
/// - takes a [`rel_path`](Self::rel_path) that is a [`RelPath`] by type (relative, with no
///   `.`, `..` or empty component), so a plan can only write inside its
///   [`location`](Self::location);
/// - opens the location itself with `O_DIRECTORY | O_NOFOLLOW` (the office made it, by its
///   real path), then walks `rel_path` one directory at a time with `openat(...,
///   O_DIRECTORY | O_NOFOLLOW)`, **never following a symlink**: a symlink anywhere on the way
///   fails the launch;
/// - writes the contents to a new temporary file in the final directory (`O_CREAT | O_EXCL |
///   O_NOFOLLOW`, mode `0600`) and **renames it over** the target (`renameat`), so the file is
///   replaced atomically and a symlink or hard link at the target is replaced, **never written
///   through**.
///
/// Missing directories on the way must be listed in [`LaunchPlan::dirs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Which folder `rel_path` is relative to.
    pub location: FileLocation,
    /// The file's path inside `location`.
    pub rel_path: RelPath,
    /// The file's full contents. Never a secret.
    pub contents: Vec<u8>,
    /// Whether to add the file to the repository's shared exclude file
    /// (`work.git/info/exclude`), for a file written into the workspace.
    pub git_exclude: bool,
}

/// A directory the supervisor creates before the spawn, mode `0700`.
///
/// Same rules as [`PlannedFile`]: a [`RelPath`] (relative, no `.`, `..` or empty component),
/// walked with `openat(..., O_DIRECTORY | O_NOFOLLOW)`, never following a symlink. Each
/// missing component is made with `mkdirat` (`0700`); an existing one must be a real
/// directory, not a symlink, and is kept as it is (its mode included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedDir {
    /// Which folder `rel_path` is relative to.
    pub location: FileLocation,
    /// The directory's path inside `location`.
    pub rel_path: RelPath,
}

/// Where a [`PlannedFile`] or [`PlannedDir`] goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileLocation {
    /// [`crate::LaunchContext::runtime_dir`]: agent-readable, never agent-writable. The
    /// default place for every generated file.
    RuntimeDir,
    /// The session's workspace, only for a runtime that can read a file from nowhere else.
    Workspace,
}

/// How the child's standard streams are connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stdio {
    /// Three pipes, for a headless session or a probe, driven by a [`crate::Session`].
    Piped,
    /// A pseudo-terminal of this size, for take-over. Its bytes go to a person, not through
    /// a [`crate::Session`].
    Pty {
        /// Columns.
        cols: u16,
        /// Rows.
        rows: u16,
    },
}

/// The graceful stop sequence, run after the [`crate::Session`] has had its say.
///
/// **When it starts:** only after the session's [`crate::Output::CloseStdin`] (its answer to
/// [`crate::Control::Close`]), or after the supervisor's own timeout for that answer. Never
/// while the session may still be writing its goodbye.
///
/// **How the supervisor runs it:**
/// 1. **Graceful steps keep their time.** It runs the plan's steps in order, and after each
///    one waits up to the step's grace for the process to exit. A [`StopStep::CloseStdin`]
///    (listed before any signal) gets its full grace: that's Claude's exit on end of input,
///    which waits for background work, and Antigravity's close. It closes stdin only if the
///    session's own [`crate::Output::CloseStdin`] hasn't (the plan can start on a timeout).
/// 2. **The first signal is always `SIGTERM` to the leader.** When it reaches the plan's first
///    [`StopStep::Signal`], whatever that step says, it first sends `SIGTERM` to the runtime's
///    own process (the leader) and waits up to that step's grace. Runtimes clean up their
///    tool trees on it (Claude kills its tools, which have left the process group). It then
///    runs the plan's signal steps in order, that first one included (unless it's itself
///    `SIGTERM` to the leader, which isn't sent twice), so a runtime that also wants `SIGINT`
///    (Antigravity's stream mode) still gets it, after that `SIGTERM`.
/// 3. After the last step it kills the whole process group (`SIGKILL`) and runs orphan
///    cleanup. A plan never lists that step.
///
/// A plan with no signal step still gets the first `SIGTERM` to the leader, with a default
/// grace of 5 seconds, after its graceful steps: an empty plan means `SIGTERM` to the leader,
/// 5 seconds, then `SIGKILL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopPlan {
    /// The steps, in order.
    pub steps: Vec<StopStep>,
}

/// One step of a [`StopPlan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StopStep {
    /// Close the child's stdin (if it's still open), then wait.
    CloseStdin {
        /// How long to wait for the exit, in milliseconds.
        grace_ms: u64,
    },
    /// Send a signal, then wait.
    Signal {
        /// Which signal.
        signal: StopSignal,
        /// Who gets it.
        target: SignalTarget,
        /// How long to wait for the exit, in milliseconds.
        grace_ms: u64,
    },
}

/// A signal a [`StopStep`] may send. `SIGKILL` isn't one: it's always the supervisor's last
/// resort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StopSignal {
    /// `SIGINT`: how Antigravity's stream mode stops (it kills the tool group). Like every
    /// signal step, it's sent after the supervisor's first signal, `SIGTERM` to the leader.
    Interrupt,
    /// `SIGTERM`. (A plan's first signal step of `SIGTERM` to the leader is the supervisor's
    /// own first signal, not sent twice.)
    Terminate,
}

/// Who a [`StopStep::Signal`] goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalTarget {
    /// The runtime's own process only. Claude, for one, kills its tool trees itself on
    /// `SIGTERM`, and its tools leave the process group, so a group signal wouldn't reach them.
    Leader,
    /// The runtime's whole process group.
    Group,
}

/// A runtime's socket to remove once the process has exited, as a template inside an
/// allowed parent folder (e.g. Claude's `/private/tmp/cc-socks/{pid}.sock`, which a `SIGKILL`
/// leaves behind).
///
/// `{pid}` stands for the runtime process's pid, and the template **must** use it: the file
/// to remove is that process's own. It's a **real path**: no component may be a symlink (on
/// macOS `/tmp` is one, so the template names `/private/tmp`), because the supervisor
/// follows none. The resolved path must lie inside [`root`](Self::root).
///
/// What the supervisor does with it, for each one:
/// 1. While the child is still an **unreaped zombie** (so its pid can't have been reused), it
///    resolves the path and checks it's inside `root`, then walks from `/` one directory at a
///    time with `openat(..., O_DIRECTORY | O_NOFOLLOW)`. A symlink anywhere on the way
///    (planted in a shared `/tmp` folder, say) ends the cleanup for that path.
/// 2. It checks the leaf with `lstat` (`fstatat(..., AT_SYMLINK_NOFOLLOW)`), and **unlinks it
///    only if it's a socket (`S_IFSOCK`) owned by the current uid**: never a regular file, a
///    directory, a symlink, or another user's file.
/// 3. Then it reaps the child.
///
/// Checked when made ([`CleanupPath::new`]): `root` isn't `/` and holds no brace; the
/// template starts with
/// `root` and a `/`, uses `{pid}` after that, holds no other `{` or `}`, and with every `{pid}`
/// taken as a number is a valid [`crate::SpecPath`] (absolute, no `.`, `..` or empty
/// components, no trailing `/`, no control, separator or format characters). So a pid only
/// ever fills in part or all of one segment, and the path can't leave `root`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CleanupPath {
    root: SpecPath,
    template: String,
}

impl CleanupPath {
    /// Checks `template` against `root` and wraps them.
    pub fn new(root: SpecPath, template: &str) -> Result<Self, InvalidValue> {
        const WHAT: &str = "cleanup path";
        if root.is_root() {
            return Err(InvalidValue::new(WHAT, "its allowed root is `/`"));
        }
        if root.as_str().contains(['{', '}']) {
            return Err(InvalidValue::new(WHAT, "its allowed root holds `{` or `}`"));
        }
        let Some(inside) = template
            .strip_prefix(root.as_str())
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            return Err(InvalidValue::new(WHAT, "it isn't inside its allowed root"));
        };
        if !inside.contains("{pid}") {
            return Err(InvalidValue::new(
                WHAT,
                "it doesn't use `{pid}` (after its root)",
            ));
        }
        let sample = template.replace("{pid}", "1");
        if sample.contains(['{', '}']) {
            return Err(InvalidValue::new(
                WHAT,
                "`{` and `}` may only be part of `{pid}`",
            ));
        }
        SpecPath::try_from(sample.as_str()).map_err(|e| InvalidValue::new(WHAT, e.message()))?;
        Ok(CleanupPath {
            root,
            template: template.to_owned(),
        })
    }

    /// The folder the resolved path must be inside.
    pub fn root(&self) -> &SpecPath {
        &self.root
    }

    /// The template, `{pid}` placeholders included.
    pub fn template(&self) -> &str {
        &self.template
    }

    /// The path for the process with this pid: every `{pid}` in the template replaced.
    pub fn path_for(&self, pid: u32) -> PathBuf {
        PathBuf::from(self.template.replace("{pid}", &pid.to_string()))
    }
}

/// The process of one probe: a short-lived process, the lines to feed its stdin first, and how
/// long it may take. [`crate::ProbeRun`] says whether it's one-shot or a conversation.
///
/// Its type holds only what a probe may ask for. The supervisor runs it on pipes, with `/` as
/// its cwd (a probe touches no workspace), the office's environment plus
/// [`env_add`](Self::env_add), no files or directories written, and no session id. When it
/// exits or times out, the supervisor kills what's left of its process group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbePlan {
    /// The binary: [`crate::ProbeContext::runtime_path`], never a bare name.
    pub program: PathBuf,
    /// The arguments, without the program. Never a secret.
    pub args: Vec<String>,
    /// The adapter's own switches, added to the office's environment.
    pub env_add: Vec<(EnvName, String)>,
    /// Lines to write to stdin first, in order. A one-shot probe's stdin is closed after
    /// them; a conversation's stays open for its [`crate::ProbeSession`].
    pub stdin_lines: Vec<Line>,
    /// How long the probe may run before it's stopped and reported as timed out.
    pub timeout_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socks() -> SpecPath {
        SpecPath::try_from("/private/tmp/cc-socks").unwrap()
    }

    fn cleanup(template: &str) -> CleanupPath {
        CleanupPath::new(socks(), template).unwrap()
    }

    #[test]
    fn cleanup_path_resolves_every_pid_placeholder() {
        let c = cleanup("/private/tmp/cc-socks/{pid}.sock");
        assert_eq!(c.template(), "/private/tmp/cc-socks/{pid}.sock");
        assert_eq!(c.root(), &socks());
        assert_eq!(
            c.path_for(4242),
            PathBuf::from("/private/tmp/cc-socks/4242.sock")
        );
        let twice = cleanup("/private/tmp/cc-socks/{pid}/{pid}.sock");
        assert_eq!(
            twice.path_for(7),
            PathBuf::from("/private/tmp/cc-socks/7/7.sock")
        );
        let whole = cleanup("/private/tmp/cc-socks/{pid}");
        assert_eq!(whole.path_for(9), PathBuf::from("/private/tmp/cc-socks/9"));
        // Valid by type, but not by contract on macOS (`/tmp` is a symlink there): the type
        // can't see the file system, so naming real paths is the adapter's duty.
        let tmp = SpecPath::try_from("/tmp/cc-socks").unwrap();
        assert!(CleanupPath::new(tmp, "/tmp/cc-socks/{pid}.sock").is_ok());
    }

    #[test]
    fn cleanup_path_templates_are_checked() {
        for bad in [
            "",
            "/",
            "/private/tmp/cc-socks",
            "/private/tmp/cc-socks/",
            "/private/tmp/cc-socks/fixed.sock",
            "/private/tmp/{pid}.sock",
            "/private/tmp/cc-socks2/{pid}.sock",
            "/private/tmp/cc-socksX{pid}.sock",
            "private/tmp/cc-socks/{pid}.sock",
            "/private/tmp/cc-socks/../{pid}",
            "/private/tmp/cc-socks/./{pid}",
            "/private/tmp/cc-socks//{pid}",
            "/private/tmp/cc-socks/{pid}/",
            "/private/tmp/cc-socks/{uid}.sock",
            "/private/tmp/cc-socks/{pid}{",
            "/private/tmp/cc-socks/pid}{pid}",
            "/private/tmp/cc-socks/{{pid}}",
            "/private/tmp/cc-socks/{pid}\n.sock",
            "/private/tmp/cc-socks/\u{200B}{pid}",
        ] {
            assert!(CleanupPath::new(socks(), bad).is_err(), "{bad:?}");
        }
        // The allowed root can't be `/`, or hold braces.
        let root = SpecPath::try_from("/").unwrap();
        assert!(CleanupPath::new(root, "/{pid}.sock").is_err());
        let braces = SpecPath::try_from("/private/tmp/{pid}").unwrap();
        assert!(CleanupPath::new(braces, "/private/tmp/{pid}/{pid}.sock").is_err());
    }
}
