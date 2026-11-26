//! The terminal: escaping, closed pipes, which streams are terminals and how wide, and Ctrl+C.

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

pub use crate::escape::{escape_json_controls, sanitize, sanitize_lines};

use crate::error::{ShellError, EXIT_CANCELLED};

/// True when the reader of the stream went away (`agentic-org org ls | head -1`).
pub fn is_broken_pipe(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::BrokenPipe
}

/// Writes all of `bytes` to `w` and flushes. A closed reader is [`ShellError::OutputClosed`]
/// (exit 0, nothing printed); any other failure is [`ShellError::Io`].
pub fn write_out(w: &mut dyn Write, bytes: &[u8]) -> Result<(), ShellError> {
    w.write_all(bytes).and_then(|()| w.flush()).map_err(|e| {
        if is_broken_pipe(&e) {
            ShellError::OutputClosed
        } else {
            ShellError::io("writing the output", e)
        }
    })
}

/// Which standard streams are terminals, and the terminal's width in columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Terminal {
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
    /// The width of the terminal on standard output, when it is one.
    pub width: Option<usize>,
}

impl Terminal {
    /// The process's real streams.
    pub fn detect() -> Self {
        Self {
            stdin_tty: io::stdin().is_terminal(),
            stdout_tty: io::stdout().is_terminal(),
            stderr_tty: io::stderr().is_terminal(),
            width: terminal_size::terminal_size()
                .map(|(width, _)| usize::from(width.0))
                .filter(|width| *width > 0),
        }
    }

    /// No terminal anywhere (a script, a pipe, a test).
    pub fn piped() -> Self {
        Self::default()
    }
}

/// Shows the cursor again (a prompt or spinner may have hidden it).
const SHOW_CURSOR: &[u8] = b"\x1b[?25h";

static INTERRUPT_HANDLER: AtomicBool = AtomicBool::new(false);

/// Installs the Ctrl+C handler: restore the cursor when standard error is a terminal (best
/// effort), then exit with 130. A second call does nothing, and a failure to install (another
/// handler already owns the signal) is ignored.
///
/// The consumer must NOT call this for `agentic-org serve`: the server blocks and waits for
/// its own shutdown signals, and a password prompt that blocks signals relies on them staying
/// blocked. Prompts themselves read Ctrl+C as a key and return [`ShellError::Cancelled`].
pub fn install_interrupt_handler() {
    if INTERRUPT_HANDLER.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = ctrlc::set_handler(on_interrupt);
}

fn on_interrupt() {
    let mut stderr = io::stderr();
    if stderr.is_terminal() {
        let _ = stderr.write_all(SHOW_CURSOR);
        let _ = stderr.flush();
    }
    std::process::exit(i32::from(EXIT_CANCELLED));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream that fails every write with `kind`.
    struct Failing(io::ErrorKind);

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(self.0.into())
        }
    }

    /// Accepts writes but fails to flush with a closed pipe.
    struct FlushClosed(Vec<u8>);

    impl Write for FlushClosed {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn write_out_writes_everything() {
        let mut buffer = Vec::new();
        write_out(&mut buffer, b"one\ntwo\n").unwrap();
        assert_eq!(buffer, b"one\ntwo\n");
    }

    #[test]
    fn broken_pipe_is_output_closed() {
        let error = write_out(&mut Failing(io::ErrorKind::BrokenPipe), b"x").unwrap_err();
        assert!(matches!(error, ShellError::OutputClosed), "{error:?}");
        let error = write_out(&mut FlushClosed(Vec::new()), b"x").unwrap_err();
        assert!(matches!(error, ShellError::OutputClosed), "{error:?}");
    }

    #[test]
    fn other_errors_are_io() {
        let error = write_out(&mut Failing(io::ErrorKind::PermissionDenied), b"x").unwrap_err();
        match error {
            ShellError::Io { source, .. } => {
                assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
            }
            other => panic!("{other:?}"),
        }
        assert!(is_broken_pipe(&io::ErrorKind::BrokenPipe.into()));
        assert!(!is_broken_pipe(&io::ErrorKind::Interrupted.into()));
    }

    #[test]
    fn piped_is_no_terminal() {
        assert_eq!(
            Terminal::piped(),
            Terminal {
                stdin_tty: false,
                stdout_tty: false,
                stderr_tty: false,
                width: None,
            }
        );
        // Under the test harness the streams are whatever runs it; detection must not fail.
        let detected = Terminal::detect();
        assert!(detected.width.is_none_or(|width| width > 0));
    }

    #[test]
    fn interrupt_handler_installs_once() {
        install_interrupt_handler();
        assert!(INTERRUPT_HANDLER.load(Ordering::SeqCst));
        install_interrupt_handler();
    }

    #[test]
    fn reexported_escapes() {
        assert_eq!(sanitize("\x1b"), "\\u{1b}");
        assert_eq!(escape_json_controls("\"\u{85}\"".into()), "\"\\u0085\"");
    }
}
