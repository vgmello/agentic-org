//! The procfs records of one process (`stat`, `status`, `auxv`, `environ`) and the guards
//! the recovery scan applies to what they say.

use std::fs::File;
use std::io;
use std::time::Instant;

use crate::process::{native_mark, MarkEvidence, ProcessIdentity};

use super::pidfd::DescriptorExhaustion;
use super::procfs::read_at;
use super::{invalid, ENVIRONMENT_LIMIT, PROC_RECORD_LIMIT, PROC_STATUS_LIMIT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stat {
    pub(super) parent: u32,
    pub(super) group: u32,
    pub(super) session: u32,
    pub(super) started: u64,
    pub(super) environment: (u64, u64),
    pub(super) zombie: bool,
}

pub(super) fn read_stat(directory: &File, pid: u32, deadline: Option<Instant>) -> io::Result<Stat> {
    let bytes = read_at(directory, c"stat", PROC_RECORD_LIMIT, deadline)?;
    parse_stat(&bytes, pid)
}

pub(super) fn parse_stat(bytes: &[u8], pid: u32) -> io::Result<Stat> {
    let open = bytes
        .iter()
        .position(|b| *b == b'(')
        .ok_or_else(|| invalid("missing proc stat comm"))?;
    let close = bytes
        .iter()
        .rposition(|b| *b == b')')
        .filter(|close| *close > open)
        .ok_or_else(|| invalid("unterminated proc stat comm"))?;
    let prefix = bytes[..open]
        .strip_suffix(b" ")
        .ok_or_else(|| invalid("invalid proc stat PID"))?;
    if number::<u32>(prefix)? != pid {
        return Err(invalid("proc stat PID mismatch"));
    }
    let mut fields = bytes[close + 1..]
        .split(|b| b.is_ascii_whitespace())
        .filter(|f| !f.is_empty());
    let mut next = || {
        fields
            .next()
            .ok_or_else(|| invalid("short proc stat record"))
    };
    let state = next()?;
    if state.len() != 1 {
        return Err(invalid("invalid proc stat state"));
    }
    let parent = number(next()?)?;
    // The kernel leaves these hints at -1 if signal state disappears during reaping.
    let hint = |field: &[u8]| if field == b"-1" { Ok(0) } else { number(field) };
    let group = hint(next()?)?;
    let session = hint(next()?)?;
    let started = number(
        fields
            .nth(15)
            .ok_or_else(|| invalid("missing proc stat start time"))?,
    )?;
    // Field 22 was start time. Fields 50 and 51 bound the environment.
    let environment = (
        number(
            fields
                .nth(27)
                .ok_or_else(|| invalid("missing proc stat env_start"))?,
        )?,
        number(
            fields
                .next()
                .ok_or_else(|| invalid("missing proc stat env_end"))?,
        )?,
    );
    Ok(Stat {
        parent,
        group,
        session,
        started,
        environment,
        zombie: matches!(state, b"Z" | b"X" | b"x"),
    })
}

pub(super) struct Credentials {
    pub(super) uid: [u32; 4],
    pub(super) gid: [u32; 4],
    pub(super) capabilities: u64,
}
impl Credentials {
    pub(super) fn safe(&self, uid: u32) -> bool {
        uid != 0 && self.uid == [uid; 4] && self.gid == [self.gid[0]; 4] && self.capabilities == 0
    }
}

pub(super) fn credentials(directory: &File, deadline: Instant) -> io::Result<Credentials> {
    let bytes = read_at(directory, c"status", PROC_STATUS_LIMIT, Some(deadline))?;
    // A comm field can contain non-UTF-8 bytes. Parse only the exact numeric rows.
    let value = |key: &[u8]| -> io::Result<&str> {
        let mut rows = bytes
            .split(|b| *b == b'\n')
            .filter_map(|line| line.strip_prefix(key));
        let row = rows
            .next()
            .ok_or_else(|| invalid("missing proc credential row"))?;
        if rows.next().is_some() {
            return Err(invalid("duplicate proc credential row"));
        }
        std::str::from_utf8(row).map_err(|_| invalid("invalid proc credential row"))
    };
    let ids = |key: &[u8]| -> io::Result<[u32; 4]> {
        let mut words = value(key)?.split_ascii_whitespace();
        let mut ids = [0; 4];
        for id in &mut ids {
            *id = words
                .next()
                .and_then(|word| word.parse().ok())
                .ok_or_else(|| invalid("invalid proc credential IDs"))?;
        }
        if words.next().is_some() {
            return Err(invalid("extra proc credential ID"));
        }
        Ok(ids)
    };
    let caps = |key| {
        u64::from_str_radix(value(key)?.trim(), 16)
            .map_err(|_| invalid("invalid proc capabilities"))
    };
    Ok(Credentials {
        uid: ids(b"Uid:")?,
        gid: ids(b"Gid:")?,
        capabilities: caps(b"CapPrm:")? | caps(b"CapEff:")?,
    })
}

// AT_SECURE distinguishes a privileged exec even when the process has since
// reset its UID/GID values. Unknown layouts or unreadable evidence fail closed.
pub(super) fn ordinary_exec(directory: &File, deadline: Instant) -> io::Result<bool> {
    ordinary_exec_with(|| read_at(directory, c"auxv", PROC_RECORD_LIMIT, Some(deadline)))
}

pub(super) fn ordinary_exec_with(read: impl FnOnce() -> io::Result<Vec<u8>>) -> io::Result<bool> {
    evidence_or(read().map(|bytes| auxv_ordinary(&bytes)), false)
}

pub(super) fn auxv_ordinary(bytes: &[u8]) -> bool {
    let word = std::mem::size_of::<usize>();
    let mut entries = bytes.chunks_exact(2 * word);
    if !entries.remainder().is_empty() {
        return false;
    }
    let mut ordinary = None;
    for entry in &mut entries {
        let kind = usize::from_ne_bytes(entry[..word].try_into().unwrap());
        let value = usize::from_ne_bytes(entry[word..].try_into().unwrap());
        if kind == libc::AT_NULL as usize {
            return value == 0 && entries.len() == 0 && ordinary == Some(true);
        }
        if kind == libc::AT_SECURE as usize && ordinary.replace(value == 0).is_some() {
            return false;
        }
    }
    false
}

pub(super) fn read_mark(
    directory: &File,
    pid: u32,
    stat: Stat,
    mark_prefix: &[u8],
    deadline: Instant,
) -> io::Result<MarkEvidence> {
    read_mark_with(
        stat,
        mark_prefix,
        |length| read_at(directory, c"environ", length, Some(deadline)),
        || read_stat(directory, pid, Some(deadline)),
    )
}

pub(super) fn read_mark_with(
    stat: Stat,
    mark_prefix: &[u8],
    read: impl FnOnce(usize) -> io::Result<Vec<u8>>,
    current: impl FnOnce() -> io::Result<Stat>,
) -> io::Result<MarkEvidence> {
    let (start, end) = stat.environment;
    let Some(length) = end
        .checked_sub(start)
        .filter(|length| start != 0 && *length <= ENVIRONMENT_LIMIT as u64)
    else {
        return Ok(MarkEvidence::Unreadable);
    };
    let capture: io::Result<MarkEvidence> = (|| {
        let bytes = read(length as usize)?;
        // A hole in the target's memory can produce an early EOF even after a
        // complete NUL-terminated entry. Require the entire advertised range.
        if bytes.len() != length as usize {
            return Ok(MarkEvidence::Unreadable);
        }
        let current = current()?;
        if !environment_unchanged(stat, current) {
            return Ok(MarkEvidence::Unreadable);
        }
        Ok(native_mark(&bytes, mark_prefix))
    })();
    evidence_or(capture, MarkEvidence::Unreadable)
}

fn evidence_or<T>(capture: io::Result<T>, unreadable: T) -> io::Result<T> {
    match capture {
        Ok(evidence) => Ok(evidence),
        Err(err)
            if err.kind() == io::ErrorKind::TimedOut
                || err
                    .get_ref()
                    .is_some_and(|source| source.is::<DescriptorExhaustion>()) =>
        {
            Err(err)
        }
        Err(_) => Ok(unreadable),
    }
}

pub(super) fn number<T: std::str::FromStr>(bytes: &[u8]) -> io::Result<T> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| invalid("invalid proc number"))
}

pub(super) fn sample_mark_allowed(
    stat: Stat,
    credentials_safe: bool,
    pid: u32,
    supervisor: u32,
) -> bool {
    !stat.zombie && credentials_safe && pid != supervisor
}

pub(super) fn verify_signal_stat(stat: Stat, target: &ProcessIdentity) -> io::Result<()> {
    if stat.started.to_string() != target.started {
        return Err(invalid("pinned process start identity changed"));
    }
    if stat.zombie {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "recovery cannot verify credentials and mark for an exited thread-group leader",
        ));
    }
    Ok(())
}

pub(super) fn environment_unchanged(before: Stat, after: Stat) -> bool {
    after.started == before.started && !after.zombie && after.environment == before.environment
}
