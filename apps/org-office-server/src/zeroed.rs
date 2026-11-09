//! A small buffered reader that wipes its buffer when it is dropped. The key hand-off line carries
//! the app token and the database key; the launch contract asks the server to read it "unbuffered or
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

pub struct ZeroedReader<R = File> {
    inner: Option<R>,
    buf: [u8; SIZE],
    pos: usize,
    cap: usize,
}

impl<R> ZeroedReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner: Some(inner),
            buf: [0; SIZE],
            pos: 0,
            cap: 0,
        }
    }

    /// Wipes the buffer IN PLACE and gives the file back. It takes `&mut self` on purpose: a
    /// by-value `self` would be moved, leaving the token in the caller's old stack slot.
    pub fn take_inner(&mut self) -> R {
        self.wipe();
        self.inner.take().expect("the file is only taken here")
    }

    /// Overwrites the buffer with zeros (volatile writes, so they are not optimized away).
    pub fn wipe(&mut self) {
        #[cfg(test)]
        WIPES.with(|w| w.set(w.get() + 1));
        for byte in &mut self.buf {
            // SAFETY: `byte` is a valid, aligned, exclusive reference into the array.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
        self.pos = 0;
        self.cap = 0;
    }

    #[cfg(test)]
    fn buffer_is_zero(&self) -> bool {
        self.buf.iter().all(|&b| b == 0)
    }
}

impl<R: Read> Read for ZeroedReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(out.len());
        out[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl<R: Read> BufRead for ZeroedReader<R> {
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

impl<R> Drop for ZeroedReader<R> {
    fn drop(&mut self) {
        self.wipe();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
