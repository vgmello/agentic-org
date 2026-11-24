//! Bounded `Sources` implementation for `--input-file` and `--stdin`.

use std::fs::File;
use std::io::{self, Read};

use org_command::Sources;

/// Standard filesystem and stdin reader honoring `org-command`'s `max_bytes + 1` bound.
#[derive(Debug, Default)]
pub struct StdSources;

fn read_bounded(reader: impl Read, max_bytes: usize) -> io::Result<Vec<u8>> {
    let cap = (max_bytes as u64).saturating_add(1);
    let mut out = Vec::new();
    reader.take(cap).read_to_end(&mut out)?;
    Ok(out)
}

impl Sources for StdSources {
    fn file(&mut self, path: &str, max_bytes: usize) -> io::Result<Vec<u8>> {
        let file = File::open(path)?;
        read_bounded(file, max_bytes)
    }

    fn stdin(&mut self, max_bytes: usize) -> io::Result<Vec<u8>> {
        read_bounded(io::stdin().lock(), max_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn bounded_read_stops_at_max_plus_one() {
        let bytes = vec![b'a'; 100];
        let got = read_bounded(Cursor::new(&bytes), 16).unwrap();
        assert_eq!(got.len(), 17);
    }
}
