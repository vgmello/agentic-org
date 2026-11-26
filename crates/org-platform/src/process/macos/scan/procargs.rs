//! A process's launch image (`KERN_PROCARGS2`): whether code signing lets it be read, and
//! the recovery mark its environment carries.

use crate::process::{native_mark, MarkEvidence};

// XNU sys/codesign.h and kern/cs_blobs.h. CS_RESTRICT controls PROCARGS2 redaction.
const CS_OPS_STATUS: libc::c_uint = 0;
pub(super) const CS_RESTRICT: u32 = 0x0000_0800;

unsafe extern "C" {
    fn csops(
        pid: libc::pid_t,
        ops: libc::c_uint,
        address: *mut libc::c_void,
        size: usize,
    ) -> libc::c_int;
}

pub(super) fn unrestricted(pid: libc::pid_t) -> bool {
    let mut flags = 0u32;
    // SAFETY: CS_OPS_STATUS writes one u32 to the correctly sized output slot.
    let status = unsafe {
        csops(
            pid,
            CS_OPS_STATUS,
            (&mut flags as *mut u32).cast(),
            std::mem::size_of_val(&flags),
        )
    };
    code_signing_allows(status, flags)
}

pub(super) fn code_signing_allows(status: libc::c_int, flags: u32) -> bool {
    status == 0 && flags & CS_RESTRICT == 0
}

pub(super) fn procargs_mark(
    bytes: &[u8],
    expected_size: usize,
    mark_prefix: &[u8],
) -> MarkEvidence {
    if bytes.len() != expected_size {
        return MarkEvidence::Unreadable;
    }
    let Some((header, payload)) = bytes.split_at_checked(std::mem::size_of::<i32>()) else {
        return MarkEvidence::Unreadable;
    };
    let argc = i32::from_ne_bytes(header.try_into().unwrap());
    if argc <= 0 || !payload.len().is_multiple_of(8) || payload.last() != Some(&0) {
        return MarkEvidence::Unreadable;
    }
    let Some(path_end) = payload.iter().position(|b| *b == 0).filter(|end| *end > 0) else {
        return MarkEvidence::Unreadable;
    };
    // XNU aligns the argument vector strings after the saved executable path.
    // PROCARGS2 strips a 16-byte executable_path= prefix, preserving alignment.
    // Skip just that padding. XNU may partially redact captures with empty
    // argv[0]. Subsequent empty arguments still count toward argc.
    let args_start = (path_end + 1).next_multiple_of(8);
    let Some(padding) = payload.get(path_end..args_start) else {
        return MarkEvidence::Unreadable;
    };
    if padding.iter().any(|b| *b != 0) {
        return MarkEvidence::Unreadable;
    }
    let mut rest = &payload[args_start..];
    if rest.first().is_none_or(|byte| *byte == 0) {
        return MarkEvidence::Unreadable;
    }
    for _ in 0..argc {
        let Some(end) = rest.iter().position(|b| *b == 0) else {
            return MarkEvidence::Unreadable;
        };
        rest = &rest[end + 1..];
    }
    // A complete launch image includes strings after argv, even with an empty env.
    if rest.iter().all(|b| *b == 0) {
        return MarkEvidence::Unreadable;
    }
    // The kernel supplies the launch argc and reads the current stack strings.
    // Deliberate same-UID argv mutation can forge the boundary if the process
    // already knows the exact mark. Recovery trusts cooperating same-UID code.
    native_mark(rest, mark_prefix)
}
