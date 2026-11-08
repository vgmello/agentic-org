//! Spawning ([`spawn`], [`spawn_session`]) and the running child ([`Child`]): its pipes or
//! terminal, the stop sequence, and reaping with the zombie-window cleanup and orphan cleanup.
//!
//! ## Groups and sessions
//! - A **piped** child (a headless session, a probe, an office server) leads a new process
//!   group in this process's session (`POSIX_SPAWN_SETPGROUP`, pgid = pid). It has no
//!   controlling terminal to share, and one `killpg` reaches everything it didn't move out.
//! - A **terminal** child (take-over) leads a new session (`POSIX_SPAWN_SETSID`, sid = pgid =
//!   pid), because only a session leader can acquire a controlling terminal. Its group is its
//!   own too, so the stop sequence is the same.
//!
//! ## The lifecycle, and who does what
//! 1. [`spawn_session`] writes the plan's dirs and files, then spawns. Record [`Child::id`]
//!    (pid and start time) and the mark before the first byte is sent.
//! 2. The caller (the pump) owns the pipes it takes out ([`Child::take_stdout`], ...).
//! 3. To end it: [`Child::stop`] (after the session's own `CloseStdin` and its wait, the
//!    pump's business): `SIGTERM` to the runtime's pid, then the plan's steps, then `SIGKILL`
//!    to the group. It **doesn't reap**, so the pump can drain stdout to its end first.
//! 4. [`Child::reap`]: waits for the exit, `SIGKILL`s the group (whatever is left of it),
//!    removes the plan's [`CleanupPath`]s while the child is still a zombie (so its pid can't
//!    have been reused), runs orphan cleanup for the session's mark, and only then reaps,
//!    returning the [`ExitInfo`].
//!
//! Dropping a `Child` that wasn't reaped kills its group and reaps it (with the cleanup), so
//! no zombie or stray group outlives it.
//!
//! **The pump's drain must be time-limited.** End-of-file on the child's stdout isn't
//! guaranteed once it has exited: a tool that detached (and so survived the group kill) may
//! still hold the stream, and so, for a moment, may a child that another spawner in this
//! process started while a pipe end wasn't close-on-exec yet (macOS has no `pipe2`). So
//! after the exit, drain for a bounded time (the probe runner allows 250 ms), then call
//! [`Child::reap`], whose orphan cleanup kills such holders.
//!
//! The host process must not set `SIGCHLD` to `SIG_IGN` (children would be reaped by the
//! kernel, and the zombie window would be lost), and must not reap these children itself.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use org_runtime::{
    CleanupPath, ExitInfo, LaunchPlan, SignalTarget, Stdio, StopPlan, StopSignal, StopStep,
};

use crate::cleanup::{self, CleanupReport};
use crate::env::Env;
use crate::error::{Error, Result};
use crate::files::{self, PlanTargets};
use crate::orphans::{self, OrphanQuery, OrphanReport, ProcessId};
use crate::pty::Pty;
use crate::sys::{self, ChildStdio, PidFilter, SpawnRequest};

/// How long orphans get between `SIGTERM` and `SIGKILL` when [`Child::reap`] cleans up after
/// a session (the design's 5 s).
pub const ORPHAN_GRACE: Duration = Duration::from_secs(5);

/// How often a wait checks whether the child has exited.
const POLL_MAX: Duration = Duration::from_millis(20);

/// A process to spawn that isn't an agent's session: an office server, say. Its environment
/// is exactly [`Env`]; nothing is inherited from this process.
#[derive(Debug, Clone)]
pub struct Command {
    /// The binary, by its absolute real path (no symlink in it). Never looked up on `PATH`.
    pub program: PathBuf,
    /// The arguments, without the program (it's `argv[0]`).
    pub args: Vec<String>,
    /// The working directory, absolute.
    pub cwd: PathBuf,
    /// The whole environment.
    pub env: Env,
    /// Pipes or a terminal.
    pub stdio: Stdio,
}

/// Spawns `cmd`. The child holds only fds 0, 1 and 2, leads its own process group (or, on a
/// terminal, its own session), and starts with default signal dispositions and an empty
/// signal mask.
pub fn spawn(cmd: &Command) -> Result<Child> {
    spawn_inner(Spawn {
        program: &cmd.program,
        args: &cmd.args,
        cwd: &cmd.cwd,
        env: &cmd.env,
        stdio: cmd.stdio,
        cleanup: Vec::new(),
        cleanup_root: None,
    })
}

/// Carries out a [`LaunchPlan`]: writes its directories and files (never through a symlink;
/// see [`PlanTargets`]), then spawns its program in its workspace with `env` plus the plan's
/// `env_add` (which never overrides the office's variables).
///
/// `env` must be an agent session's ([`crate::EnvBuilder::agent_session`]) with a mark, so
/// orphan cleanup can always find the session's processes; and a plan with cleanup paths
/// needs [`PlanTargets::cleanup_root`]. Otherwise nothing is written or spawned.
pub fn spawn_session(plan: &LaunchPlan, targets: &PlanTargets, env: &Env) -> Result<Child> {
    if !env.is_agent_session() || env.mark().is_none() {
        return Err(Error::invalid(
            "a session's environment needs the agent session values and a mark",
        ));
    }
    let cleanup_root = match (&targets.cleanup_root, plan.cleanup_after_exit.is_empty()) {
        (Some(root), _) => Some(root.to_path_buf()),
        (None, true) => None,
        (None, false) => {
            return Err(Error::invalid(
                "the plan has cleanup paths, but no cleanup root was given",
            ))
        }
    };
    check_program(&plan.program)?;
    let env = env.with_additions(&plan.env_add)?;
    files::write_plan(plan, targets)?;
    spawn_inner(Spawn {
        program: &plan.program,
        args: &plan.args,
        cwd: &plan.cwd.to_path_buf(),
        env: &env,
        stdio: plan.stdio,
        cleanup: plan.cleanup_after_exit.clone(),
        cleanup_root,
    })
}

pub(crate) struct Spawn<'a> {
    pub program: &'a Path,
    pub args: &'a [String],
    pub cwd: &'a Path,
    pub env: &'a Env,
    pub stdio: Stdio,
    pub cleanup: Vec<CleanupPath>,
    pub cleanup_root: Option<PathBuf>,
}

/// The program must be absolute and its own real path, so what runs is what the office
/// recorded at preflight, not whatever a symlink points to now.
fn check_program(program: &Path) -> Result<()> {
    if !program.is_absolute() {
        return Err(Error::invalid("the program isn't an absolute path"));
    }
    let real = std::fs::canonicalize(program)
        .map_err(|e| Error::io_at("resolve the program", program, e))?;
    if real != program {
        return Err(Error::Refused {
            path: program.to_path_buf(),
            why: "the program isn't named by its real path",
        });
    }
    Ok(())
}

fn c_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Error::invalid(format!("{} holds a NUL byte", path.display())))
}

pub(crate) fn spawn_inner(s: Spawn<'_>) -> Result<Child> {
    check_program(s.program)?;
    if !s.cwd.is_absolute() {
        return Err(Error::invalid("the working directory isn't absolute"));
    }
    let program = c_path(s.program)?;
    let cwd = c_path(s.cwd)?;
    let mut argv = vec![program.clone()];
    for arg in s.args {
        argv.push(
            CString::new(arg.as_bytes())
                .map_err(|_| Error::invalid("an argument holds a NUL byte"))?,
        );
    }
    let terminal = matches!(s.stdio, Stdio::Pty { .. });
    let envp = s.env.to_envp(terminal)?;

    let mut child = Child {
        pid: 0,
        id: None,
        mark: s.env.mark().map(str::to_owned),
        stdin: None,
        stdout: None,
        stderr: None,
        pty: None,
        cleanup: s.cleanup,
        cleanup_root: s.cleanup_root,
        tool_sessions: BTreeSet::new(),
        exited: false,
        reaped: true,
        orphan_grace: ORPHAN_GRACE,
    };
    let pid = match s.stdio {
        Stdio::Piped => {
            let pipe = || sys::pipe().map_err(|e| Error::io("create a pipe", e));
            let (in_r, in_w) = pipe()?;
            let (out_r, out_w) = pipe()?;
            let (err_r, err_w) = pipe()?;
            sys::set_nosigpipe(in_w.as_fd()).map_err(|e| Error::io("configure a pipe", e))?;
            let pid = sys::spawn(&SpawnRequest {
                program: &program,
                argv: &argv,
                envp: &envp,
                cwd: &cwd,
                stdio: ChildStdio::Fds {
                    stdin: in_r.as_fd(),
                    stdout: out_w.as_fd(),
                    stderr: err_w.as_fd(),
                },
            })
            .map_err(|e| Error::io_at("spawn", s.program, e))?;
            child.stdin = Some(File::from(in_w));
            child.stdout = Some(File::from(out_r));
            child.stderr = Some(File::from(err_r));
            pid
        }
        Stdio::Pty { cols, rows } => {
            // `_slave_fd` stays open until the spawn returns, then closes: this process must
            // not hold the slave, or reading the master would never see the child's end.
            let (master, slave, _slave_fd): (OwnedFd, CString, OwnedFd) =
                sys::open_pty(cols, rows).map_err(|e| Error::io("open a terminal", e))?;
            sys::set_nosigpipe(master.as_fd()).map_err(|e| Error::io("configure a terminal", e))?;
            let pid = sys::spawn(&SpawnRequest {
                program: &program,
                argv: &argv,
                envp: &envp,
                cwd: &cwd,
                stdio: ChildStdio::Terminal { slave: &slave },
            })
            .map_err(|e| Error::io_at("spawn", s.program, e))?;
            child.pty = Some(Pty::new(File::from(master)));
            pid
        }
    };
    child.pid = pid;
    child.reaped = false;
    child.id = sys::proc_info(pid).ok().flatten().map(|i| ProcessId {
        pid: u32::try_from(i.pid).unwrap_or(0),
        start_us: i.start_us,
    });
    Ok(child)
}

/// How a reaped child ended, and what was cleaned up after it.
#[derive(Debug)]
pub struct Exit {
    /// The exit code or the signal.
    pub info: ExitInfo,
    /// Each cleanup path's outcome.
    pub cleanup: Vec<CleanupReport>,
    /// Orphan cleanup for the session's mark: `None` if the child had no mark.
    pub orphans: Option<Result<OrphanReport>>,
}

/// A spawned child that this process owns until [`Child::reap`].
#[derive(Debug)]
pub struct Child {
    pid: libc::pid_t,
    id: Option<ProcessId>,
    mark: Option<String>,
    stdin: Option<File>,
    stdout: Option<File>,
    stderr: Option<File>,
    pty: Option<Pty>,
    cleanup: Vec<CleanupPath>,
    cleanup_root: Option<PathBuf>,
    tool_sessions: BTreeSet<ProcessId>,
    /// The child has exited (seen with `WNOWAIT`; still a zombie until reaped).
    exited: bool,
    reaped: bool,
    orphan_grace: Duration,
}

impl Child {
    /// The child's pid; also its process group id (and, on a terminal, its session id).
    pub fn pid(&self) -> u32 {
        u32::try_from(self.pid).unwrap_or(0)
    }

    /// The child's identity (pid and start time) as read right after the spawn, for the
    /// record and the host's watchdog. `None` only if it was already gone by then.
    pub fn id(&self) -> Option<ProcessId> {
        self.id
    }

    /// `ORG_SESSION_MARK`, if its environment has one.
    pub fn mark(&self) -> Option<&str> {
        self.mark.as_deref()
    }

    /// Takes the write end of the child's stdin (piped children only). Whoever holds it closes
    /// it; [`Child::stop`]'s `CloseStdin` step can then only wait.
    pub fn take_stdin(&mut self) -> Option<File> {
        self.stdin.take()
    }

    /// Takes the read end of the child's stdout (piped children only).
    pub fn take_stdout(&mut self) -> Option<File> {
        self.stdout.take()
    }

    /// Takes the read end of the child's stderr (piped children only).
    pub fn take_stderr(&mut self) -> Option<File> {
        self.stderr.take()
    }

    /// Takes the terminal's master (terminal children only).
    pub fn take_pty(&mut self) -> Option<Pty> {
        self.pty.take()
    }

    /// Closes the child's stdin, if this `Child` still holds it.
    pub fn close_stdin(&mut self) {
        self.stdin = None;
    }

    /// Changes how long orphans get between `SIGTERM` and `SIGKILL` in [`Child::reap`]
    /// (default [`ORPHAN_GRACE`]).
    pub fn set_orphan_grace(&mut self, grace: Duration) {
        self.orphan_grace = grace;
    }

    /// Whether the child has exited (it stays an unreaped zombie until [`Child::reap`]).
    pub fn has_exited(&mut self) -> Result<bool> {
        if !self.exited {
            self.exited = self.peek(false)?;
        }
        Ok(self.exited)
    }

    /// `waitid(WNOWAIT)`. If the child is no longer ours to wait for (`ECHILD`: something
    /// else reaped it, or `SIGCHLD` is ignored), its pid may already be reused, so this
    /// `Child` never signals it again.
    fn peek(&mut self, block: bool) -> Result<bool> {
        match sys::has_exited(self.pid, block) {
            Ok(exited) => Ok(exited),
            Err(e) => {
                if e.raw_os_error() == Some(libc::ECHILD) {
                    self.reaped = true;
                    self.exited = true;
                }
                Err(Error::io("wait for the child", e))
            }
        }
    }

    /// Waits up to `timeout` (or for ever, with `None`) for the child to exit, without
    /// reaping it. Returns whether it has exited.
    pub fn wait_exit(&mut self, timeout: Option<Duration>) -> Result<bool> {
        let Some(timeout) = timeout else {
            if !self.exited {
                self.exited = self.peek(true)?;
            }
            return Ok(true);
        };
        let until = Instant::now() + timeout;
        let mut nap = Duration::from_millis(1);
        loop {
            if self.has_exited()? {
                return Ok(true);
            }
            let now = Instant::now();
            if now >= until {
                return Ok(false);
            }
            thread::sleep(nap.min(until - now));
            nap = (nap * 2).min(POLL_MAX);
        }
    }

    /// Records the child's direct children that lead sessions of their own (Claude runs each
    /// Bash tool that way), by pid and start time, so [`Child::reap`]'s orphan cleanup can
    /// kill those sessions whole while their leaders live. Their environment (and so the
    /// mark) is unreadable when they're Apple binaries such as `/bin/zsh` (see
    /// [`crate::orphans`]).
    ///
    /// [`Child::stop`] and [`Child::kill`] call it themselves just before signalling; call it
    /// also now and then while the session runs (e.g. on each tool start), so a session that
    /// crashes is covered too. The records live in memory only.
    pub fn record_tool_sessions(&mut self) -> Result<()> {
        if self.exited || self.reaped {
            return Ok(());
        }
        let own_sid = sys::getsid(self.pid).ok();
        let kids = sys::list_pids(PidFilter::Ppid(self.pid))
            .map_err(|e| Error::io("list the child's children", e))?;
        for kid in kids {
            if sys::getsid(kid).is_ok_and(|sid| sid == kid && Some(sid) != own_sid) {
                if let Ok(Some(info)) = sys::proc_info(kid) {
                    if info.ppid == self.pid && !info.zombie {
                        self.tool_sessions.insert(ProcessId {
                            pid: u32::try_from(kid).unwrap_or(0),
                            start_us: info.start_us,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// The tool sessions recorded so far (for a record the host's watchdog can use while
    /// this process lives).
    pub fn tool_sessions(&self) -> Vec<ProcessId> {
        self.tool_sessions.iter().copied().collect()
    }

    /// The stop sequence. Call it once the session has had its say (the pump closes stdin on
    /// the session's `CloseStdin` and waits, or gives up after a timeout):
    /// 1. `SIGTERM` to the runtime's own pid, always (Claude kills its tools' trees on it),
    ///    then up to `term_grace` for the exit;
    /// 2. the plan's steps in order, each followed by its grace, skipped once the child has
    ///    exited (`CloseStdin` closes the stdin this `Child` still holds; a terminal child has
    ///    no stdin pipe, so for it the step only waits);
    /// 3. `SIGKILL` to the whole group (and the pid, in case it left the group).
    ///
    /// It doesn't reap: drain the pipes, then call [`Child::reap`], which runs the cleanup
    /// and the orphan scan.
    pub fn stop(&mut self, plan: &StopPlan, term_grace: Duration) -> Result<()> {
        if !self.has_exited()? {
            let _ = self.record_tool_sessions();
            self.signal_leader(libc::SIGTERM);
            self.wait_exit(Some(term_grace))?;
        }
        for step in &plan.steps {
            if self.has_exited()? {
                break;
            }
            match *step {
                StopStep::CloseStdin { grace_ms } => {
                    self.close_stdin();
                    self.wait_exit(Some(Duration::from_millis(grace_ms)))?;
                }
                StopStep::Signal {
                    signal,
                    target,
                    grace_ms,
                } => {
                    let sig = match signal {
                        StopSignal::Interrupt => libc::SIGINT,
                        StopSignal::Terminate => libc::SIGTERM,
                    };
                    match target {
                        SignalTarget::Leader => self.signal_leader(sig),
                        SignalTarget::Group => self.signal_group(sig),
                    }
                    self.wait_exit(Some(Duration::from_millis(grace_ms)))?;
                }
            }
        }
        self.kill();
        Ok(())
    }

    /// `SIGKILL` to the child's group and to the child itself, at once (recording its tool
    /// sessions first). It doesn't reap.
    pub fn kill(&mut self) {
        let _ = self.record_tool_sessions();
        self.signal_group(libc::SIGKILL);
        self.signal_leader(libc::SIGKILL);
    }

    /// Signals the runtime's own pid. Safe until it's reaped: a zombie's pid can't be reused.
    fn signal_leader(&self, sig: libc::c_int) {
        if !self.reaped {
            // ESRCH (already gone) needs no handling.
            let _ = sys::kill(self.pid, sig);
        }
    }

    /// Signals the child's group. Safe until the leader is reaped: its pid, and so the group
    /// id, can't be reused while it's a zombie. On macOS `killpg` fails with `EPERM` for a
    /// group that holds only a zombie: that group is already gone.
    fn signal_group(&self, sig: libc::c_int) {
        if !self.reaped {
            let _ = sys::killpg(self.pid, sig);
        }
    }

    /// Waits for the exit (for ever: stop it first if it must end), `SIGKILL`s what's left of
    /// its group, removes the cleanup paths while the child is still a zombie, runs orphan
    /// cleanup for its mark, then reaps it.
    pub fn reap(mut self) -> Result<Exit> {
        self.reap_inner()
    }

    fn reap_inner(&mut self) -> Result<Exit> {
        self.wait_exit(None)?;
        self.kill();
        // The child is an unreaped zombie here, so its pid names no other process.
        let mut cleanup = Vec::new();
        if let Some(root) = &self.cleanup_root {
            for template in &self.cleanup {
                let path = template.path_for(self.pid());
                let outcome = cleanup::remove_path(&path, root);
                cleanup.push(CleanupReport { path, outcome });
            }
        }
        let tool_sessions = self.tool_sessions();
        let orphans = self.mark.as_deref().map(|mark| {
            orphans::kill_orphans(&OrphanQuery {
                mark,
                group_leader: None,
                tool_sessions: &tool_sessions,
                grace: self.orphan_grace,
            })
        });
        let status = sys::reap(self.pid);
        // Reaped, or not ours to reap: either way its pid must not be signalled again.
        self.reaped = true;
        let status = status.map_err(|e| Error::io("reap the child", e))?;
        Ok(Exit {
            info: ExitInfo {
                code: status.code,
                signal: status.signal,
            },
            cleanup,
            orphans,
        })
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if !self.reaped {
            self.kill();
            let _ = self.reap_inner();
        }
    }
}
