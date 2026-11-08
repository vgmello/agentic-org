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

/// The graceful stop sequence, run after the [`crate::Session`] has had its say
/// ([`crate::Control::Close`]).
///
/// The supervisor runs the steps in order. After each step it waits up to the step's grace
/// for the process to exit, and moves on if it hasn't. **After the last step it always kills
/// the whole process group** (`SIGKILL`) and runs orphan cleanup, so a plan never lists
/// that step, and an empty plan means "kill at once".
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
    /// `SIGINT`: the design's stop sequence sends it after closing stdin, and it's how
    /// Antigravity's stream mode stops (it kills the tool group).
    Interrupt,
    /// `SIGTERM`.
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

/// A path to remove once the process has exited, as a template. Removed while the child is still
/// an unreaped zombie (its pid can't be reused yet), then the child is reaped.
///
/// `{pid}` stands for the runtime process's pid, e.g. `/private/tmp/cc-socks/{pid}.sock`.
/// It's a **real path**: no component may be a symlink (on macOS `/tmp` is one, so the
/// template names `/private/tmp`), because the supervisor refuses to follow any.
///
/// What the supervisor does with it, for each one:
/// 1. While the child is still an **unreaped zombie** (so its pid can't have been reused), it
///    walks from `/` one directory at a time with `openat(..., O_DIRECTORY | O_NOFOLLOW)`; a
///    symlink anywhere on the way (planted in a shared `/tmp` folder, say) ends the cleanup
///    for that path.
/// 2. It checks the leaf with `fstatat(..., AT_SYMLINK_NOFOLLOW)`, and **unlinks it only if
///    it's not a directory and is owned by the current uid** (a socket, typically): never
///    `rmdir`, never a tree, never another user's file, never through a symlink. A symlink
///    at the leaf that the user owns is removed as a link; its target is never touched.
/// 3. Then it reaps the child.
///
/// Checked when made (`TryFrom<&str>`): with every `{pid}` taken as a number, it must be a
/// valid [`crate::SpecPath`] (absolute, no `.`, `..` or empty components, no trailing `/`, no
/// control, separator or format characters) other than `/`, and hold no other `{` or `}`. So
/// a pid only ever fills in part or all of one segment: it can't add segments or climb out of
/// a directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CleanupPath(String);

impl CleanupPath {
    /// The template, `{pid}` placeholders included.
    pub fn template(&self) -> &str {
        &self.0
    }

    /// The path for the process with this pid: every `{pid}` in the template replaced.
    pub fn path_for(&self, pid: u32) -> PathBuf {
        PathBuf::from(self.0.replace("{pid}", &pid.to_string()))
    }
}

impl TryFrom<&str> for CleanupPath {
    type Error = InvalidValue;

    fn try_from(template: &str) -> Result<Self, Self::Error> {
        const WHAT: &str = "cleanup path";
        let sample = template.replace("{pid}", "1");
        if sample.contains(['{', '}']) {
            return Err(InvalidValue::new(
                WHAT,
                "`{` and `}` may only be part of `{pid}`",
            ));
        }
        SpecPath::try_from(sample.as_str()).map_err(|e| InvalidValue::new(WHAT, e.message()))?;
        if sample == "/" {
            return Err(InvalidValue::new(WHAT, "it's the root"));
        }
        Ok(CleanupPath(template.to_owned()))
    }
}

/// How to run one probe: a short-lived process, the lines to feed its stdin, and how long it
/// may take. Its output goes to [`crate::AgentRuntime::parse_probe`].
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
    /// Lines to write to stdin, in order.
    pub stdin_lines: Vec<Line>,
    /// `false`: stdin is closed right after the last line. `true`: it stays open until the
    /// process exits or the timeout stops it, for an exchange the runtime would end on EOF
    /// (Codex's `initialize` / `account/read` over its app-server).
    pub keep_stdin_open_until_exit: bool,
    /// How long the probe may run before it's stopped and reported as timed out.
    pub timeout_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cleanup(template: &str) -> CleanupPath {
        CleanupPath::try_from(template).unwrap()
    }

    #[test]
    fn cleanup_path_resolves_every_pid_placeholder() {
        let c = cleanup("/private/tmp/cc-socks/{pid}.sock");
        assert_eq!(c.template(), "/private/tmp/cc-socks/{pid}.sock");
        assert_eq!(
            c.path_for(4242),
            PathBuf::from("/private/tmp/cc-socks/4242.sock")
        );
        let twice = cleanup("/private/tmp/{pid}/{pid}.lock");
        assert_eq!(twice.path_for(7), PathBuf::from("/private/tmp/7/7.lock"));
        let whole = cleanup("/private/tmp/{pid}");
        assert_eq!(whole.path_for(9), PathBuf::from("/private/tmp/9"));
        let none = cleanup("/private/tmp/fixed");
        assert_eq!(none.path_for(7), PathBuf::from("/private/tmp/fixed"));
        // Valid by type, but not by contract on macOS (`/tmp` is a symlink there): the type
        // can't see the file system, so naming real paths is the adapter's duty.
        assert!(CleanupPath::try_from("/tmp/{pid}.sock").is_ok());
    }

    #[test]
    fn cleanup_path_templates_are_checked() {
        for bad in [
            "",
            "/",
            "tmp/{pid}.sock",
            "/tmp/../etc/{pid}",
            "/tmp/./{pid}",
            "/tmp//{pid}",
            "/tmp/{pid}/",
            "/tmp/{uid}.sock",
            "/tmp/{pid",
            "/tmp/pid}",
            "/tmp/{{pid}}",
            "/tmp/{pid}\n.sock",
            "/tmp/\u{200B}{pid}",
        ] {
            assert!(CleanupPath::try_from(bad).is_err(), "{bad:?}");
        }
    }
}
