//! macOS listener state, including sockets that were connected before listening.
use super::super::invalid;
#[cfg(test)]
use super::tests::proc_pidfdinfo;
#[cfg(not(test))]
use libc::proc_pidfdinfo;
use std::io;
use std::mem::{align_of, offset_of, size_of};
use std::os::fd::RawFd;

// SDK 27.2 C probe (<sys/proc_info.h>, sizeof/_Alignof/offsetof):
// socket_fdinfo_size=792
// socket_fdinfo_align=8
// soi_options_offset=188
// soi_options_size=2
// PROC_PIDFDSOCKETINFO=3
// SO_ACCEPTCONN=2
#[repr(C, align(8))]
struct SocketInfo {
    prefix: [u8; 188],
    options: i16,
    suffix: [u8; 602],
}
const _: () = assert!(
    size_of::<SocketInfo>() == 792
        && align_of::<SocketInfo>() == 8
        && offset_of!(SocketInfo, options) == 188
);

pub(super) fn listening(fd: RawFd) -> io::Result<bool> {
    let mut info = SocketInfo {
        prefix: [0; 188],
        options: 0,
        suffix: [0; 602],
    };
    // SAFETY: fd is borrowed from an owned descriptor; the exact, aligned SDK
    // layout is initialized, writable for its full size, and lives through this call.
    let count = unsafe {
        proc_pidfdinfo(
            libc::getpid(),
            fd,
            3, /* PROC_PIDFDSOCKETINFO, SDK probe */
            (&mut info as *mut SocketInfo).cast(),
            size_of::<SocketInfo>() as _,
        )
    };
    if count != size_of::<SocketInfo>() as i32 {
        return Err(invalid("incomplete channel socket information"));
    }
    Ok(i32::from(info.options) & libc::SO_ACCEPTCONN != 0)
}
