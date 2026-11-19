use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Parsed control request sent over the Unix domain socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlRequest {
    Ping,
    Status,
    Send(String),
    Screen,
    Raw,
    WaitFor { timeout_ms: u64, needle: String },
    Resize { rows: u16, cols: u16 },
}

impl ControlRequest {
    pub fn parse_line(line: &str) -> Result<Self, String> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let (verb, rest) = match trimmed.find(' ') {
            Some(idx) => (&trimmed[..idx], &trimmed[idx + 1..]),
            None => (trimmed, ""),
        };

        match verb {
            "PING" => Ok(Self::Ping),
            "STATUS" => Ok(Self::Status),
            "SEND" => {
                if rest.trim().is_empty() {
                    return Err("SEND requires a command argument".to_string());
                }
                Ok(Self::Send(rest.to_string()))
            }
            "SCREEN" => Ok(Self::Screen),
            "RAW" => Ok(Self::Raw),
            "WAIT_FOR" => {
                let Some((ms_str, needle)) = rest.split_once(' ') else {
                    return Err("usage: WAIT_FOR <timeout_ms> <substring>".to_string());
                };
                let timeout_ms: u64 = ms_str
                    .parse()
                    .map_err(|_| format!("invalid timeout_ms: '{ms_str}'"))?;
                if needle.is_empty() {
                    return Err("WAIT_FOR substring must not be empty".to_string());
                }
                Ok(Self::WaitFor {
                    timeout_ms,
                    needle: needle.to_string(),
                })
            }
            "RESIZE" => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() != 2 {
                    return Err("usage: RESIZE <rows> <cols>".to_string());
                }
                let rows: u16 = parts[0]
                    .parse()
                    .map_err(|_| format!("invalid rows: '{}'", parts[0]))?;
                let cols: u16 = parts[1]
                    .parse()
                    .map_err(|_| format!("invalid cols: '{}'", parts[1]))?;
                if rows == 0 || cols == 0 {
                    return Err("rows and cols must be > 0".to_string());
                }
                Ok(Self::Resize { rows, cols })
            }
            other => Err(format!("unknown control request: '{other}'")),
        }
    }
}

/// Writes an `OK <len>\n<bytes>` framed response to the socket.
pub fn write_ok(stream: &mut UnixStream, payload: &[u8]) -> io::Result<()> {
    writeln!(stream, "OK {}", payload.len())?;
    stream.write_all(payload)?;
    stream.flush()
}

/// Writes an `ERR <message>\n` response to the socket.
pub fn write_err(stream: &mut UnixStream, message: &str) -> io::Result<()> {
    let sanitized = message.replace(['\r', '\n'], " ");
    writeln!(stream, "ERR {sanitized}")?;
    stream.flush()
}

/// Client for sending background control commands to a running `console-attach` proxy.
pub struct ControlClient {
    socket_path: std::path::PathBuf,
}

impl ControlClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn ping(&self) -> io::Result<String> {
        self.request_text("PING")
    }

    pub fn status(&self) -> io::Result<String> {
        self.request_text("STATUS")
    }

    pub fn send_command(&self, command: &str) -> io::Result<String> {
        if command.contains('\n') || command.contains('\r') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "control command must be a single line (use `key enter` or `line <text>` for newlines)",
            ));
        }
        self.request_text(&format!("SEND {command}"))
    }

    pub fn screen(&self) -> io::Result<String> {
        self.request_text("SCREEN")
    }

    pub fn raw(&self) -> io::Result<Vec<u8>> {
        self.request_bytes("RAW")
    }

    pub fn wait_for(&self, timeout: Duration, needle: &str) -> io::Result<String> {
        let ms = timeout.as_millis().min(u128::from(u64::MAX)) as u64;
        self.request_text(&format!("WAIT_FOR {ms} {needle}"))
    }

    pub fn resize(&self, rows: u16, cols: u16) -> io::Result<String> {
        self.request_text(&format!("RESIZE {rows} {cols}"))
    }

    fn request_text(&self, line: &str) -> io::Result<String> {
        let bytes = self.request_bytes(line)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn request_bytes(&self, line: &str) -> io::Result<Vec<u8>> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        writeln!(stream, "{line}")?;
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "proxy closed connection without replying",
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
            Ok(payload)
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
