//! What libproc and the kernel process list say of a process: its identity, credentials
//! and ancestry, each read as one complete record.

use std::io;

use crate::process::{MarkEvidence, ProcessIdentity, ProcessObservation};

use super::{invalid, KINFO_PROC_PID_OFFSET, KINFO_PROC_SIZE, PROC_FLAG_PSUGID, PROC_FLAG_TRACED};

pub(super) fn proc_list_pids(
    bytes: &[u8],
    supervisor: libc::pid_t,
) -> io::Result<Vec<libc::pid_t>> {
    let (rows, remainder) = bytes.as_chunks::<KINFO_PROC_SIZE>();
    if !remainder.is_empty() {
        return Err(invalid("short kinfo_proc record"));
    }
    let pids: Vec<_> = rows
        .iter()
        .map(|row| {
            let pid = &row[KINFO_PROC_PID_OFFSET..][..std::mem::size_of::<libc::pid_t>()];
            libc::pid_t::from_ne_bytes(pid.try_into().unwrap())
        })
        .collect();
    if !pids.contains(&supervisor) {
        return Err(invalid("recovery process list omits the supervisor"));
    }
    Ok(pids)
}

pub(super) enum SnapshotInfo {
    Observed(libc::proc_bsdinfo),
    Absent,
    Inaccessible,
}

pub(super) fn snapshot_info(pid: u32) -> io::Result<SnapshotInfo> {
    classify_snapshot_info(bsd_info(pid))
}

fn classify_snapshot_info(
    result: io::Result<Option<libc::proc_bsdinfo>>,
) -> io::Result<SnapshotInfo> {
    match result {
        Ok(Some(info)) => Ok(SnapshotInfo::Observed(info)),
        Ok(None) => Ok(SnapshotInfo::Absent),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            Ok(SnapshotInfo::Inaccessible)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn enumerated_info(
    pid: u32,
    uid: u32,
    rows: &mut Vec<ProcessObservation>,
    query: impl FnOnce(u32) -> io::Result<Option<libc::proc_bsdinfo>>,
) -> io::Result<Option<libc::proc_bsdinfo>> {
    match classify_snapshot_info(query(pid))? {
        SnapshotInfo::Observed(info) => Ok(Some(info)),
        SnapshotInfo::Absent => Ok(None),
        SnapshotInfo::Inaccessible => {
            // The real-UID list supplies the PID. Without a readable identity
            // or credentials the row can only contribute to S0 reporting.
            rows.push(ProcessObservation {
                pid,
                identity: None,
                parent: None,
                group: 0,
                session: 0,
                uid,
                credentials_safe: false,
                zombie: false,
                mark: MarkEvidence::Unreadable,
            });
            Ok(None)
        }
    }
}

pub(super) fn bsd_info(pid: u32) -> io::Result<Option<libc::proc_bsdinfo>> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is sized for this libproc flavor and pid fits in pid_t.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::pid_t,
            libc::PROC_PIDTBSDINFO,
            1, // Include zombies that have not been reaped.
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written <= 0 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ESRCH | libc::ENOENT) => Ok(None),
            _ => Err(error),
        };
    }
    if written != size {
        return Err(invalid("short libproc identity record"));
    }
    // SAFETY: libproc reported a complete record.
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid {
        return Err(invalid("libproc PID mismatch"));
    }
    Ok(Some(info))
}

pub(super) fn start(info: &libc::proc_bsdinfo) -> String {
    format!("{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec)
}

pub(super) fn same_identity(first: &libc::proc_bsdinfo, second: &libc::proc_bsdinfo) -> bool {
    first.pbi_pid == second.pbi_pid
        && first.pbi_start_tvsec == second.pbi_start_tvsec
        && first.pbi_start_tvusec == second.pbi_start_tvusec
}

pub(super) fn observed_parent_pid(
    initial: &libc::proc_bsdinfo,
    current: &libc::proc_bsdinfo,
) -> Option<u32> {
    // XNU temporarily reports an attaching debugger as the tracee's parent.
    // Both observations must show ordinary process ancestry.
    (same_identity(initial, current)
        && !is_zombie(current)
        && (initial.pbi_flags | current.pbi_flags) & PROC_FLAG_TRACED == 0
        && initial.pbi_ppid == current.pbi_ppid)
        .then_some(current.pbi_ppid)
}

pub(super) fn is_zombie(info: &libc::proc_bsdinfo) -> bool {
    info.pbi_status == libc::SZOMB
}

pub(super) fn live_matches(info: &libc::proc_bsdinfo, identity: &ProcessIdentity) -> bool {
    info.pbi_pid == identity.pid && start(info) == identity.started && !is_zombie(info)
}

pub(super) fn safe_credentials(info: &libc::proc_bsdinfo, uid: u32) -> bool {
    uid != 0
        && info.pbi_uid == uid
        && info.pbi_ruid == uid
        && info.pbi_svuid == uid
        && info.pbi_gid == info.pbi_rgid
        && info.pbi_gid == info.pbi_svgid
        && info.pbi_flags & PROC_FLAG_PSUGID == 0
}
