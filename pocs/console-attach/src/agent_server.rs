use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::agent::{AgentEvent, AgentStatus};
use crate::claude::{ClaudeConfig, ClaudeSession};
use crate::protocol::{write_err, write_ok};

/// JSON request sent by `AgentClient` to the background agent daemon over Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AgentSocketRequest {
    Ping,
    Status,
    Send {
        message: String,
    },
    SendWait {
        message: String,
        timeout_ms: u64,
    },
    Interrupt,
    Approve {
        request_id: String,
    },
    Deny {
        request_id: String,
        reason: Option<String>,
    },
    Events {
        since_seq: u64,
    },
    WaitEvents {
        since_seq: u64,
        timeout_ms: u64,
    },
    Stop,
}

struct SocketGuard {
    path: PathBuf,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Running background daemon exposing a long-lived `ClaudeSession` on a Unix domain socket.
pub struct RunningAgentDaemon {
    session: Arc<ClaudeSession>,
    shutdown: Arc<AtomicBool>,
    _socket_guard: SocketGuard,
    server_thread: Option<JoinHandle<()>>,
}

impl RunningAgentDaemon {
    pub fn start_claude(socket_path: PathBuf, config: ClaudeConfig) -> io::Result<Self> {
        let listener = bind_agent_socket(&socket_path)?;
        listener.set_nonblocking(true)?;
        let socket_guard = SocketGuard { path: socket_path };

        let session = ClaudeSession::spawn(config)?;
        let shutdown = Arc::new(AtomicBool::new(false));

        let server_session = Arc::clone(&session);
        let server_shutdown = Arc::clone(&shutdown);
        let server_thread = thread::Builder::new()
            .name("agent-socket-server".to_string())
            .spawn(move || server_loop(listener, server_session, server_shutdown))?;

        Ok(Self {
            session,
            shutdown,
            _socket_guard: socket_guard,
            server_thread: Some(server_thread),
        })
    }

    pub fn pid(&self) -> u32 {
        self.session.pid()
    }

    pub fn stop(&self) {
        self.session.request_stop();
    }

    /// Blocks until the underlying Claude process exits, then tears down the control socket.
    pub fn wait(mut self) -> io::Result<ExitStatus> {
        let status = self.session.wait()?;
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(h) = self.server_thread.take() {
            let _ = h.join();
        }
        Ok(status)
    }
}

fn bind_agent_socket(path: &Path) -> io::Result<UnixListener> {
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
                        "agent socket {} is already active (pass --socket <other-path>)",
                        path.display()
                    ),
                ));
            }
            Err(_) => {
                let _ = fs::remove_file(path);
            }
        }
    }
    let listener = UnixListener::bind(path)?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    Ok(listener)
}

fn server_loop(listener: UnixListener, session: Arc<ClaudeSession>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let s = Arc::clone(&session);
                let _ = thread::Builder::new()
                    .name("agent-socket-conn".to_string())
                    .spawn(move || handle_connection(stream, s));
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(15));
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn handle_connection(stream: UnixStream, session: Arc<ClaudeSession>) {
    // On macOS/BSD, accepted streams inherit non-blocking mode from the listener.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

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

    let req: AgentSocketRequest = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(err) => {
            let _ = write_err(&mut writer, &format!("invalid request JSON: {err}"));
            return;
        }
    };

    match req {
        AgentSocketRequest::Ping => {
            let _ = write_json_ok(&mut writer, &json!({ "ok": true, "pong": true }));
        }
        AgentSocketRequest::Status => {
            let status = session.status();
            let _ = write_json_ok(&mut writer, &status);
        }
        AgentSocketRequest::Send { message } => match session.send_user_message(&message) {
            Ok(ev) => {
                let _ = write_json_ok(&mut writer, &ev);
            }
            Err(err) => {
                let _ = write_err(&mut writer, &err.to_string());
            }
        },
        AgentSocketRequest::SendWait {
            message,
            timeout_ms,
        } => {
            let _ = writer.set_write_timeout(Some(Duration::from_millis(timeout_ms + 5_000)));
            match session.send_and_wait(&message, Duration::from_millis(timeout_ms)) {
                Ok(ev) => {
                    let _ = write_json_ok(&mut writer, &ev);
                }
                Err(err) => {
                    let _ = write_err(&mut writer, &err.to_string());
                }
            }
        }
        AgentSocketRequest::Interrupt => match session.send_interrupt() {
            Ok(rid) => {
                let _ = write_json_ok(&mut writer, &json!({ "request_id": rid }));
            }
            Err(err) => {
                let _ = write_err(&mut writer, &err.to_string());
            }
        },
        AgentSocketRequest::Approve { request_id } => {
            match session.respond_approval(&request_id, true, None) {
                Ok(()) => {
                    let _ = write_json_ok(
                        &mut writer,
                        &json!({ "request_id": request_id, "behavior": "allow" }),
                    );
                }
                Err(err) => {
                    let _ = write_err(&mut writer, &err.to_string());
                }
            }
        }
        AgentSocketRequest::Deny { request_id, reason } => {
            match session.respond_approval(&request_id, false, reason.as_deref()) {
                Ok(()) => {
                    let _ = write_json_ok(
                        &mut writer,
                        &json!({ "request_id": request_id, "behavior": "deny" }),
                    );
                }
                Err(err) => {
                    let _ = write_err(&mut writer, &err.to_string());
                }
            }
        }
        AgentSocketRequest::Events { since_seq } => {
            let events = session.events_since(since_seq);
            let _ = write_json_ok(&mut writer, &events);
        }
        AgentSocketRequest::WaitEvents {
            since_seq,
            timeout_ms,
        } => {
            let _ = writer.set_write_timeout(Some(Duration::from_millis(timeout_ms + 5_000)));
            let events = session.wait_events_since(since_seq, Duration::from_millis(timeout_ms));
            let _ = write_json_ok(&mut writer, &events);
        }
        AgentSocketRequest::Stop => {
            session.request_stop();
            let _ = write_json_ok(&mut writer, &json!({ "stopping": true }));
        }
    }
}

fn write_json_ok<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let bytes =
        serde_json::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_ok(stream, &bytes)
}

/// Client for controlling a running structured agent daemon over its Unix domain socket.
pub struct AgentClient {
    socket_path: PathBuf,
}

impl AgentClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn status(&self) -> io::Result<AgentStatus> {
        self.call(&AgentSocketRequest::Status, None)
    }

    /// Sends a user message immediately without waiting for the turn to complete.
    pub fn send(&self, message: &str) -> io::Result<AgentEvent> {
        self.call(
            &AgentSocketRequest::Send {
                message: message.to_string(),
            },
            None,
        )
    }

    /// Sends a user message and waits for the next `result` event.
    pub fn send_wait(&self, message: &str, timeout: Duration) -> io::Result<AgentEvent> {
        let timeout_ms = timeout.as_millis().min(u128::from(u64::MAX)) as u64;
        self.call(
            &AgentSocketRequest::SendWait {
                message: message.to_string(),
                timeout_ms,
            },
            Some(timeout + Duration::from_secs(5)),
        )
    }

    pub fn interrupt(&self) -> io::Result<serde_json::Value> {
        self.call(&AgentSocketRequest::Interrupt, None)
    }

    pub fn approve(&self, request_id: &str) -> io::Result<serde_json::Value> {
        self.call(
            &AgentSocketRequest::Approve {
                request_id: request_id.to_string(),
            },
            None,
        )
    }

    pub fn deny(&self, request_id: &str, reason: Option<&str>) -> io::Result<serde_json::Value> {
        self.call(
            &AgentSocketRequest::Deny {
                request_id: request_id.to_string(),
                reason: reason.map(ToString::to_string),
            },
            None,
        )
    }

    pub fn events(&self, since_seq: u64) -> io::Result<Vec<AgentEvent>> {
        self.call(&AgentSocketRequest::Events { since_seq }, None)
    }

    pub fn wait_events(&self, since_seq: u64, timeout: Duration) -> io::Result<Vec<AgentEvent>> {
        let timeout_ms = timeout.as_millis().min(u128::from(u64::MAX)) as u64;
        self.call(
            &AgentSocketRequest::WaitEvents {
                since_seq,
                timeout_ms,
            },
            Some(timeout + Duration::from_secs(5)),
        )
    }

    pub fn stop(&self) -> io::Result<serde_json::Value> {
        self.call(&AgentSocketRequest::Stop, None)
    }

    fn call<R: for<'de> Deserialize<'de>>(
        &self,
        req: &AgentSocketRequest,
        read_timeout: Option<Duration>,
    ) -> io::Result<R> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        if let Some(t) = read_timeout {
            let _ = stream.set_read_timeout(Some(t));
        }

        let line = serde_json::to_string(req)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        writeln!(stream, "{line}")?;
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "agent daemon closed connection without replying",
            ));
        }
        let header = header.trim_end_matches(['\r', '\n']);

        if let Some(len_str) = header.strip_prefix("OK ") {
            let len: usize = len_str.parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid OK length header: '{header}'"),
                )
            })?;
            let mut payload = vec![0u8; len];
            reader.read_exact(&mut payload)?;
            serde_json::from_slice(&payload)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        } else if let Some(err_msg) = header.strip_prefix("ERR ") {
            Err(io::Error::other(err_msg.to_string()))
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected response header: '{header}'"),
            ))
        }
    }
}
