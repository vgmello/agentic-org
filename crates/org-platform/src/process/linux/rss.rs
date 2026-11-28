//! Resident memory (RSS) tree sampling on GNU/Linux via `/proc/<pid>/status` and
//! `/proc/<pid>/task/*/children` (with full `/proc` fallback when `CONFIG_PROC_CHILDREN` is absent).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;

/// Maximum bytes read from a single `/proc` file (rules §3.1 bounded reads).
const MAX_STATUS_BYTES: u64 = 64 * 1024;
/// Upper bound on process-tree nodes walked in one sample so pathological trees stay bounded.
const MAX_TREE_PIDS: usize = 4096;

/// Returns resident set size (RSS) in bytes for the single process `pid`,
/// or `None` if `pid` does not exist or cannot be sampled.
pub(in crate::process) fn process_rss_bytes(pid: u32) -> Option<u64> {
    read_proc_status(pid)?.1
}

/// Returns total resident set size (RSS) in bytes across `root_pid` and its live descendant tree,
/// or `None` if `root_pid` does not exist or cannot be sampled.
pub(in crate::process) fn process_tree_rss_bytes(root_pid: u32) -> Option<u64> {
    let (_, root_rss) = read_proc_status(root_pid)?;
    let root_rss = root_rss?;
    if let Some(total) = walk_via_task_children(root_pid, root_rss) {
        return Some(total);
    }
    let (children, rss_by_pid) = collect_proc_snapshot();
    Some(sum_tree_rss(root_pid, root_rss, &children, &rss_by_pid))
}

fn read_proc_bytes(path: &str) -> Option<Vec<u8>> {
    // Open with O_NOFOLLOW | O_NONBLOCK (std sets O_CLOEXEC) and confirm regular file on the fd.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut buf = Vec::new();
    let read = file
        .take(MAX_STATUS_BYTES.saturating_add(1))
        .read_to_end(&mut buf)
        .ok()?;
    if (read as u64) > MAX_STATUS_BYTES {
        return None;
    }
    Some(buf)
}

fn read_proc_status(pid: u32) -> Option<(Option<u32>, Option<u64>)> {
    let bytes = read_proc_bytes(&format!("/proc/{pid}/status"))?;
    Some(parse_status_ppid_and_rss_bytes(&bytes))
}

/// Byte-safe parser for Linux `/proc/<pid>/status`: ignores non-UTF-8 `Name:` bytes and scans
/// ASCII `PPid:` and `VmRSS:` prefixes directly.
pub(super) fn parse_status_ppid_and_rss_bytes(status: &[u8]) -> (Option<u32>, Option<u64>) {
    let mut ppid = None;
    let mut rss_bytes = None;
    for raw_line in status.split(|&b| b == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if let Some(rest) = line.strip_prefix(b"PPid:") {
            ppid = std::str::from_utf8(rest)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok());
        } else if let Some(rest) = line.strip_prefix(b"VmRSS:") {
            rss_bytes = std::str::from_utf8(rest)
                .ok()
                .and_then(|s| s.split_whitespace().next())
                .and_then(|val| val.parse::<u64>().ok())
                .map(|kib| kib.saturating_mul(1024));
        }
        if ppid.is_some() && rss_bytes.is_some() {
            break;
        }
    }
    (ppid, rss_bytes)
}

/// Fast `O(tree_size)` BFS walk via `/proc/<pid>/task/*/children`; returns `None` if the kernel
/// lacks `/proc/<pid>/task/*/children`, allowing fallback to a full `/proc` scan.
fn walk_via_task_children(root_pid: u32, root_rss: u64) -> Option<u64> {
    let root_kids = read_direct_children_from_tasks(root_pid)?;
    let mut total = root_rss;
    let mut visited = HashSet::from([root_pid]);
    let mut queue = VecDeque::new();
    enqueue_children(&root_kids, &mut visited, &mut queue);
    while let Some(pid) = queue.pop_front() {
        if let Some((_, Some(bytes))) = read_proc_status(pid) {
            total = total.saturating_add(bytes);
        }
        if let Some(kids) = read_direct_children_from_tasks(pid) {
            if !enqueue_children(&kids, &mut visited, &mut queue) {
                return Some(total);
            }
        }
    }
    Some(total)
}

fn enqueue_children(kids: &[u32], visited: &mut HashSet<u32>, queue: &mut VecDeque<u32>) -> bool {
    for &child in kids {
        if visited.len() >= MAX_TREE_PIDS {
            return false;
        }
        if visited.insert(child) {
            queue.push_back(child);
        }
    }
    true
}

fn read_direct_children_from_tasks(pid: u32) -> Option<Vec<u32>> {
    let entries = std::fs::read_dir(format!("/proc/{pid}/task")).ok()?;
    let mut saw_children_file = false;
    let mut children = Vec::new();
    for entry in entries.flatten() {
        let Some(tid) = parse_positive_pid(&entry.file_name()) else {
            continue;
        };
        let Some(bytes) = read_proc_bytes(&format!("/proc/{pid}/task/{tid}/children")) else {
            continue;
        };
        saw_children_file = true;
        parse_child_pids(&bytes, pid, &mut children);
    }
    saw_children_file.then_some(children)
}

fn parse_positive_pid(name: &std::ffi::OsStr) -> Option<u32> {
    name.to_str()?.parse::<u32>().ok().filter(|&p| p > 0)
}

fn parse_child_pids(bytes: &[u8], parent_pid: u32, out: &mut Vec<u32>) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    out.extend(
        text.split_whitespace()
            .filter_map(|tok| tok.parse::<u32>().ok())
            .filter(|&child| child > 0 && child != parent_pid),
    );
}

fn collect_proc_snapshot() -> (HashMap<u32, Vec<u32>>, HashMap<u32, u64>) {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut rss_by_pid: HashMap<u32, u64> = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return (children, rss_by_pid);
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
            .filter(|&p| p > 0)
        else {
            continue;
        };
        let Some((ppid, rss)) = read_proc_status(pid) else {
            continue;
        };
        if let Some(parent) = ppid.filter(|&p| p > 0 && p != pid) {
            children.entry(parent).or_default().push(pid);
        }
        if let Some(bytes) = rss {
            rss_by_pid.insert(pid, bytes);
        }
    }
    (children, rss_by_pid)
}

fn sum_tree_rss(
    root_pid: u32,
    root_rss: u64,
    children: &HashMap<u32, Vec<u32>>,
    rss_by_pid: &HashMap<u32, u64>,
) -> u64 {
    let mut total = rss_by_pid.get(&root_pid).copied().unwrap_or(root_rss);
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(root_pid);
    queue.push_back(root_pid);
    while let Some(pid) = queue.pop_front() {
        let Some(kids) = children.get(&pid) else {
            continue;
        };
        for &child in kids {
            if visited.len() >= MAX_TREE_PIDS {
                return total;
            }
            if !visited.insert(child) {
                continue;
            }
            total = total.saturating_add(rss_by_pid.get(&child).copied().unwrap_or(0));
            queue.push_back(child);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ppid_and_vm_rss_kib_including_non_utf8_name() {
        let sample =
            b"Name:\tworker\nState:\tS (sleeping)\nPPid:\t42\nVmSize:\t10000 kB\nVmRSS:\t  512 kB\n";
        assert_eq!(
            parse_status_ppid_and_rss_bytes(sample),
            (Some(42), Some(512 * 1024))
        );
        let non_utf8 = b"Name:\t\xff\xfe-proc\nPPid:\t99\nVmRSS:\t256 kB\n";
        assert_eq!(
            parse_status_ppid_and_rss_bytes(non_utf8),
            (Some(99), Some(256 * 1024))
        );
        assert_eq!(
            parse_status_ppid_and_rss_bytes(b"Name:\tzombie\nPPid:\t1\n"),
            (Some(1), None)
        );
        assert_eq!(
            parse_status_ppid_and_rss_bytes(b"VmRSS:\tnot-a-number kB\n"),
            (None, None)
        );
    }

    #[test]
    fn tree_sum_handles_cycles_and_descendants() {
        let mut children = HashMap::new();
        children.insert(10, vec![20, 30]);
        children.insert(20, vec![40, 10]);
        let rss = HashMap::from([(10, 100), (20, 200), (30, 300), (40, 400)]);
        assert_eq!(sum_tree_rss(10, 100, &children, &rss), 1000);
    }
}
