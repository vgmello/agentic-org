//! Orphan cleanup: finding and killing a session's processes that outlived it, by its
//! recorded process group, its recorded tool sessions and its `ORG_SESSION_MARK`.
//!
//! Why it exists: Claude runs every Bash tool in its own session and process group, so a
//! group kill never reaches them, and a `SIGKILL`ed (or crashed) runtime leaves them running
//! under launchd. The mark, set in every session's environment and inherited by its tools, is
//! what finds them. [`crate::Child::reap`] runs it after every session; `org-office-host`
//! runs it from its watchdog with what the launch pipe recorded ([`OrphanQuery`]).
//!
//! Who is a target, all of this user's:
//! - processes that **carry the mark** in their environment (read with
//!   `sysctl(KERN_PROCARGS2)`, which shows the environment a process was started with), and
//!   their **descendants** (by parent pid, in one snapshot);
//! - the members of a **recorded tool session** whose leader is still the very process that
//!   was recorded (same pid and start time), and their descendants. A session id can't be
//!   reused while its leader lives, so those members are the tool's.
//!
//! Why both: on macOS 26 (verified on 26.7.2) the kernel **withholds the environment of Apple
//! platform binaries** (`/bin/zsh`, `/bin/sh`, `/bin/sleep`, `/usr/bin/perl`, ...) from
//! `KERN_PROCARGS2` for a non-root caller, while third-party and ad-hoc-signed binaries
//! (`node`, so Claude itself) show it. Claude's tool shells are `/bin/zsh`, so the mark finds
//! them only while a marked ancestor lives; the recorded tool sessions cover them after. A
//! tool shell that exits, leaving a platform-binary grandchild behind in its session, can't
//! be found by either (known limit).
//!
//! Every process is named by its pid **and start time** ([`ProcessId`]): right before each
//! signal its uid, start time and (for a marked one) its mark are read again, so a pid that
//! was reused since the snapshot isn't signalled. What's left is the window between that
//! check and the signal, the design's accepted best effort.
//!
//! Known limit (design): a process that clears its environment and double-forks escapes.

use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::sys::{self, PidFilter, ProcInfo};

/// One process: its pid and when it started. A reused pid has a different start time, so the
/// pair names one process for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessId {
    /// The pid.
    pub pid: u32,
    /// When it started, in microseconds since the Unix epoch.
    pub start_us: u64,
}

/// The identity of the live process `pid` now, or `None` if there's none (a zombie counts as
/// none). Record a child's right after the spawn.
pub fn process_id(pid: u32) -> Result<Option<ProcessId>> {
    let Ok(raw) = libc::pid_t::try_from(pid) else {
        return Ok(None);
    };
    Ok(sys::proc_info(raw)
        .map_err(|e| Error::io("read the process table", e))?
        .filter(|i| !i.zombie)
        .map(identity))
}

fn identity(info: ProcInfo) -> ProcessId {
    ProcessId {
        pid: u32::try_from(info.pid).unwrap_or(0),
        start_us: info.start_us,
    }
}

/// What to clean up after one session.
#[derive(Debug, Clone)]
pub struct OrphanQuery<'a> {
    /// The session's `ORG_SESSION_MARK`.
    pub mark: &'a str,
    /// The recorded group leader (its pid is the group id), if the caller has it. If it's
    /// still alive with that start time, it gets `SIGTERM` first (Claude kills its tools'
    /// trees on it), then, after the grace, the group gets `SIGKILL`.
    pub group_leader: Option<ProcessId>,
    /// The leaders of the runtime's direct children's own sessions, as recorded
    /// ([`crate::Child::record_tool_sessions`]): a session whose leader is still that process
    /// is killed whole (the leader's pid is the session id).
    pub tool_sessions: &'a [ProcessId],
    /// How long a `SIGTERM` gets before `SIGKILL`.
    pub grace: Duration,
}

/// What [`kill_orphans`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrphanReport {
    /// Whether the recorded leader was alive and got `SIGTERM` and its group `SIGKILL`.
    pub leader_signalled: bool,
    /// Every process that was signalled (`SIGTERM`, then `SIGKILL` if it outlived the grace).
    pub signalled: Vec<ProcessId>,
    /// Those still alive at the end (this should stay empty).
    pub survivors: Vec<ProcessId>,
}

/// A process to signal, and whether it carried the mark itself (then the mark is checked
/// again before each signal) or was chosen by ancestry or a recorded session (then its pid,
/// start time and uid are its identity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    id: ProcessId,
    marked: bool,
}

/// How many times the scan repeats after the kill, to catch processes forked meanwhile.
const RESCANS: usize = 3;

/// How often a wait checks the process table.
const POLL: Duration = Duration::from_millis(10);

/// This user's live processes that carry `ORG_SESSION_MARK=<mark>` in their environment.
pub fn find_by_mark(mark: &str) -> Result<Vec<ProcessId>> {
    let snap = Snapshot::take()?;
    Ok(snap
        .procs
        .values()
        .filter(|p| carries_mark(p.pid, mark))
        .map(|p| identity(*p))
        .collect())
}

/// Finds and kills what's left of a session (see the module docs), in the design's order:
/// 1. the recorded leader, if alive with its start time: its group's members are taken by
///    identity (pid and start time), then the leader gets `SIGTERM`, the grace, `SIGKILL` to
///    its group if it's still alive (safe while the leader lives: its group id can't be
///    reused), and `SIGKILL` to every member still the process that was taken;
/// 2. the recorded tool sessions' members (while each session's recorded leader lives), then
///    the recorded group's members and every process of this user that carry the mark or
///    descend from one that does: `SIGTERM`, the grace, `SIGKILL` for those still there;
/// 3. the scan again (up to three times) for processes forked meanwhile, killed at once.
pub fn kill_orphans(query: &OrphanQuery<'_>) -> Result<OrphanReport> {
    if query.mark.is_empty() {
        return Err(Error::invalid("an empty session mark"));
    }
    let mut report = OrphanReport::default();
    let uid = sys::geteuid();

    let mut group_members: Vec<ProcessId> = Vec::new();
    if let Some(leader) = query.group_leader {
        if is_still(leader, uid) {
            let pid = to_pid(leader.pid);
            // While the leader lives its group id is surely its group's, so its members are
            // the session's: take them by identity now, before the leader can exit on the
            // SIGTERM and leave them unreachable (Apple binaries show no mark, and they're
            // no longer anyone's descendants once reparented).
            group_members = sys::list_pids(PidFilter::Pgrp(pid))
                .map_err(|e| Error::io("list a process group", e))?
                .into_iter()
                .filter_map(|p| sys::proc_info(p).ok().flatten())
                .filter(|i| i.uid == uid && !i.zombie && i.pgid == pid && i.pid != pid)
                .map(identity)
                .collect();
            if is_still(leader, uid) {
                report.leader_signalled = true;
                let _ = sys::kill(pid, libc::SIGTERM);
                wait_gone(&[leader], uid, query.grace);
                if is_still(leader, uid) {
                    let _ = sys::killpg(pid, libc::SIGKILL);
                    let _ = sys::kill(pid, libc::SIGKILL);
                }
                // Whether or not the leader went on SIGTERM: every member still the process
                // that was recorded gets SIGKILL.
                for m in &group_members {
                    if is_still(*m, uid) && sys::kill(to_pid(m.pid), libc::SIGKILL).is_ok() {
                        report.signalled.push(*m);
                    }
                }
                wait_gone(&group_members, uid, Duration::from_secs(1));
            }
        }
    }

    let targets = targets(query, uid)?;
    for t in &targets {
        if signal_if_still(*t, uid, query.mark, libc::SIGTERM) {
            report.signalled.push(t.id);
        }
    }
    let ids: Vec<ProcessId> = targets.iter().map(|t| t.id).collect();
    wait_gone(&ids, uid, query.grace);
    for t in &targets {
        signal_if_still(*t, uid, query.mark, libc::SIGKILL);
    }
    wait_gone(&ids, uid, Duration::from_secs(1));

    let mut seen: BTreeSet<ProcessId> = ids.iter().chain(&group_members).copied().collect();
    for _ in 0..RESCANS {
        let late: Vec<Target> = targets_in(&Snapshot::take()?, query.mark, &BTreeSet::new(), None)
            .into_iter()
            .filter(|t| !seen.contains(&t.id))
            .collect();
        if late.is_empty() {
            break;
        }
        for t in &late {
            seen.insert(t.id);
            if signal_if_still(*t, uid, query.mark, libc::SIGKILL) {
                report.signalled.push(t.id);
            }
        }
        let late_ids: Vec<ProcessId> = late.iter().map(|t| t.id).collect();
        wait_gone(&late_ids, uid, Duration::from_secs(1));
    }

    report.survivors = seen.into_iter().filter(|t| is_still(*t, uid)).collect();
    Ok(report)
}

/// The targets, in order: recorded group members, recorded tool sessions' members, then the
/// rest of the full scan.
fn targets(query: &OrphanQuery<'_>, uid: u32) -> Result<Vec<Target>> {
    let snap = Snapshot::take()?;
    let mut first: BTreeSet<libc::pid_t> = BTreeSet::new();
    let live_sessions: BTreeSet<libc::pid_t> = query
        .tool_sessions
        .iter()
        .filter(|leader| is_still(**leader, uid))
        .map(|leader| to_pid(leader.pid))
        .collect();
    let mut in_sessions = BTreeSet::new();
    if !live_sessions.is_empty() {
        for &pid in snap.procs.keys() {
            if sys::getsid(pid).is_ok_and(|sid| live_sessions.contains(&sid)) {
                in_sessions.insert(pid);
            }
        }
        first.extend(in_sessions.iter().copied());
    }
    if let Some(leader) = query.group_leader {
        let members = sys::list_pids(PidFilter::Pgrp(to_pid(leader.pid)))
            .map_err(|e| Error::io("list a process group", e))?;
        first.extend(members);
    }
    Ok(targets_in(&snap, query.mark, &in_sessions, Some(&first)))
}

/// The marked processes in `snap`, the `extra` roots, and all their descendants, those in
/// `first` (if given) first.
fn targets_in(
    snap: &Snapshot,
    mark: &str,
    extra: &BTreeSet<libc::pid_t>,
    first: Option<&BTreeSet<libc::pid_t>>,
) -> Vec<Target> {
    let marked: BTreeSet<libc::pid_t> = snap
        .procs
        .keys()
        .copied()
        .filter(|&pid| carries_mark(pid, mark))
        .collect();
    let mut chosen: BTreeSet<libc::pid_t> = marked.union(extra).copied().collect();
    let mut queue: Vec<libc::pid_t> = chosen.iter().copied().collect();
    while let Some(pid) = queue.pop() {
        for &child in snap.children.get(&pid).map(Vec::as_slice).unwrap_or(&[]) {
            if chosen.insert(child) {
                queue.push(child);
            }
        }
    }
    let (mut head, tail): (Vec<_>, Vec<_>) = chosen
        .into_iter()
        .partition(|pid| first.is_some_and(|f| f.contains(pid)));
    head.extend(tail);
    head.into_iter()
        .filter_map(|pid| {
            snap.procs.get(&pid).map(|p| Target {
                id: identity(*p),
                marked: marked.contains(&pid),
            })
        })
        .collect()
}

/// This user's live processes (not this one, not zombies), and who's whose child.
struct Snapshot {
    procs: BTreeMap<libc::pid_t, ProcInfo>,
    children: BTreeMap<libc::pid_t, Vec<libc::pid_t>>,
}

impl Snapshot {
    fn take() -> Result<Snapshot> {
        let uid = sys::geteuid();
        let me = libc::pid_t::try_from(std::process::id()).unwrap_or(-1);
        let pids = sys::list_pids(PidFilter::Uid(uid))
            .map_err(|e| Error::io("list this user's processes", e))?;
        let mut procs = BTreeMap::new();
        let mut children: BTreeMap<libc::pid_t, Vec<libc::pid_t>> = BTreeMap::new();
        for pid in pids {
            if pid == me || pid <= 1 {
                continue;
            }
            let Ok(Some(info)) = sys::proc_info(pid) else {
                continue;
            };
            if info.uid != uid || info.zombie || info.pid != pid {
                continue;
            }
            procs.insert(pid, info);
            children.entry(info.ppid).or_default().push(pid);
        }
        Ok(Snapshot { procs, children })
    }
}

fn to_pid(pid: u32) -> libc::pid_t {
    libc::pid_t::try_from(pid).unwrap_or(-1)
}

/// Whether `id` is still that process: alive (not a zombie), of this user, with the same
/// start time (so not a reused pid).
fn is_still(id: ProcessId, uid: u32) -> bool {
    let Ok(Some(info)) = sys::proc_info(to_pid(id.pid)) else {
        return false;
    };
    !info.zombie && info.uid == uid && info.start_us == id.start_us
}

/// Re-checks `t` right before signalling it (its identity, and its mark if it carried one);
/// returns whether the signal was sent.
fn signal_if_still(t: Target, uid: u32, mark: &str, signal: libc::c_int) -> bool {
    let pid = to_pid(t.id.pid);
    if !is_still(t.id, uid) || (t.marked && !carries_mark(pid, mark)) {
        return false;
    }
    if pid == libc::pid_t::try_from(std::process::id()).unwrap_or(-1) {
        return false;
    }
    sys::kill(pid, signal).is_ok()
}

/// Waits until none of `ids` is still alive, or `grace` passes.
fn wait_gone(ids: &[ProcessId], uid: u32, grace: Duration) {
    let until = Instant::now() + grace;
    while ids.iter().any(|id| is_still(*id, uid)) && Instant::now() < until {
        thread::sleep(POLL);
    }
}

/// Whether `pid`'s environment (as started) holds `ORG_SESSION_MARK=<mark>` exactly.
pub(crate) fn carries_mark(pid: libc::pid_t, mark: &str) -> bool {
    let Ok(area) = sys::procargs(pid) else {
        return false;
    };
    let entry = format!("ORG_SESSION_MARK={mark}");
    let found = environment(&area).any(|e| e == entry.as_bytes());
    found
}

/// The environment strings of a `KERN_PROCARGS2` area: after `argc` (a native `int`), the exec
/// path, its NUL padding and `argc` argument strings, each NUL-terminated, up to the first
/// empty string.
fn environment(area: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest: &[u8] = &[];
    if area.len() >= 4 {
        let argc = i32::from_ne_bytes([area[0], area[1], area[2], area[3]]);
        let mut pos = 4;
        // The exec path.
        pos = skip_string(area, pos);
        // Its padding.
        while pos < area.len() && area[pos] == 0 {
            pos += 1;
        }
        for _ in 0..argc.max(0) {
            pos = skip_string(area, pos);
        }
        rest = area.get(pos..).unwrap_or(&[]);
    }
    rest.split(|&b| b == 0).take_while(|s| !s.is_empty())
}

/// The index just past the NUL ending the string at `pos`.
fn skip_string(area: &[u8], pos: usize) -> usize {
    match area.get(pos..).and_then(|s| s.iter().position(|&b| b == 0)) {
        Some(n) => pos + n + 1,
        None => area.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(argc: i32, parts: &[&[u8]]) -> Vec<u8> {
        let mut v = argc.to_ne_bytes().to_vec();
        for p in parts {
            v.extend_from_slice(p);
        }
        v
    }

    #[test]
    fn the_environment_is_read_after_argv_only() {
        let a = area(
            2,
            &[
                b"/bin/sh\0\0\0\0",
                b"sh\0ORG_SESSION_MARK=fake\0",
                b"A=1\0ORG_SESSION_MARK=m1\0\0",
                b"executable_path=/bin/sh\0",
            ],
        );
        let env: Vec<&[u8]> = environment(&a).collect();
        assert_eq!(env, [b"A=1".as_slice(), b"ORG_SESSION_MARK=m1"]);
        assert!(environment(&area(5, &[b"/x\0a\0"])).next().is_none());
        assert!(environment(&[1, 0]).next().is_none());
    }

    #[test]
    fn this_process_is_readable_and_unmarked() {
        let me = libc::pid_t::try_from(std::process::id()).unwrap();
        let area = sys::procargs(me).unwrap();
        assert!(environment(&area).next().is_some());
        assert!(!carries_mark(me, "no-such-mark"));
        let id = process_id(std::process::id()).unwrap().unwrap();
        assert!(id.start_us > 0);
    }
}
#[cfg(test)]
mod align_probe_tmp {
    use super::*;
    #[test]
    fn align_probe() {
        let uid = sys::geteuid();
        let (mut ok, mut bad, mut pads) = (0, 0, std::collections::BTreeMap::new());
        for pid in sys::list_pids(PidFilter::Uid(uid)).unwrap() {
            let Ok(a) = sys::procargs(pid) else { continue };
            if a.len() < 5 { continue; }
            let end = skip_string(&a, 4);
            let mut pos = end;
            while pos < a.len() && a[pos] == 0 { pos += 1; }
            *pads.entry(pos - end).or_insert(0) += 1;
            if (pos - 4) % 8 == 0 { ok += 1 } else { bad += 1; eprintln!("unaligned pid {pid} end {end} pos {pos}"); }
        }
        eprintln!("aligned8={ok} not={bad} pads={pads:?}");
    }
}
