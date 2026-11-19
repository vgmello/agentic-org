use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_CAPACITY: usize = 128 * 1024;

struct BufferState {
    bytes: VecDeque<u8>,
    capacity: usize,
    total_written: u64,
    closed: bool,
}

/// Thread-safe bounded rolling capture of PTY output with wait-for-text support.
pub struct OutputBuffer {
    state: Mutex<BufferState>,
    changed: Condvar,
}

impl Default for OutputBuffer {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl OutputBuffer {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            state: Mutex::new(BufferState {
                bytes: VecDeque::with_capacity(capacity.min(4096)),
                capacity: capacity.max(1024),
                total_written: 0,
                closed: false,
            }),
            changed: Condvar::new(),
        }
    }

    /// Appends raw bytes emitted by the PTY child.
    pub fn push(&self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let mut guard = self.state.lock().expect("output buffer lock poisoned");
        let cap = guard.capacity;
        if chunk.len() >= cap {
            guard.bytes.clear();
            guard.bytes.extend(&chunk[chunk.len() - cap..]);
        } else {
            let overflow = guard
                .bytes
                .len()
                .saturating_add(chunk.len())
                .saturating_sub(cap);
            for _ in 0..overflow {
                guard.bytes.pop_front();
            }
            guard.bytes.extend(chunk);
        }
        guard.total_written = guard.total_written.saturating_add(chunk.len() as u64);
        self.changed.notify_all();
    }

    /// Marks the PTY output stream as closed so waiting readers do not hang.
    pub fn mark_closed(&self) {
        let mut guard = self.state.lock().expect("output buffer lock poisoned");
        guard.closed = true;
        self.changed.notify_all();
    }

    /// Returns a snapshot of the raw captured bytes.
    pub fn snapshot_raw(&self) -> Vec<u8> {
        let guard = self.state.lock().expect("output buffer lock poisoned");
        guard.bytes.iter().copied().collect()
    }

    /// Returns a snapshot with ANSI escape sequences stripped for readable inspection.
    pub fn snapshot_plain(&self) -> String {
        let raw = self.snapshot_raw();
        strip_ansi(&raw)
    }

    /// Waits until `needle` appears in the ANSI-stripped rolling output or `timeout` elapses.
    pub fn wait_for(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut guard = self.state.lock().expect("output buffer lock poisoned");
        loop {
            let raw: Vec<u8> = guard.bytes.iter().copied().collect();
            if strip_ansi(&raw).contains(needle) {
                return true;
            }
            if guard.closed {
                return false;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (next_guard, wait_res) = self
                .changed
                .wait_timeout(guard, deadline - now)
                .expect("output buffer condvar poisoned");
            guard = next_guard;
            if wait_res.timed_out() {
                let raw: Vec<u8> = guard.bytes.iter().copied().collect();
                return strip_ansi(&raw).contains(needle);
            }
        }
    }

    /// Waits until the child has produced at least one byte of output (or `timeout` elapses).
    pub fn wait_for_any_output(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut guard = self.state.lock().expect("output buffer lock poisoned");
        while guard.total_written == 0 && !guard.closed {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (next_guard, _) = self
                .changed
                .wait_timeout(guard, deadline - now)
                .expect("output buffer condvar poisoned");
            guard = next_guard;
        }
        guard.total_written > 0
    }
}

/// Strips common ANSI CSI, OSC, and 2-byte ESC sequences into readable UTF-8 text.
pub fn strip_ansi(input: &[u8]) -> String {
    let text = String::from_utf8_lossy(input);
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            i += 1;
            if i >= bytes.len() {
                break;
            }
            match bytes[i] {
                // CSI sequence: ESC [ ... final byte 0x40..=0x7E
                b'[' => {
                    i += 1;
                    while i < bytes.len() {
                        let b = bytes[i];
                        i += 1;
                        if (0x40..=0x7e).contains(&b) {
                            break;
                        }
                    }
                }
                // OSC sequence: ESC ] ... BEL (0x07) or ST (ESC \)
                b']' => {
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                // Charset designation sequences like ESC ( B
                b'(' | b')' | b'*' | b'+' => {
                    i = (i + 2).min(bytes.len());
                }
                // Single-character ESC sequence (e.g. ESC =, ESC >)
                _ => {
                    i += 1;
                }
            }
        } else {
            let b = bytes[i];
            // Keep printable ASCII, UTF-8 multibyte bytes, newlines, tabs, and convert CR to newline spacing.
            if b == b'\r' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    // CRLF -> skip CR, keep LF
                } else {
                    out.push(b'\n');
                }
            } else if b == b'\n' || b == b'\t' || b >= 0x20 {
                out.push(b);
            }
            i += 1;
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_and_osc_sequences() {
        let raw = b"\x1b]0;vi demo.txt\x07\x1b[2J\x1b[HHello, \x1b[32mworld\x1b[0m!\r\n";
        assert_eq!(strip_ansi(raw), "Hello, world!\n");
    }

    #[test]
    fn rolling_buffer_evicts_oldest_bytes() {
        let buf = OutputBuffer::with_capacity(1024);
        buf.push(&vec![b'a'; 900]);
        buf.push(&vec![b'b'; 300]);
        let snap = buf.snapshot_raw();
        assert_eq!(snap.len(), 1024);
        assert!(snap.ends_with(&vec![b'b'; 300]));
    }
}
