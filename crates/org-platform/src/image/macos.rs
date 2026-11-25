//! The macOS image adapter: the kernel's CodeDirectory hash of a running image, read
//! with `csops` and `csops_audittoken` (the C library only, no framework).

use super::{refused, OwnExecutable, ReleasePlatform};
use crate::Refused;
use std::io;

pub(super) fn open_own_executable() -> io::Result<OwnExecutable> {
    Err(refused::image_identity().into())
}

/// Darwin's kernel CodeDirectory hash of this running image. A replaced on-disk
/// executable cannot change this result.
pub(super) fn own_loaded_cdhash() -> io::Result<[u8; 20]> {
    let mut hash = [0; 20];
    // SAFETY: getpid has no preconditions; CS_OPS_CDHASH is read-only and
    // receives a writable buffer of precisely the kernel ABI's20 bytes.
    let result = unsafe {
        csops(
            libc::getpid(),
            CS_OPS_CDHASH,
            hash.as_mut_ptr().cast(),
            hash.len(),
        )
    };
    finish(result, hash)
}

/// Query the exact process incarnation identified by an authenticated kernel
/// audit token. Callers must obtain it from their admitted protected peer, never
/// browser/command bytes. This uses csops_audittoken, with no unchecked PID,
/// static-signature, filesystem or environment fallback; exec/exit refuses.
pub(super) fn loaded_cdhash_for_audit_token(token: &[u8; 32]) -> io::Result<[u8; 20]> {
    // Native audit_token_t is eight aligned native-endian u32 words. Do not
    // cast a potentially unaligned byte slice into a kernel ABI pointer.
    let mut words = [0_u32; 8];
    for (word, bytes) in words.iter_mut().zip(token.as_chunks::<4>().0) {
        *word = u32::from_ne_bytes(*bytes);
    }
    let pid = libc::pid_t::try_from(words[5])
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid audit token PID"))?;
    let mut hash = [0; 20];
    // SAFETY: live aligned audit_token_t and exact writable output buffer.
    // The kernel binds PID and pidversion atomically; no separate PID lookup.
    let result = unsafe {
        csops_audittoken(
            pid,
            CS_OPS_CDHASH,
            hash.as_mut_ptr().cast(),
            hash.len(),
            &mut words,
        )
    };
    finish(result, hash)
}

pub(super) fn release_platform() -> Result<ReleasePlatform, Refused> {
    Ok(ReleasePlatform::Macos)
}

const CS_OPS_CDHASH: libc::c_uint = 5;
extern "C" {
    fn csops(
        pid: libc::pid_t,
        operation: libc::c_uint,
        output: *mut libc::c_void,
        size: usize,
    ) -> libc::c_int;
    fn csops_audittoken(
        pid: libc::pid_t,
        operation: libc::c_uint,
        output: *mut libc::c_void,
        size: usize,
        token: *mut [u32; 8],
    ) -> libc::c_int;
}
fn finish(result: libc::c_int, hash: [u8; 20]) -> io::Result<[u8; 20]> {
    if result == 0 {
        Ok(hash)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern "C" {
        static mach_task_self_: libc::mach_port_t;
        fn task_info(
            task: libc::mach_port_t,
            flavor: libc::task_flavor_t,
            info: *mut libc::integer_t,
            count: *mut libc::mach_msg_type_number_t,
        ) -> libc::kern_return_t;
    }
    #[test]
    fn kernel_self_and_exact_audit_incarnation_match_and_forged_tokens_fail() {
        let own = own_loaded_cdhash().unwrap();
        assert_ne!(own, [0; 20]);
        let mut words = [0_u32; 8];
        let mut count = 8;
        // SAFETY: system own-task port and writable eight-word audit-token ABI.
        let status =
            unsafe { task_info(mach_task_self_, 15, words.as_mut_ptr().cast(), &mut count) };
        assert_eq!(status, 0);
        assert_eq!(count, 8);
        let encode = |words: [u32; 8]| {
            let mut token = [0; 32];
            for (bytes, word) in token.as_chunks_mut::<4>().0.iter_mut().zip(words) {
                bytes.copy_from_slice(&word.to_ne_bytes());
            }
            token
        };
        assert_eq!(loaded_cdhash_for_audit_token(&encode(words)).unwrap(), own);
        words[7] ^= 1;
        assert_eq!(
            loaded_cdhash_for_audit_token(&encode(words))
                .unwrap_err()
                .raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(
            loaded_cdhash_for_audit_token(&[0; 32]).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        words[5] = u32::MAX;
        assert_eq!(
            loaded_cdhash_for_audit_token(&encode(words))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn macos_names_its_release_platform() {
        assert_eq!(release_platform(), Ok(ReleasePlatform::Macos));
    }
}
