//! Step 1 of the Linux adapter: the preconditions at entry (Linux host design,
//! section 3.3). The process must have exactly one thread, because Landlock and the
//! seccomp filter restrict the calling thread and those it creates later, and exactly
//! descriptors 0 to 2, because a descriptor it already holds stays usable.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};

/// A precondition that does not hold. Nothing was confined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EntryRefusal {
    /// `/proc/self/status` or `/proc/self/fd` cannot be read or has another form.
    Unreadable(&'static str),
    /// The process has this many threads.
    Threads(u32),
    /// The process's open descriptors are not exactly 0, 1 and 2.
    Descriptors(Vec<RawFd>),
}

impl fmt::Display for EntryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(what) => write!(f, "{what} cannot be read"),
            Self::Threads(count) => write!(
                f,
                "the process has {count} threads; it must confine itself before it \
                 starts any"
            ),
            Self::Descriptors(open) => write!(
                f,
                "the process must hold exactly descriptors 0, 1 and 2; it holds {open:?}"
            ),
        }
    }
}

/// More entries than this in `/proc/self/fd` refuse without reading on.
const MOST_DESCRIPTORS: usize = 64;

/// The `Threads:` count of a `/proc/<pid>/status` text.
pub(super) fn threads(status: &str) -> Option<u32> {
    super::status::field(status, "Threads")?.parse().ok()
}

/// Whether the open descriptors are exactly standard input, output and error.
pub(super) fn only_stdio(open: &[RawFd]) -> bool {
    let mut sorted = open.to_vec();
    sorted.sort_unstable();
    sorted == [0, 1, 2]
}

/// The descriptor numbers in one buffer of `getdents64` records
/// (`struct linux_dirent64`: inode 8 bytes, offset 8, record length 2, type 1, then
/// the NUL-terminated name). `None` for a record that does not fit or a name that is
/// not a number, other than `.` and `..`.
pub(super) fn descriptor_names(mut records: &[u8], found: &mut Vec<RawFd>) -> Option<()> {
    const NAME: usize = 19;
    while !records.is_empty() {
        let length = usize::from(u16::from_ne_bytes([*records.get(16)?, *records.get(17)?]));
        let record = records.get(..length).filter(|_| length > NAME)?;
        let name = record[NAME..].split(|byte| *byte == 0).next()?;
        if name != b"." && name != b".." {
            found.push(std::str::from_utf8(name).ok()?.parse().ok()?);
        }
        records = &records[length..];
    }
    Some(())
}

/// The process's open descriptors, without the one this function reads them through.
fn descriptors() -> Result<Vec<RawFd>, EntryRefusal> {
    const UNREADABLE: EntryRefusal = EntryRefusal::Unreadable("/proc/self/fd");
    let directory = File::open("/proc/self/fd").map_err(|_| UNREADABLE)?;
    let mut found = Vec::new();
    let mut buffer = [0u8; 2048];
    loop {
        // SAFETY: the buffer is writable for the length given, and the descriptor
        // is this function's own open directory.
        let read = unsafe {
            libc::syscall(
                libc::SYS_getdents64,
                directory.as_raw_fd(),
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        };
        if read == 0 {
            break;
        }
        let records = usize::try_from(read)
            .ok()
            .and_then(|read| buffer.get(..read))
            .ok_or(UNREADABLE)?;
        descriptor_names(records, &mut found).ok_or(UNREADABLE)?;
        if found.len() > MOST_DESCRIPTORS {
            break;
        }
    }
    found.retain(|fd| *fd != directory.as_raw_fd());
    Ok(found)
}

/// The preconditions of confinement. Reads `/proc`; changes nothing.
pub(super) fn check() -> Result<(), EntryRefusal> {
    const UNREADABLE: EntryRefusal = EntryRefusal::Unreadable("/proc/self/status");
    let mut status = String::new();
    File::open("/proc/self/status")
        .and_then(|mut file| file.read_to_string(&mut status))
        .map_err(|_: io::Error| UNREADABLE)?;
    match threads(&status).ok_or(UNREADABLE)? {
        1 => (),
        count => return Err(EntryRefusal::Threads(count)),
    }
    let open = descriptors()?;
    if !only_stdio(&open) {
        return Err(EntryRefusal::Descriptors(open));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thread_count_is_read_from_the_status_text() {
        let status = "Name:\tweb\nTgid:\t7\nThreads:\t1\nSigQ:\t0/100\n";
        assert_eq!(threads(status), Some(1));
        assert_eq!(threads(&status.replace("\t1\n", "\t12\n")), Some(12));
        assert_eq!(threads("Name:\tThreads:\n"), None);
        assert_eq!(threads("Threads:\tmany\n"), None);
        assert_eq!(threads(""), None);
    }

    #[test]
    fn only_the_three_standard_descriptors_pass() {
        assert!(only_stdio(&[0, 1, 2]));
        assert!(only_stdio(&[2, 0, 1]));
        for open in [
            &[][..],
            &[0, 1],
            &[0, 1, 3],
            &[1, 2, 3],
            &[0, 1, 2, 3],
            &[0, 1, 2, 2],
            &[0, 0, 1, 2],
        ] {
            assert!(!only_stdio(open), "{open:?}");
        }
    }

    fn record(name: &[u8]) -> Vec<u8> {
        let length = (19 + name.len() + 1).next_multiple_of(8);
        let mut bytes = vec![0u8; length];
        bytes[16..18].copy_from_slice(&(length as u16).to_ne_bytes());
        bytes[19..19 + name.len()].copy_from_slice(name);
        bytes
    }

    #[test]
    fn directory_records_give_the_descriptor_numbers() {
        let records: Vec<u8> = [&b"."[..], b"..", b"0", b"1", b"2", b"17"]
            .into_iter()
            .flat_map(record)
            .collect();
        let mut found = Vec::new();
        assert_eq!(descriptor_names(&records, &mut found), Some(()));
        assert_eq!(found, [0, 1, 2, 17]);
    }

    #[test]
    fn a_malformed_directory_record_is_refused() {
        let mut found = Vec::new();
        assert_eq!(descriptor_names(&record(b"x"), &mut found), None);
        assert_eq!(descriptor_names(&record(b"-1x"), &mut found), None);
        // A record that claims more bytes than the buffer holds, or none at all.
        let mut long = record(b"3");
        long[16..18].copy_from_slice(&200u16.to_ne_bytes());
        assert_eq!(descriptor_names(&long, &mut found), None);
        let mut empty = record(b"3");
        empty[16..18].copy_from_slice(&0u16.to_ne_bytes());
        assert_eq!(descriptor_names(&empty, &mut found), None);
        assert_eq!(descriptor_names(&[0u8; 10], &mut found), None);
    }

    #[test]
    fn a_process_with_a_second_thread_is_refused_for_its_threads() {
        let (release, parked) = std::sync::mpsc::channel::<()>();
        let second = std::thread::spawn(move || parked.recv());
        let refusal = check().unwrap_err();
        let EntryRefusal::Threads(count) = refusal else {
            panic!("refused for another reason: {refusal}");
        };
        assert!(count > 1);
        drop(release);
        let _ = second.join().unwrap();
    }

    #[test]
    fn the_listing_finds_the_standard_descriptors_and_one_held_beside_them() {
        // That the listing leaves out its own descriptor needs a process with no
        // other thread: the child-process tests of the adapter show it, where a
        // process with exactly descriptors 0 to 2 passes.
        let held = File::open("/proc/self/status").unwrap();
        let open = descriptors().unwrap();
        for fd in [0, 1, 2, held.as_raw_fd()] {
            assert!(open.contains(&fd), "{fd} in {open:?}");
        }
    }
}
