//! Resident memory (RSS) tree sampling on macOS via `libproc`.

use std::collections::{HashSet, VecDeque};

/// Upper bound on process-tree nodes walked in one sample so pathological trees stay bounded.
const MAX_TREE_PIDS: usize = 4096;
/// Upper bound on direct children buffered from a single `proc_listchildpids` call.
const MAX_DIRECT_CHILDREN: usize = 4096;

/// Returns total resident set size (RSS) in bytes across `root_pid` and its live descendant tree,
/// or `None` if `root_pid` does not exist or cannot be sampled.
pub(in crate::process) fn process_tree_rss_bytes(root_pid: u32) -> Option<u64> {
    let root = libc::pid_t::try_from(root_pid).ok().filter(|&p| p > 0)?;
    let mut total = single_pid_rss_bytes(root)?;
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(root);
    queue.push_back(root);
    while let Some(pid) = queue.pop_front() {
        for child in direct_children(pid) {
            if visited.len() >= MAX_TREE_PIDS {
                return Some(total);
            }
            if child <= 0 || !visited.insert(child) {
                continue;
            }
            total = total.saturating_add(single_pid_rss_bytes(child).unwrap_or(0));
            queue.push_back(child);
        }
    }
    Some(total)
}

fn single_pid_rss_bytes(pid: libc::pid_t) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `info` is correctly sized for `PROC_PIDTASKINFO` and exclusively borrowed.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: `proc_pidinfo` initialized the complete `proc_taskinfo` record.
    Some(unsafe { info.assume_init() }.pti_resident_size)
}

fn direct_children(ppid: libc::pid_t) -> Vec<libc::pid_t> {
    // SAFETY: null sizing query for child PIDs of `ppid`.
    let estimate = unsafe { libc::proc_listchildpids(ppid, std::ptr::null_mut(), 0) };
    if estimate <= 0 {
        return Vec::new();
    }
    let capacity = (estimate as usize)
        .saturating_add(16)
        .min(MAX_DIRECT_CHILDREN);
    let mut pids = vec![0 as libc::pid_t; capacity];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: `pids` is an initialized writable buffer of exact byte length `bytes`.
    let count = unsafe { libc::proc_listchildpids(ppid, pids.as_mut_ptr().cast(), bytes) };
    if count <= 0 {
        return Vec::new();
    }
    pids.truncate((count as usize).min(capacity));
    pids
}
