//! The parts of `<sys/proc_info.h>` the `libc` crate doesn't declare, for `peer_fds`.
//!
//! `libc` already has `proc_pidinfo`, `proc_pidfdinfo`, `proc_fdinfo`, `vinfo_stat`,
//! `PROC_PIDLISTFDS` and `PROX_FDTYPE_SOCKET`, and those are used from there. Everything here is
//! copied from the macOS SDK header; the test below compiles a C probe against the SDK and checks
//! every size, offset and constant the scan relies on, so a header that changes fails the build's
//! tests instead of the scan reading the wrong bytes.
#![allow(non_camel_case_types, dead_code)]

use libc::{c_char, c_int, off_t, vinfo_stat};

/// The `proc_pidfdinfo` flavor that fills a [`socket_fdinfo`].
pub(super) const PROC_PIDFDSOCKETINFO: c_int = 3;
/// `socket_info::soi_kind` of a Unix-domain socket: `soi_proto.pri_un` is the member filled in.
pub(super) const SOCKINFO_UN: c_int = 3;
/// `proc_fileinfo::fi_status`: the open file is referenced more than once (by another fd, in this
/// or any process, or by a descriptor in flight).
pub(super) const PROC_FP_SHARED: u32 = 1;
/// `proc_fileinfo::fi_status`: the fd is close-on-exec.
pub(super) const PROC_FP_CLEXEC: u32 = 2;

const SOCK_MAXADDRLEN: usize = 255;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct proc_fileinfo {
    fi_openflags: u32,
    pub(super) fi_status: u32,
    fi_offset: off_t,
    fi_type: i32,
    fi_guardflags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct sockbuf_info {
    sbi_cc: u32,
    sbi_hiwat: u32,
    sbi_mbcnt: u32,
    sbi_mbmax: u32,
    sbi_lowat: u32,
    sbi_flags: i16,
    sbi_timeo: i16,
}

/// `un_sockinfo`. Its two address unions (`sockaddr_un` or `char[SOCK_MAXADDRLEN]`) are
/// byte-aligned and the char array is their larger member, so a plain array has their layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct un_sockinfo {
    pub(super) unsi_conn_so: u64,
    pub(super) unsi_conn_pcb: u64,
    unsi_addr: [c_char; SOCK_MAXADDRLEN],
    unsi_caddr: [c_char; SOCK_MAXADDRLEN],
}

/// `socket_info::soi_proto`. Only the Unix-domain member is ever read. It is also the SDK union's
/// largest member (528 bytes; TCP's is 120), so this one-member union has the same size and
/// alignment (checked by the test below).
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union soi_proto {
    pub(super) pri_un: un_sockinfo,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct socket_info {
    soi_stat: vinfo_stat,
    pub(super) soi_so: u64,
    pub(super) soi_pcb: u64,
    pub(super) soi_type: c_int,
    soi_protocol: c_int,
    soi_family: c_int,
    soi_options: i16,
    soi_linger: i16,
    soi_state: i16,
    soi_qlen: i16,
    soi_incqlen: i16,
    soi_qlimit: i16,
    soi_timeo: i16,
    soi_error: u16,
    soi_oobmark: u32,
    soi_rcv: sockbuf_info,
    soi_snd: sockbuf_info,
    pub(super) soi_kind: c_int,
    rfu_1: u32,
    pub(super) soi_proto: soi_proto,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct socket_fdinfo {
    pub(super) pfi: proc_fileinfo,
    pub(super) psi: socket_info,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::mem::{align_of, offset_of, size_of};
    use std::process::Command;

    /// Prints every size, offset and constant the Rust side relies on, as the SDK has them.
    const PROBE: &str = r#"
#include <stdio.h>
#include <stddef.h>
#include <libproc.h>
#include <sys/proc_info.h>
#include <sys/socket.h>
#define S(t) printf("sizeof_" #t " %zu\n", sizeof(struct t));
#define O(t, f) printf("offsetof_" #t "_" #f " %zu\n", offsetof(struct t, f));
#define C(c) printf("const_" #c " %lld\n", (long long)(c));
int main(void) {
    S(proc_fileinfo) O(proc_fileinfo, fi_status)
    S(sockbuf_info)
    S(un_sockinfo) O(un_sockinfo, unsi_conn_so) O(un_sockinfo, unsi_conn_pcb)
    S(socket_info) O(socket_info, soi_so) O(socket_info, soi_pcb) O(socket_info, soi_type)
    O(socket_info, soi_kind) O(socket_info, soi_proto)
    printf("sizeof_soi_proto %zu\n", sizeof(((struct socket_info *)0)->soi_proto));
    S(socket_fdinfo) O(socket_fdinfo, pfi) O(socket_fdinfo, psi)
    printf("alignof_socket_fdinfo %zu\n", _Alignof(struct socket_fdinfo));
    S(vinfo_stat) S(proc_fdinfo)
    C(PROC_PIDFDSOCKETINFO) C(SOCKINFO_UN) C(PROC_FP_SHARED) C(PROC_FP_CLEXEC)
    C(PROC_PIDLISTFDS) C(PROX_FDTYPE_SOCKET) C(SOCK_STREAM)
    return 0;
}
"#;

    /// Compiles [`PROBE`] with the system C compiler (`cc`, the Xcode command line tools every
    /// development Mac has) in a temporary folder, removed even if an assertion fails, and
    /// compares each declared layout (and the `libc` ones the scan reuses) with the SDK's.
    #[test]
    fn the_declared_layouts_match_the_sdk() {
        let dir = tempfile::tempdir().unwrap();
        let (src, exe) = (dir.path().join("probe.c"), dir.path().join("probe"));
        std::fs::write(&src, PROBE).unwrap();
        let built = Command::new("cc")
            .arg("-o")
            .arg(&exe)
            .arg(&src)
            .output()
            .unwrap_or_else(|e| {
                panic!(
                    "couldn't run `cc` to check peer_fds's libproc layouts against the SDK \
                     ({e}); install the Xcode command line tools (`xcode-select --install`)"
                )
            });
        assert!(
            built.status.success(),
            "`cc` couldn't build the layout probe: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let out = Command::new(&exe).output().unwrap();
        assert!(out.status.success(), "the layout probe failed");
        let sdk: HashMap<String, i64> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|line| {
                let (name, value) = line.split_once(' ').unwrap();
                (name.to_owned(), value.parse().unwrap())
            })
            .collect();

        let ours: &[(&str, usize)] = &[
            ("sizeof_proc_fileinfo", size_of::<proc_fileinfo>()),
            (
                "offsetof_proc_fileinfo_fi_status",
                offset_of!(proc_fileinfo, fi_status),
            ),
            ("sizeof_sockbuf_info", size_of::<sockbuf_info>()),
            ("sizeof_un_sockinfo", size_of::<un_sockinfo>()),
            (
                "offsetof_un_sockinfo_unsi_conn_so",
                offset_of!(un_sockinfo, unsi_conn_so),
            ),
            (
                "offsetof_un_sockinfo_unsi_conn_pcb",
                offset_of!(un_sockinfo, unsi_conn_pcb),
            ),
            ("sizeof_socket_info", size_of::<socket_info>()),
            (
                "offsetof_socket_info_soi_so",
                offset_of!(socket_info, soi_so),
            ),
            (
                "offsetof_socket_info_soi_pcb",
                offset_of!(socket_info, soi_pcb),
            ),
            (
                "offsetof_socket_info_soi_type",
                offset_of!(socket_info, soi_type),
            ),
            (
                "offsetof_socket_info_soi_kind",
                offset_of!(socket_info, soi_kind),
            ),
            (
                "offsetof_socket_info_soi_proto",
                offset_of!(socket_info, soi_proto),
            ),
            ("sizeof_soi_proto", size_of::<soi_proto>()),
            ("sizeof_socket_fdinfo", size_of::<socket_fdinfo>()),
            ("offsetof_socket_fdinfo_pfi", offset_of!(socket_fdinfo, pfi)),
            ("offsetof_socket_fdinfo_psi", offset_of!(socket_fdinfo, psi)),
            ("alignof_socket_fdinfo", align_of::<socket_fdinfo>()),
            ("sizeof_vinfo_stat", size_of::<vinfo_stat>()),
            ("sizeof_proc_fdinfo", size_of::<libc::proc_fdinfo>()),
        ];
        let constants: &[(&str, i64)] = &[
            ("const_PROC_PIDFDSOCKETINFO", PROC_PIDFDSOCKETINFO.into()),
            ("const_SOCKINFO_UN", SOCKINFO_UN.into()),
            ("const_PROC_FP_SHARED", PROC_FP_SHARED.into()),
            ("const_PROC_FP_CLEXEC", PROC_FP_CLEXEC.into()),
            ("const_PROC_PIDLISTFDS", libc::PROC_PIDLISTFDS.into()),
            ("const_PROX_FDTYPE_SOCKET", libc::PROX_FDTYPE_SOCKET.into()),
            ("const_SOCK_STREAM", libc::SOCK_STREAM.into()),
        ];
        assert_eq!(
            ours.len() + constants.len(),
            sdk.len(),
            "the probe and the table list different items"
        );
        for (name, value) in ours {
            assert_eq!(
                sdk.get(*name),
                Some(&(*value as i64)),
                "{name}: SDK vs Rust"
            );
        }
        for (name, value) in constants {
            assert_eq!(sdk.get(*name), Some(value), "{name}: SDK vs Rust");
        }
    }
}
