use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::buffer::OutputBuffer;
use crate::keys::{translate_command, InputAction};
use crate::protocol::{write_err, write_ok, ControlRequest};
use crate::pty::{
    current_terminal_size, install_sigwinch_handler, is_interactive_tty, resize_fd, take_sigwinch,
    PtySession, PtySize, RawTerminalGuard,
};

/// Configuration for launching a program inside an intercepted PTY.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub program: String,
    pub args: Vec<String>,
    pub socket_path: PathBuf,
    pub headless: bool,
    pub size: Option<PtySize>,
    pub term: Option<String>,
}

struct SharedState {
    pid: u32,
    alive: AtomicBool,
    shutdown: AtomicBool,
    size: Mutex<PtySize>,
    master_write: Mutex<File>,
    output: OutputBuffer,
}

struct SocketFileGuard {
    path: PathBuf,
}

impl Drop for SocketFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Running PTY proxy session handle (useful for embedding or automated tests).
pub struct RunningProxy {
    session: PtySession,
    shared: Arc<SharedState>,
    _raw_guard: Option<RawTerminalGuard>,
    _socket_guard: SocketFileGuard,
    reader_thread: Option<JoinHandle<()>>,
    stdin_thread: Option<JoinHandle<()>>,
    control_thread: Option<JoinHandle<()>>,
}

impl RunningProxy {
    /// Starts the target program inside a PTY and opens the background control socket.
    pub fn start(config: ProxyConfig) -> io::Result<Self> {
        let attached = !config.headless && is_interactive_tty();
        let initial_size = config.size.unwrap_or_else(|| {
            if attached {
                current_terminal_size()
            } else {
                PtySize::default()
            }
        });

        let listener = bind_control_socket(&config.socket_path)?;
        listener.set_nonblocking(true)?;
        let socket_guard = SocketFileGuard {
            path: config.socket_path.clone(),
        };

        let raw_guard = if attached {
            install_sigwinch_handler()?;
            Some(RawTerminalGuard::enter()?)
        } else {
            None
        };

        let session = PtySession::spawn(
            &config.program,
            &config.args,
            initial_size,
            config.term.as_deref(),
        )?;

        let master_read = session.clone_master()?;
        let master_write = session.clone_master()?;

        let shared = Arc::new(SharedState {
            pid: session.pid(),
            alive: AtomicBool::new(true),
            shutdown: AtomicBool::new(false),
            size: Mutex::new(initial_size),
            master_write: Mutex::new(master_write),
            output: OutputBuffer::default(),
        });

        // Thread 1: PTY master -> rolling OutputBuffer (+ real stdout when attached).
        let reader_shared = Arc::clone(&shared);
        let reader_thread = thread::Builder::new()
            .name("pty-reader".to_string())
            .spawn(move || pty_reader_loop(master_read, reader_shared, attached))?;

        // Thread 2: Real stdin -> PTY master (+ SIGWINCH resize forwarding) when attached.
        let stdin_thread = if attached {
            let stdin_shared = Arc::clone(&shared);
            Some(
                thread::Builder::new()
                    .name("pty-stdin".to_string())
                    .spawn(move || stdin_forward_loop(stdin_shared))?,
            )
        } else {
            None
        };

        // Thread 3: Unix domain socket background command server.
        let control_shared = Arc::clone(&shared);
        let control_thread = thread::Builder::new()
            .name("pty-control".to_string())
            .spawn(move || control_server_loop(listener, control_shared))?;

        Ok(Self {
            session,
            shared,
            _raw_guard: raw_guard,
            _socket_guard: socket_guard,
            reader_thread: Some(reader_thread),
            stdin_thread,
            control_thread: Some(control_thread),
        })
    }

    pub fn pid(&self) -> u32 {
        self.shared.pid
    }

    /// Waits until the PTY child emits at least one byte (useful to ensure startup completed).
    pub fn wait_for_any_output(&self, timeout: Duration) -> bool {
        self.shared.output.wait_for_any_output(timeout)
    }

    /// Blocks until the child process exits, then cleans up threads, socket, and raw terminal mode.
    pub fn wait(mut self) -> io::Result<ExitStatus> {
        let status = self.session.wait()?;
        self.shared.alive.store(false, Ordering::SeqCst);
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.join_threads();
        Ok(status)
    }

    /// Kills the child process if still running and cleans up all resources.
    pub fn kill_and_wait(mut self) -> io::Result<ExitStatus> {
        let _ = self.session.kill();
        let status = self.session.wait()?;
        self.shared.alive.store(false, Ordering::SeqCst);
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.join_threads();
        Ok(status)
    }

    fn join_threads(&mut self) {
        if let Some(h) = self.control_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.stdin_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.reader_thread.take() {
            let _ = h.join();
        }
    }
}

/// Convenience helper to run a proxy synchronously until the child exits.
pub fn run_proxy(config: ProxyConfig) -> io::Result<ExitStatus> {
    let proxy = RunningProxy::start(config)?;
    proxy.wait()
}

fn bind_control_socket(path: &Path) -> io::Result<UnixListener> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    if path.exists() {
        match UnixStream::connect(path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!(
                        "control socket {} is already active (pass --socket <other-path>)",
                        path.display()
                    ),
                ));
            }
            Err(_) => {
                // Stale socket file from a previous run.
                let _ = fs::remove_file(path);
            }
        }
    }
    let listener = UnixListener::bind(path)?;
    // Restrict socket access to the current user only.
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    Ok(listener)
}

fn pty_reader_loop(mut master_read: File, shared: Arc<SharedState>, mirror_stdout: bool) {
    let mut buf = [0u8; 4096];
    let mut stdout = io::stdout();

    loop {
        match master_read.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let chunk = &buf[..n];
                shared.output.push(chunk);
                if mirror_stdout {
                    let _ = stdout.write_all(chunk);
                    let _ = stdout.flush();
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            // EIO is normal on Unix PTY masters when the slave side closes.
            Err(err) if err.raw_os_error() == Some(libc::EIO) => break,
            Err(_) => break,
        }
    }
    shared.output.mark_closed();
}

fn stdin_forward_loop(shared: Arc<SharedState>) {
    let mut buf = [0u8; 1024];

    while !shared.shutdown.load(Ordering::Relaxed) {
        if take_sigwinch() {
            let new_size = current_terminal_size();
            if let Ok(master) = shared.master_write.lock() {
                if resize_fd(master.as_raw_fd(), new_size).is_ok() {
                    if let Ok(mut s) = shared.size.lock() {
                        *s = new_size;
                    }
                }
            }
        }

        let mut pfd = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: pfd points to a single valid pollfd struct.
        let rc = unsafe { libc::poll(&mut pfd, 1, 25) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if rc == 0 {
            continue;
        }
        if (pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0
            && (pfd.revents & libc::POLLIN) == 0
        {
            break;
        }
        if (pfd.revents & libc::POLLIN) != 0 {
            // SAFETY: buf is a valid writable byte buffer.
            let n = unsafe {
                libc::read(
                    libc::STDIN_FILENO,
                    buf.as_mut_ptr().cast::<libc::c_void>(),
                    buf.len(),
                )
            };
            if n <= 0 {
                break;
            }
            let bytes = &buf[..n as usize];
            if let Ok(mut master) = shared.master_write.lock() {
                if master.write_all(bytes).is_err() || master.flush().is_err() {
                    break;
                }
            }
        }
    }
}

fn control_server_loop(listener: UnixListener, shared: Arc<SharedState>) {
    while !shared.shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let conn_shared = Arc::clone(&shared);
                let _ = thread::Builder::new()
                    .name("pty-control-conn".to_string())
                    .spawn(move || handle_control_connection(stream, conn_shared));
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(15));
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn handle_control_connection(stream: UnixStream, shared: Arc<SharedState>) {
    // On macOS/BSD, accepted streams inherit non-blocking mode from the listener.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut writer = stream;

    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) | Err(_) => return,
        Ok(_) => {}
    }

    let request = match ControlRequest::parse_line(&line) {
        Ok(req) => req,
        Err(msg) => {
            let _ = write_err(&mut writer, &msg);
            return;
        }
    };

    match request {
        ControlRequest::Ping => {
            let _ = write_ok(&mut writer, b"pong");
        }
        ControlRequest::Status => {
            let alive = shared.alive.load(Ordering::Relaxed);
            let size = shared.size.lock().map(|g| *g).unwrap_or_default();
            let reply = format!(
                "pid={} alive={} rows={} cols={}",
                shared.pid, alive, size.rows, size.cols
            );
            let _ = write_ok(&mut writer, reply.as_bytes());
        }
        ControlRequest::Send(cmd_str) => match translate_command(&cmd_str) {
            Ok(actions) => match execute_actions(&shared, &actions) {
                Ok(total_bytes) => {
                    let reply = format!("sent {total_bytes} bytes");
                    let _ = write_ok(&mut writer, reply.as_bytes());
                }
                Err(err) => {
                    let _ = write_err(&mut writer, &format!("failed writing to PTY: {err}"));
                }
            },
            Err(err) => {
                let _ = write_err(&mut writer, &err);
            }
        },
        ControlRequest::Screen => {
            let text = shared.output.snapshot_plain();
            let _ = write_ok(&mut writer, text.as_bytes());
        }
        ControlRequest::Raw => {
            let bytes = shared.output.snapshot_raw();
            let _ = write_ok(&mut writer, &bytes);
        }
        ControlRequest::WaitFor { timeout_ms, needle } => {
            let matched = shared
                .output
                .wait_for(&needle, Duration::from_millis(timeout_ms));
            if matched {
                let _ = write_ok(&mut writer, b"matched");
            } else {
                let _ = write_err(
                    &mut writer,
                    &format!("timed out waiting {timeout_ms}ms for '{needle}'"),
                );
            }
        }
        ControlRequest::Resize { rows, cols } => {
            let new_size = PtySize { rows, cols };
            let res = match shared.master_write.lock() {
                Ok(master) => resize_fd(master.as_raw_fd(), new_size),
                Err(_) => Err(io::Error::other("PTY lock poisoned")),
            };
            match res {
                Ok(()) => {
                    if let Ok(mut s) = shared.size.lock() {
                        *s = new_size;
                    }
                    let reply = format!("resized to {rows}x{cols}");
                    let _ = write_ok(&mut writer, reply.as_bytes());
                }
                Err(err) => {
                    let _ = write_err(&mut writer, &format!("resize failed: {err}"));
                }
            }
        }
    }
}

fn execute_actions(shared: &SharedState, actions: &[InputAction]) -> io::Result<usize> {
    // Hold the master write mutex across the whole translated action sequence so concurrent
    // human typing or another control client cannot interleave bytes mid-escape-sequence.
    let mut master = shared
        .master_write
        .lock()
        .map_err(|_| io::Error::other("PTY write lock poisoned"))?;
    let mut total_bytes = 0usize;
    for action in actions {
        match action {
            InputAction::SendBytes(bytes) => {
                master.write_all(bytes)?;
                master.flush()?;
                total_bytes += bytes.len();
            }
            InputAction::Pause(duration) => {
                thread::sleep(*duration);
            }
        }
    }
    Ok(total_bytes)
}
