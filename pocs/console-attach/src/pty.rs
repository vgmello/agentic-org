use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

static SIGWINCH_RECEIVED: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigwinch(_sig: libc::c_int) {
    SIGWINCH_RECEIVED.store(true, Ordering::Relaxed);
}

/// Terminal dimensions in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        Self { rows: 24, cols: 80 }
    }
}

/// Returns whether stdin and stdout are both interactive terminals.
pub fn is_interactive_tty() -> bool {
    // SAFETY: isatty is a standard POSIX query with no memory safety preconditions.
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 && libc::isatty(libc::STDOUT_FILENO) == 1 }
}

/// Queries the current terminal window size, falling back to `PtySize::default()`.
pub fn current_terminal_size() -> PtySize {
    for fd in [libc::STDOUT_FILENO, libc::STDIN_FILENO] {
        let mut ws = MaybeUninit::<libc::winsize>::zeroed();
        // SAFETY: ws points to a valid writable winsize struct.
        let rc = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, ws.as_mut_ptr()) };
        if rc == 0 {
            // SAFETY: ioctl succeeded and initialized ws.
            let ws = unsafe { ws.assume_init() };
            if ws.ws_row > 0 && ws.ws_col > 0 {
                return PtySize {
                    rows: ws.ws_row,
                    cols: ws.ws_col,
                };
            }
        }
    }
    PtySize::default()
}

/// Installs a minimal async-signal-safe SIGWINCH handler.
pub fn install_sigwinch_handler() -> io::Result<()> {
    // SAFETY: handle_sigwinch only performs a relaxed atomic store, which is async-signal-safe.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle_sigwinch as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_flags = libc::SA_RESTART;
        if libc::sigaction(libc::SIGWINCH, &action, ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Returns true once if a SIGWINCH signal was received since the last check.
pub fn take_sigwinch() -> bool {
    SIGWINCH_RECEIVED.swap(false, Ordering::Relaxed)
}

/// RAII guard that switches stdin into raw mode and restores the original termios on drop.
pub struct RawTerminalGuard {
    fd: RawFd,
    original: libc::termios,
}

impl RawTerminalGuard {
    /// Enters raw mode on `STDIN_FILENO`. Fails if stdin is not a TTY.
    pub fn enter() -> io::Result<Self> {
        let fd = libc::STDIN_FILENO;
        let mut original = MaybeUninit::<libc::termios>::zeroed();
        // SAFETY: original points to valid writable memory.
        if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr succeeded.
        let original = unsafe { original.assume_init() };
        let mut raw = original;
        // SAFETY: raw is a valid initialized termios struct.
        unsafe {
            libc::cfmakeraw(&mut raw);
            if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self { fd, original })
    }
}

impl Drop for RawTerminalGuard {
    fn drop(&mut self) {
        // SAFETY: self.original was retrieved by tcgetattr on self.fd.
        unsafe {
            let _ = libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}

/// A child process running inside a dedicated Unix pseudo-terminal (PTY).
pub struct PtySession {
    master: File,
    child: Child,
}

impl PtySession {
    /// Spawns `program` with `args` attached to a newly allocated PTY of `size`.
    pub fn spawn<S, I, T>(
        program: S,
        args: I,
        size: PtySize,
        term: Option<&str>,
    ) -> io::Result<Self>
    where
        S: AsRef<OsStr>,
        I: IntoIterator<Item = T>,
        T: AsRef<OsStr>,
    {
        Self::spawn_with_env(program, args, size, term, &[])
    }

    /// Spawns `program` with `args` and additional environment variables inside a newly allocated PTY.
    pub fn spawn_with_env<S, I, T>(
        program: S,
        args: I,
        size: PtySize,
        term: Option<&str>,
        extra_env: &[(String, String)],
    ) -> io::Result<Self>
    where
        S: AsRef<OsStr>,
        I: IntoIterator<Item = T>,
        T: AsRef<OsStr>,
    {
        let mut master_raw: RawFd = -1;
        let mut slave_raw: RawFd = -1;
        let mut ws = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // SAFETY: valid pointers for openpty output FDs and initial window size.
        let rc = unsafe {
            libc::openpty(
                &mut master_raw,
                &mut slave_raw,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut ws,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: openpty returned fresh valid owned file descriptors.
        let master_fd = unsafe { OwnedFd::from_raw_fd(master_raw) };
        let slave_fd = unsafe { OwnedFd::from_raw_fd(slave_raw) };

        set_cloexec(master_fd.as_raw_fd())?;
        set_cloexec(slave_fd.as_raw_fd())?;

        let slave_for_child = slave_fd.as_raw_fd();
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        if let Some(term_val) = term {
            cmd.env("TERM", term_val);
        } else if std::env::var_os("TERM").is_none() {
            cmd.env("TERM", "xterm-256color");
        }
        for (k, v) in extra_env {
            cmd.env(k, v);
        }

        // SAFETY: closure runs between fork and exec and only calls async-signal-safe syscalls.
        unsafe {
            cmd.pre_exec(move || {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(slave_for_child, libc::TIOCSCTTY.into(), 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                for target_fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
                    if libc::dup2(slave_for_child, target_fd) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                }
                if slave_for_child > libc::STDERR_FILENO {
                    libc::close(slave_for_child);
                }
                Ok(())
            });
        }

        let child = cmd.spawn()?;
        // Drop the parent's slave handle so EOF/EIO is delivered when the child closes its TTY.
        drop(slave_fd);

        let master = File::from(master_fd);
        Ok(Self { master, child })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Clones the master PTY file handle for concurrent reading/writing across threads.
    pub fn clone_master(&self) -> io::Result<File> {
        self.master.try_clone()
    }

    /// Updates the PTY window size and notifies the foreground process group (SIGWINCH).
    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        resize_fd(self.master.as_raw_fd(), size)
    }

    /// Writes bytes directly into the PTY master as if typed on the terminal keyboard.
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)?;
        self.master.flush()
    }

    /// Reads available output from the PTY master.
    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.master.read(buf)
    }

    /// Checks whether the child process has exited without blocking.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Blocks until the child process exits.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }

    /// Sends SIGTERM/SIGKILL if the child is still running.
    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }
}

/// Resizes an open PTY master file descriptor via `TIOCSWINSZ`.
pub fn resize_fd(fd: RawFd, size: PtySize) -> io::Result<()> {
    let ws = libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: ws points to a valid initialized winsize struct.
    if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &ws) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl F_GETFD/F_SETFD on an open descriptor.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags == -1 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
