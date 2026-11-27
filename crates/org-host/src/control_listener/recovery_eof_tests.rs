use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;

use crate::peer::Accepted;

use super::connection::Connection;

#[test]
fn inline_presence_wait_observes_peer_eof_without_reading_or_authorizing_pipelined_bytes() {
    use std::io::{Read, Write};
    let (stream, mut peer) = UnixStream::pair().unwrap();
    let (connection, _queue) =
        Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
    assert!(connection.recovery_peer_idle());
    peer.write_all(b"x").unwrap();
    assert!(!connection.recovery_peer_idle());
    // Peek neither swallows the following request nor changes shared flags.
    let mut read = stream;
    let mut byte = [0];
    read.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [b'x']);
    assert!(connection.recovery_peer_idle());
    peer.shutdown(std::net::Shutdown::Write).unwrap();
    assert!(!connection.recovery_peer_idle());
    assert!(!connection.closed.load(Ordering::SeqCst));
}
