//! A small buffered reader that wipes its buffer when it is dropped. The first line from the host
//! carries the app token, and the launch contract asks the server to read it "unbuffered or
//! through a small buffer you control": std's `BufReader` frees its buffer without clearing it.
//! Best effort, like the rest of the zeroing (copies elsewhere remain).
#![allow(unsafe_code)]

use std::fs::File;
use std::io::{self, BufRead, Read};

const SIZE: usize = 1024;

pub struct ZeroedReader {
    inner: Option<File>,
    buf: [u8; SIZE],
    pos: usize,
    cap: usize,
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

    /// Gives the file back (what is still buffered is wiped).
    pub fn into_inner(mut self) -> File {
        self.wipe();
        self.inner.take().expect("the file is only taken here")
    }

    /// Overwrites the buffer with zeros (volatile writes, so they are not optimized away).
    pub fn wipe(&mut self) {
        for byte in &mut self.buf {
            // SAFETY: `byte` is a valid, aligned, exclusive reference into the array.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
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
                .expect("the file is only taken by into_inner");
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
    use std::io::Write;

    use super::*;

    #[test]
    fn it_reads_lines_and_wipes_its_buffer() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"first line with a secret\nsecond\n")
            .unwrap();
        std::io::Seek::rewind(&mut file).unwrap();
        let mut reader = ZeroedReader::new(file);

        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "first line with a secret\n");
        assert!(!reader.buffer_is_zero(), "the secret is in the buffer");

        reader.wipe();
        assert!(reader.buffer_is_zero());
        let _file = reader.into_inner();
    }
}
