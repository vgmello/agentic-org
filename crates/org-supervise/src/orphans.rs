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
//! - the members of a **recorded tool session** whose leader was still the very process that
//!   was recorded (same pid and start time) when cleanup began, and of the **recorded group**
//!   on the same condition, for as long as a process already identified as the session's
//!   lives in it (a session or group id can't be reused while it has a member), and their
//!   descendants.
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

/// Finds and kills what's left of a session (see the module docs):
/// 1. **Who:** the processes that carry the mark; the members of each recorded tool session
///    whose leader is still the recorded process; the members of the recorded group if its
///    leader is still the recorded process; and all their descendants.
/// 2. **`SIGTERM`:** the recorded leader first (Claude kills its tools' trees on it), then
///    every other target; then up to `grace` for them to go.
/// 3. **Freeze:** whatever is left (scanned again) gets `SIGSTOP`, and the scan repeats until
///    it finds nothing new, so nothing can fork between the scan and the kill.
/// 4. **`SIGKILL`** to every frozen process (and to the group while it's proven to be the
///    session's); one that can no longer be confirmed is resumed (`SIGCONT`), never left
///    stopped.
///
/// A recorded session or group stays "the session's" for as long as one process already
/// identified as the session's still lives in it: a session or group id can't be reused
/// while the session or group has a member. So children forked during the grace inside a
/// tool session are found even after its leader died, and with no readable mark.
///
/// `survivors` is a fresh scan at the end (plus anything identified that still lives).
pub fn kill_orphans(query: &OrphanQuery<'_>) -> Result<OrphanReport> {
    if query.mark.is_empty() {
        return Err(Error::invalid("an empty session mark"));
    }
    let uid = sys::geteuid();
    let mut report = OrphanReport::default();
    let mut signalled: BTreeSet<ProcessId> = BTreeSet::new();
    let mut scope = Scope {
        mark: query.mark,
        sessions: BTreeSet::new(),
        group: None,
        known: BTreeSet::new(),
    };
    for leader in query.tool_sessions {
        if is_still(*leader, uid) {
            scope.sessions.insert(to_pid(leader.pid));
            scope.known.insert(*leader);
        }
    }
    let leader = query.group_leader.filter(|l| is_still(*l, uid));
    if let Some(l) = leader {
        scope.group = Some(to_pid(l.pid));
        scope.known.insert(l);
    }

    // SIGTERM: the leader first, then the rest, then one grace for all of them.
    let first = scope.collect(&Snapshot::take()?);
    scope.known.extend(first.iter().map(|t| t.id));
    if let Some(l) = leader {
        if is_still(l, uid) && sys::kill(to_pid(l.pid), libc::SIGTERM).is_ok() {
            report.leader_signalled = true;
            signalled.insert(l);
        }
    }
    for t in first.iter().filter(|t| Some(t.id) != leader) {
        if signal_if_still(*t, uid, query.mark, libc::SIGTERM) {
            signalled.insert(t.id);
        }
    }
    let mut waiting: Vec<ProcessId> = first.iter().map(|t| t.id).collect();
    waiting.extend(leader);
    wait_gone(&waiting, uid, query.grace);

    // Freeze what's left, until a scan finds nothing new.
    let mut frozen: Vec<Target> = Vec::new();
    let mut tried: BTreeSet<ProcessId> = BTreeSet::new();
    for _ in 0..FREEZE_ROUNDS {
        let new: Vec<Target> = scope
            .collect(&Snapshot::take()?)
            .into_iter()
            .filter(|t| tried.insert(t.id))
            .collect();
        if new.is_empty() {
            break;
        }
        for t in new {
            scope.known.insert(t.id);
            if signal_if_still(t, uid, query.mark, libc::SIGSTOP) {
                frozen.push(t);
            }
        }
    }

    // SIGKILL.
    if let Some(group) = scope.group {
        if scope.group_proven(group, uid) {
            let _ = sys::killpg(group, libc::SIGKILL);
        }
    }
    for t in &frozen {
        if signal_if_still(*t, uid, query.mark, libc::SIGKILL) {
            signalled.insert(t.id);
        } else if is_still(t.id, uid) {
            let _ = sys::kill(to_pid(t.id.pid), libc::SIGCONT);
        }
    }
    let frozen_ids: Vec<ProcessId> = frozen.iter().map(|t| t.id).collect();
    wait_gone(&frozen_ids, uid, Duration::from_secs(1));

    let mut left: BTreeSet<ProcessId> = scope
        .collect(&Snapshot::take()?)
        .into_iter()
        .map(|t| t.id)
        .collect();
    left.extend(scope.known.iter().copied());
    report.survivors = left.into_iter().filter(|id| is_still(*id, uid)).collect();
    report.signalled = signalled.into_iter().collect();
    Ok(report)
}

/// How many rounds the freeze takes at most (each stops what the round before it found).
const FREEZE_ROUNDS: usize = 8;

/// What counts as the session's, re-proven at every scan.
struct Scope<'q> {
    mark: &'q str,
    /// Recorded tool sessions whose leader was the recorded process at the start.
    sessions: BTreeSet<libc::pid_t>,
    /// The recorded group, if its leader was the recorded process at the start.
    group: Option<libc::pid_t>,
    /// Every process identified as the session's so far.
    known: BTreeSet<ProcessId>,
}

impl Scope<'_> {
    /// Whether a process identified as the session's still lives in `group`.
    fn group_proven(&self, group: libc::pid_t, uid: u32) -> bool {
        self.known.iter().any(|k| {
            sys::proc_info(to_pid(k.pid))
                .ok()
                .flatten()
                .is_some_and(|i| {
                    !i.zombie && i.uid == uid && i.start_us == k.start_us && i.pgid == group
                })
        })
    }

    /// The session's processes in `snap`: marked ones, members of proven sessions and of the
    /// proven group, those already known, and all their descendants.
    fn collect(&self, snap: &Snapshot) -> Vec<Target> {
        let live_known: Vec<&ProcInfo> = snap
            .procs
            .values()
            .filter(|p| self.known.contains(&identity(**p)))
            .collect();
        let sessions: BTreeSet<libc::pid_t> = if self.sessions.is_empty() {
            BTreeSet::new()
        } else {
            live_known
                .iter()
                .filter_map(|p| sys::getsid(p.pid).ok())
                .filter(|sid| self.sessions.contains(sid))
                .collect()
        };
        let group = self
            .group
            .filter(|g| live_known.iter().any(|p| p.pgid == *g));
        let marked: BTreeSet<libc::pid_t> = snap
            .procs
            .keys()
            .copied()
            .filter(|&pid| carries_mark(pid, self.mark))
            .collect();
        let mut chosen: BTreeSet<libc::pid_t> = marked.clone();
        chosen.extend(live_known.iter().map(|p| p.pid));
        for p in snap.procs.values() {
            let in_group = group == Some(p.pgid);
            let in_session =
                !sessions.is_empty() && sys::getsid(p.pid).is_ok_and(|s| sessions.contains(&s));
            if in_group || in_session {
                chosen.insert(p.pid);
            }
        }
        let mut queue: Vec<libc::pid_t> = chosen.iter().copied().collect();
        while let Some(pid) = queue.pop() {
            for &child in snap.children.get(&pid).map(Vec::as_slice).unwrap_or(&[]) {
                if chosen.insert(child) {
                    queue.push(child);
                }
            }
        }
        chosen
            .into_iter()
            .filter_map(|pid| {
                snap.procs.get(&pid).map(|p| Target {
                    id: identity(*p),
                    marked: marked.contains(&pid),
                })
            })
            .collect()
    }
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
/// path, its NUL padding (to 8 bytes) and `argc` argument strings, each NUL-terminated, up to
/// the first empty string.
fn environment(area: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest: &[u8] = &[];
    if area.len() >= 4 {
        let argc = i32::from_ne_bytes([area[0], area[1], area[2], area[3]]);
        let mut pos = 4;
        // The exec path, NUL-padded so that it (with its NUL) fills a multiple of 8 bytes from
        // offset 4 (verified on every process of a macOS 26.7 machine). Skipping only that
        // padding keeps an empty `argv[0]` (a lone NUL) from being taken for more padding.
        pos = skip_string(area, pos);
        while pos < area.len() && (pos - 4) % 8 != 0 && area[pos] == 0 {
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
        // "/bin/sh\0" fills exactly 8 bytes: no padding.
        let a = area(
            2,
            &[
                b"/bin/sh\0",
                b"sh\0ORG_SESSION_MARK=fake\0",
                b"A=1\0ORG_SESSION_MARK=m1\0\0",
                b"executable_path=/bin/sh\0",
            ],
        );
        let env: Vec<&[u8]> = environment(&a).collect();
        assert_eq!(env, [b"A=1".as_slice(), b"ORG_SESSION_MARK=m1"]);
        // "/x/y\0" plus 3 NULs of padding, then an empty argv[0] and the mark first.
        let a = area(1, &[b"/x/y\0\0\0\0", b"\0", b"ORG_SESSION_MARK=m2\0\0"]);
        let env: Vec<&[u8]> = environment(&a).collect();
        assert_eq!(env, [b"ORG_SESSION_MARK=m2".as_slice()]);
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
