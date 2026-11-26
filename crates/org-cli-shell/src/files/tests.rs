use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use super::{read_bounded_into, read_regular, read_regular_into, ReadRefusal};

fn mkfifo(path: &Path) {
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success(), "mkfifo {}", path.display());
}

#[test]
fn a_regular_file_is_read_up_to_its_limit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("list.json");
    fs::write(&path, b"12345").unwrap();
    assert_eq!(read_regular(&path, 5).unwrap(), b"12345");
    assert!(matches!(
        read_regular(&path, 4),
        Err(ReadRefusal::TooLarge { limit: 4 })
    ));
    fs::write(&path, b"").unwrap();
    assert_eq!(read_regular(&path, 0).unwrap(), b"");
}

/// Symbolic links (to a file, dangling, looping, with a trailing slash), a directory and a
/// FIFO are refused; the FIFO is refused without blocking.
#[test]
fn links_directories_and_fifos_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target.json");
    fs::write(&target, b"[]").unwrap();
    let link = root.path().join("link.json");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let dangling = root.path().join("dangling.json");
    std::os::unix::fs::symlink(root.path().join("missing"), &dangling).unwrap();
    let looping = root.path().join("loop.json");
    std::os::unix::fs::symlink(&looping, &looping).unwrap();
    let fifo = root.path().join("fifo.json");
    mkfifo(&fifo);
    for path in [
        link.clone(),
        dangling,
        looping,
        root.path().to_path_buf(),
        fifo,
    ] {
        let started = Instant::now();
        let refused = read_regular(&path, 1024);
        assert!(
            matches!(refused, Err(ReadRefusal::NotRegular)),
            "{}: {refused:?}",
            path.display()
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }
    // `link.json/` names the link's target as a directory: never read as the file.
    let slashed = Path::new(&format!("{}/", link.display())).to_path_buf();
    assert!(read_regular(&slashed, 1024).is_err());
}

#[test]
fn a_missing_file_is_an_io_error() {
    let root = tempfile::tempdir().unwrap();
    let missing = read_regular(&root.path().join("missing"), 1).unwrap_err();
    assert!(
        matches!(&missing, ReadRefusal::Io(error) if error.kind() == std::io::ErrorKind::NotFound),
        "{missing:?}"
    );
    assert_eq!(ReadRefusal::NotRegular.to_string(), "not a regular file");
    assert_eq!(
        ReadRefusal::TooLarge { limit: 9 }.to_string(),
        "larger than 9 bytes"
    );
}

/// A reader that never ends, interrupted now and then.
struct Endless(u32);

impl std::io::Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0 += 1;
        if self.0.is_multiple_of(3) {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let n = buf.len().min(7);
        buf[..n].fill(b'x');
        Ok(n)
    }
}

/// An endless stream is cut at the bound and refused; the buffer then holds exactly the bytes
/// read (`limit + 1`), in the one allocation made up front, for the caller to zero.
#[test]
fn an_endless_stream_stops_at_the_bound_in_one_buffer() {
    let mut out = Vec::new();
    let refused = read_bounded_into(&mut Endless(0), 20, &mut out);
    assert!(matches!(refused, Err(ReadRefusal::TooLarge { limit: 20 })));
    assert_eq!(out, vec![b'x'; 21]);
    let mut out = vec![b'?'; 4];
    read_bounded_into(&mut std::io::Cursor::new(b"abc".to_vec()), 3, &mut out).unwrap();
    assert_eq!(out, b"abc", "the caller's old bytes are gone");
}

/// A limit with no room for its extra byte, or one no buffer can hold, is refused as invalid
/// input before anything is read or allocated: it never panics.
#[test]
fn an_unbounded_limit_is_refused_without_a_panic() {
    for limit in [u64::MAX, u64::MAX - 1] {
        let mut out = vec![b'?'; 2];
        let refused = read_bounded_into(&mut Endless(0), limit, &mut out);
        assert!(
            matches!(&refused, Err(ReadRefusal::Io(error)) if error.kind() == std::io::ErrorKind::InvalidInput),
            "{limit}: {refused:?}"
        );
    }
}

#[test]
fn a_refused_file_leaves_only_what_was_read() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("big");
    fs::write(&path, b"0123456789").unwrap();
    let mut out = Vec::new();
    assert!(read_regular_into(&path, 4, &mut out).is_err());
    assert_eq!(out, b"01234");
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(matches!(
        read_regular_into(&link, 64, &mut out),
        Err(ReadRefusal::NotRegular)
    ));
    assert!(out.is_empty());
    let nul = Path::new("bad\0name");
    assert!(matches!(read_regular(nul, 1), Err(ReadRefusal::Io(_))));
}
