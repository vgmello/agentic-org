//! Resident memory (RSS) tree sampling on GNU/Linux via `/proc/<pid>/status`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;

/// Maximum bytes read from a single `/proc/<pid>/status` file (rules §3.1 bounded reads).
const MAX_STATUS_BYTES: u64 = 64 * 1024;
/// Upper bound on process-tree nodes walked in one sample so pathological trees stay bounded.
const MAX_TREE_PIDS: usize = 4096;

/// Returns total resident set size (RSS) in bytes across `root_pid` and its live descendant tree,
/// or `None` if `root_pid` does not exist or cannot be sampled.
pub(in crate::process) fn process_tree_rss_bytes(root_pid: u32) -> Option<u64> {
    let (_, root_rss) = read_proc_status(root_pid)?;
    let root_rss = root_rss?;
    let (children, rss_by_pid) = collect_proc_snapshot();
    Some(sum_tree_rss(root_pid, root_rss, &children, &rss_by_pid))
}

fn read_proc_status(pid: u32) -> Option<(Option<u32>, Option<u64>)> {
    let path = format!("/proc/{pid}/status");
    // Open with O_NOFOLLOW | O_NONBLOCK (std sets O_CLOEXEC) and confirm regular file on the fd.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut buf = String::new();
    let read = file
        .take(MAX_STATUS_BYTES.saturating_add(1))
        .read_to_string(&mut buf)
        .ok()?;
    if (read as u64) > MAX_STATUS_BYTES {
        return None;
    }
    Some(parse_status_ppid_and_rss(&buf))
}

pub(super) fn parse_status_ppid_and_rss(status: &str) -> (Option<u32>, Option<u64>) {
    let mut ppid = None;
    let mut rss_bytes = None;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            ppid = rest.trim().parse::<u32>().ok();
        } else if let Some(rest) = line.strip_prefix("VmRSS:") {
            rss_bytes = rest
                .split_whitespace()
                .next()
                .and_then(|val| val.parse::<u64>().ok())
                .map(|kib| kib.saturating_mul(1024));
        }
        if ppid.is_some() && rss_bytes.is_some() {
            break;
        }
    }
    (ppid, rss_bytes)
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
    fn parses_ppid_and_vm_rss_kib_safely() {
        let sample =
            "Name:\tworker\nState:\tS (sleeping)\nPPid:\t42\nVmSize:\t10000 kB\nVmRSS:\t  512 kB\n";
        assert_eq!(
            parse_status_ppid_and_rss(sample),
            (Some(42), Some(512 * 1024))
        );
        assert_eq!(
            parse_status_ppid_and_rss("Name:\tzombie\nPPid:\t1\n"),
            (Some(1), None)
        );
        assert_eq!(
            parse_status_ppid_and_rss("VmRSS:\tnot-a-number kB\n"),
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
