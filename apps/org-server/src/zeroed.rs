//! A small buffered reader that wipes its buffer when it is dropped. The first line from the host
//! carries the app token and the database key, and the launch contract asks the server to read it "unbuffered or
//! through a small buffer you control": std's `BufReader` frees its buffer without clearing it.
//! Best effort, like the rest of the zeroing (copies elsewhere remain).
#![allow(unsafe_code)]

use std::fs::File;
use std::io::{self, BufRead, Read};

const SIZE: usize = 1024;

#[cfg(test)]
thread_local! {
    /// How many times a buffer was wiped on this thread (tests count it, to know `Drop` wipes).
    static WIPES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub struct ZeroedReader {
    inner: Option<File>,
    buf: [u8; SIZE],
    pos: usize,
    cap: usize,
}

/// Fixed-size handoff storage, wiped even when decoding or reception fails.
pub(crate) struct SecretBuffer(pub Box<[u8]>);

impl Drop for SecretBuffer {
    fn drop(&mut self) {
        wipe_bytes(&mut self.0);
    }
}

fn wipe_bytes(bytes: &mut [u8]) {
    #[cfg(test)]
    WIPES.with(|w| w.set(w.get() + 1));
    for byte in bytes {
        // SAFETY: `byte` is a valid, aligned, exclusive reference into the buffer.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

impl ZeroedReader {
    pub fn new(inner: File) -> Self {
        Self {
            inner: Some(inner),
            buf: [0; SIZE],
            pos: 0,
            cap: 0,
        }
    }

    /// Wipes the buffer IN PLACE and gives the file back. It takes `&mut self` on purpose: a
    /// by-value `self` would be moved, leaving the token in the caller's old stack slot.
    pub fn take_inner(&mut self) -> File {
        self.wipe();
        self.inner.take().expect("the file is only taken here")
    }

    /// Overwrites the buffer with zeros (volatile writes, so they are not optimized away).
    pub fn wipe(&mut self) {
        wipe_bytes(&mut self.buf);
        self.pos = 0;
        self.cap = 0;
    }

    #[cfg(test)]
    fn buffer_is_zero(&self) -> bool {
        self.buf.iter().all(|&b| b == 0)
    }
}

impl Read for ZeroedReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(out.len());
        out[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for ZeroedReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.pos >= self.cap {
            let file = self
                .inner
                .as_mut()
                .expect("the file is only taken by take_inner");
            self.cap = file.read(&mut self.buf)?;
            self.pos = 0;
        }
        Ok(&self.buf[self.pos..self.cap])
    }

    fn consume(&mut self, amount: usize) {
        self.pos = (self.pos + amount).min(self.cap);
    }
}

impl Drop for ZeroedReader {
    fn drop(&mut self) {
        self.wipe();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn the_shared_wipe_overwrites_every_byte() {
        let mut secret = [0xb3; 4096];
        wipe_bytes(&mut secret);
        assert!(secret.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn secret_storage_is_wiped_on_success_error_and_unwind() {
        for outcome in 0..3 {
            let before = WIPES.with(std::cell::Cell::get);
            let result = std::panic::catch_unwind(|| {
                let _buffer = SecretBuffer(vec![0xb3; 4096].into_boxed_slice());
                match outcome {
                    0 => Ok(()),
                    1 => Err(()),
                    _ => panic!("synthetic failure"),
                }
            });
            assert_eq!(WIPES.with(std::cell::Cell::get), before + 1);
            assert_eq!(result.is_err(), outcome == 2);
        }
    }

    fn reader_after_one_line() -> ZeroedReader {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"first line with a secret\nsecond\n")
            .unwrap();
        std::io::Seek::rewind(&mut file).unwrap();
        let mut reader = ZeroedReader::new(file);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "first line with a secret\n");
        assert!(!reader.buffer_is_zero(), "the secret is in the buffer");
        reader
    }

    #[test]
    fn it_reads_lines_and_a_line_longer_than_its_buffer() {
        let mut file = tempfile::tempfile().unwrap();
        let long = "x".repeat(3000);
        file.write_all(format!("{long}\nshort\n").as_bytes())
            .unwrap();
        std::io::Seek::rewind(&mut file).unwrap();
        let mut reader = ZeroedReader::new(file);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, format!("{long}\n"));
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "short\n");
        line.clear();
        assert_eq!(reader.read_line(&mut line).unwrap(), 0, "end of file");
    }

    #[test]
    fn taking_the_file_wipes_the_buffer_in_place() {
        let mut reader = reader_after_one_line();
        let _file = reader.take_inner();
        assert!(reader.buffer_is_zero());
    }

    #[test]
    fn dropping_the_reader_wipes_its_buffer() {
        let reader = reader_after_one_line();
        let before = WIPES.with(std::cell::Cell::get);
        drop(reader);
        assert_eq!(
            WIPES.with(std::cell::Cell::get),
            before + 1,
            "Drop did not wipe"
        );
    }
}
