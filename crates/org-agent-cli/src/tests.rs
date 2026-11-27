//! The crate root: what `emit` answers when standard output fails.

use std::io::{self, Write};

use super::emit;

struct Failing(io::ErrorKind);

impl Write for Failing {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(self.0))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_closed_pipe_keeps_the_exit_code_and_any_other_write_failure_exits_1() {
    let mut written = Vec::new();
    assert_eq!(emit(&mut written, "{}\n", 2), 2);
    assert_eq!(written, b"{}\n");
    assert_eq!(emit(&mut Failing(io::ErrorKind::BrokenPipe), "x", 0), 0);
    assert_eq!(emit(&mut Failing(io::ErrorKind::BrokenPipe), "x", 1), 1);
    assert_eq!(emit(&mut Failing(io::ErrorKind::StorageFull), "x", 0), 1);
    assert_eq!(emit(&mut Failing(io::ErrorKind::Other), "x", 0), 1);
}
