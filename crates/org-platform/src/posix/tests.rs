use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::testing::{fork_child, ForkedChild};
use super::*;

fn c(text: &str) -> CString {
    CString::new(text).unwrap()
}

fn path(path: &std::path::Path) -> CString {
    CString::new(path.as_os_str().as_bytes()).unwrap()
}

#[test]
fn descriptor_flags_change_only_through_the_owner_and_bad_numbers_fail_closed() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    let fd = stream.as_raw_fd();
    assert!(is_open(fd));
    assert!(is_close_on_exec(fd).unwrap(), "std opens close-on-exec");
    set_close_on_exec(&stream, false).unwrap();
    assert!(!is_close_on_exec(fd).unwrap());
    set_close_on_exec(&stream, true).unwrap();
    assert!(is_close_on_exec(fd).unwrap());

    assert_eq!(status_flags(fd).unwrap() & libc::O_NONBLOCK, 0);
    set_nonblocking(&stream, true).unwrap();
    assert_ne!(status_flags(fd).unwrap() & libc::O_NONBLOCK, 0);
    set_nonblocking(&stream, false).unwrap();
    assert_eq!(status_flags(fd).unwrap() & libc::O_NONBLOCK, 0);

    for bad in [-1, i32::MAX] {
        assert!(!is_open(bad));
        assert_eq!(
            descriptor_flags(bad).unwrap_err().raw_os_error(),
            Some(libc::EBADF)
        );
        assert_eq!(
            status_flags(bad).unwrap_err().raw_os_error(),
            Some(libc::EBADF)
        );
        assert!(status_of_number(bad).is_err());
        assert!(socket_option(bad, libc::SOL_SOCKET, libc::SO_TYPE).is_err());
        assert!(socket_family(bad).is_err());
    }
}

#[test]
fn a_duplicate_is_a_new_close_on_exec_descriptor_above_the_minimum() {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    set_close_on_exec(&stream, false).unwrap();
    let copy = duplicate_above(&stream, 10).unwrap();
    assert!(copy.as_raw_fd() >= 10);
    assert_ne!(copy.as_raw_fd(), stream.as_raw_fd());
    assert!(is_close_on_exec(copy.as_raw_fd()).unwrap());
    assert!(
        !is_close_on_exec(stream.as_raw_fd()).unwrap(),
        "the original is unchanged"
    );
    peer.write_all(b"x").unwrap();
    let mut byte = [0];
    File::from(copy).read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"x");
}

#[test]
fn readiness_waiting_bytes_peek_and_socket_shape() {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    assert!(!poll_readable(stream.as_raw_fd(), Duration::from_millis(1)).unwrap());
    let mut slots = [poll_slot(stream.as_raw_fd(), libc::POLLIN)];
    assert_eq!(poll(&mut slots, Some(Duration::ZERO)).unwrap(), 0);
    peer.write_all(b"abc").unwrap();
    assert_eq!(poll(&mut slots, None).unwrap(), 1);
    assert_ne!(slots[0].revents & libc::POLLIN, 0);
    assert_eq!(bytes_waiting(&stream).unwrap(), 3);
    let mut buffer = [0; 2];
    assert_eq!(
        receive(&stream, &mut buffer, libc::MSG_PEEK | libc::MSG_DONTWAIT).unwrap(),
        2
    );
    assert_eq!(&buffer, b"ab");
    assert_eq!(
        bytes_waiting(&stream).unwrap(),
        3,
        "a peek leaves the bytes"
    );
    assert_eq!(
        poll_readable_either(peer.as_raw_fd(), stream.as_raw_fd(), Duration::ZERO).unwrap(),
        (false, true)
    );
    assert_eq!(
        socket_option(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_TYPE).unwrap(),
        libc::SOCK_STREAM
    );
    assert_eq!(socket_family(stream.as_raw_fd()).unwrap(), libc::AF_UNIX);
    set_socket_option(&stream, libc::SOL_SOCKET, libc::SO_SNDBUF, 64 * 1024).unwrap();

    // A slot for a number that is not open reports POLLNVAL; the readable helpers refuse.
    let mut closed = [poll_slot(i32::MAX, libc::POLLIN)];
    assert_eq!(poll(&mut closed, Some(Duration::ZERO)).unwrap(), 1);
    assert_ne!(closed[0].revents & libc::POLLNVAL, 0);
    assert_eq!(
        poll_readable(i32::MAX, Duration::from_millis(1))
            .unwrap_err()
            .raw_os_error(),
        Some(libc::EBADF)
    );
    assert!(poll_readable(-1, Duration::from_millis(1)).is_err());
}

#[test]
fn directory_relative_calls_never_follow_a_final_symbolic_link() {
    let root = tempfile::tempdir().unwrap();
    let dir = File::open(root.path()).unwrap();
    make_directory_at(&dir, &c("made"), 0o700).unwrap();
    assert_eq!(
        make_directory_at(&dir, &c("made"), 0o700)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::AlreadyExists
    );
    let made = status_at(&dir, &c("made"), libc::AT_SYMLINK_NOFOLLOW).unwrap();
    assert_eq!(made.st_mode & libc::S_IFMT, libc::S_IFDIR);

    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let file = open_at(&dir, &c("file"), flags, 0o600).unwrap();
    assert!(is_close_on_exec(file.as_raw_fd()).unwrap());
    File::from(file).write_all(b"data").unwrap();
    assert_eq!(
        open_at(&dir, &c("file"), flags, 0o600).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );

    std::os::unix::fs::symlink(root.path().join("file"), root.path().join("link")).unwrap();
    let read = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    assert_eq!(
        open_at(&dir, &c("link"), read, 0)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ELOOP)
    );
    let link = status_at(&dir, &c("link"), libc::AT_SYMLINK_NOFOLLOW).unwrap();
    assert_eq!(link.st_mode & libc::S_IFMT, libc::S_IFLNK);
    let held = open_at(&dir, &c("file"), read, 0).unwrap();
    let seen = status(&held).unwrap();
    assert_eq!(seen.st_size, 4);
    assert_eq!(
        status_of_number(held.as_raw_fd()).unwrap().st_ino,
        seen.st_ino
    );

    rename_at(&dir, &c("file"), &dir, &c("renamed")).unwrap();
    assert!(root.path().join("renamed").is_file());
    remove_at(&dir, &c("link"), 0).unwrap();
    assert!(
        root.path().join("renamed").exists(),
        "the link went, not its target"
    );
    remove_at(&dir, &c("made"), libc::AT_REMOVEDIR).unwrap();
    assert_eq!(
        status_at(&dir, &c("made"), libc::AT_SYMLINK_NOFOLLOW)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );

    let whole = open(
        &path(root.path()),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        0,
    )
    .unwrap();
    assert_eq!(
        status(&whole).unwrap().st_ino,
        dir.metadata().unwrap().ino()
    );

    let fifo = root.path().join("fifo");
    make_fifo(&path(&fifo), 0o600).unwrap();
    assert!(std::fs::symlink_metadata(&fifo)
        .unwrap()
        .file_type()
        .is_fifo());
    assert!(make_fifo(&path(&fifo), 0o600).is_err());
}

#[test]
fn entries_list_every_name_but_dot_entries_from_an_independent_offset() {
    let root = tempfile::tempdir().unwrap();
    let dir = File::open(root.path()).unwrap();
    assert_eq!(Entries::of(&dir).unwrap().count(), 0);
    let mut expected = Vec::new();
    for name in ["b", "a", "c"] {
        std::fs::write(root.path().join(name), b"").unwrap();
        expected.push(name.as_bytes().to_vec());
    }
    // A name that is not UTF-8, where the filesystem keeps one (APFS refuses it).
    let not_utf8 = root.path().join(std::ffi::OsStr::from_bytes(b"\xff"));
    match std::fs::write(&not_utf8, b"") {
        Ok(()) => expected.push(vec![0xff]),
        Err(error)
            if error.kind() == std::io::ErrorKind::InvalidInput
                || error.raw_os_error() == Some(libc::EILSEQ) => {}
        Err(error) => panic!("{}: {error}", not_utf8.display()),
    }
    expected.sort();
    for _ in 0..2 {
        let mut names: Vec<Vec<u8>> = Entries::of(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().into_bytes())
            .collect();
        names.sort();
        assert_eq!(names, expected);
    }
    let file = File::open(root.path().join("a")).unwrap();
    assert_eq!(
        Entries::of(&file).unwrap_err().raw_os_error(),
        Some(libc::ENOTDIR)
    );
}

#[test]
fn identity_limits_and_mask_answer_for_this_process() {
    let root = tempfile::tempdir().unwrap();
    let probe = File::create(root.path().join("owned")).unwrap();
    assert_eq!(probe.metadata().unwrap().uid(), effective_uid());
    assert_eq!(probe.metadata().unwrap().gid(), effective_gid());
    let _ = (real_uid(), real_gid());
    assert!(page_size().unwrap().is_power_of_two());
    assert!(open_max().unwrap() >= 3);
    let core = resource_limit(Resource::CoreFileSize).unwrap();
    assert!(core.rlim_cur <= core.rlim_max);
    set_resource_limit(Resource::CoreFileSize, core).unwrap();
    let again = resource_limit(Resource::CoreFileSize).unwrap();
    assert_eq!(
        (again.rlim_cur, again.rlim_max),
        (core.rlim_cur, core.rlim_max)
    );
    assert!(resource_limit(Resource::OpenFiles).unwrap().rlim_cur >= 3);
    let _ = resource_limit(Resource::FileSize).unwrap();
    let entry = account_by_uid(effective_uid()).unwrap();
    if let Some(entry) = entry {
        assert_eq!(entry.uid, effective_uid());
        if let Some(name) = &entry.name {
            assert_eq!(
                account_by_name(name).unwrap().map(|found| found.uid),
                Some(entry.uid)
            );
        }
    }
    assert_eq!(account_by_name(&c("no-such-account-\u{1}")).unwrap(), None);
    assert!(local_time(0).is_some());
}

#[test]
fn signals_reach_only_what_exists_and_a_forked_child_holds_until_released() {
    assert!(process_exists(std::process::id() as libc::pid_t));
    assert_eq!(
        kill(-1_000_000, 0).unwrap_err().raw_os_error(),
        Some(libc::ESRCH)
    );
    let (mut release, wait) = UnixStream::pair().unwrap();
    let pid = fork_child(ForkedChild::HoldUntilByte {
        close: release.as_raw_fd(),
        wait: wait.as_raw_fd(),
    })
    .unwrap();
    drop(wait);
    assert_eq!(wait_pid(pid, libc::WNOHANG).unwrap(), None, "still holding");
    assert!(process_exists(pid));
    assert_eq!(process_group(pid).unwrap(), process_group(0).unwrap());
    release.write_all(&[1]).unwrap();
    let status = reap(pid).unwrap();
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
    assert_eq!(
        wait_pid(pid, 0).unwrap_err().raw_os_error(),
        Some(libc::ECHILD)
    );

    let sleeper = fork_child(ForkedChild::SleepThenExit { seconds: 20 }).unwrap();
    kill(sleeper, libc::SIGKILL).unwrap();
    let status = reap(sleeper).unwrap();
    assert!(libc::WIFSIGNALED(status));
    assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
}

#[test]
fn a_forked_probe_sees_another_processs_lock_and_not_its_own() {
    let root = tempfile::tempdir().unwrap();
    let file = File::create(root.path().join("locked")).unwrap();
    let probe = |file: &File| {
        let pid = fork_child(ForkedChild::ProbeWriteLock {
            fd: file.as_raw_fd(),
            start: 0,
            length: 1,
        })
        .unwrap();
        libc::WEXITSTATUS(reap(pid).unwrap())
    };
    assert_eq!(probe(&file), 41, "nothing holds a lock yet");
    file.lock().unwrap();
    // flock and fcntl locks are separate on Linux and the same on macOS: either answer is a
    // completed query, never a failed one.
    assert!(matches!(probe(&file), 0 | 41));
}

#[test]
fn a_signal_set_holds_its_signals_and_a_blocked_signal_stays_pending() {
    let set = SignalSet::of(&[libc::SIGUSR2]).unwrap();
    assert!(set.contains(libc::SIGUSR2));
    assert!(!set.contains(libc::SIGUSR1));
    assert!(!SignalSet::empty().unwrap().contains(libc::SIGUSR2));
    assert_eq!(
        SignalSet::of(&[-1]).unwrap_err().raw_os_error(),
        Some(libc::EINVAL)
    );
    assert_eq!(
        default_disposition(-1).unwrap_err().raw_os_error(),
        Some(libc::EINVAL)
    );
    // Block in a thread of its own, raise at that thread, take it with sigwait.
    std::thread::spawn(move || {
        let previous = block_signals(&set).unwrap();
        assert!(!previous.contains(libc::SIGUSR2));
        // SAFETY: (test) pthread_kill on this live thread with a blocked signal.
        let raised = unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGUSR2) };
        assert_eq!(raised, 0);
        assert!(pending_signals().unwrap().contains(libc::SIGUSR2));
        assert_eq!(wait_for_signal(&set).unwrap(), libc::SIGUSR2);
        assert!(!pending_signals().unwrap().contains(libc::SIGUSR2));
        let restored = set_signal_mask(&previous).unwrap();
        assert!(restored.contains(libc::SIGUSR2));
    })
    .join()
    .unwrap();
}

#[test]
fn a_fork_with_other_threads_running_is_refused_before_anything_forks() {
    let _other = std::thread::spawn(|| std::thread::sleep(Duration::from_millis(200)));
    // Where the process adapter refuses, no thread count is known, so nothing forks.
    let refused = matches!(
        crate::process::thread_count(),
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported
    );
    let expected = if refused {
        std::io::ErrorKind::Unsupported
    } else {
        std::io::ErrorKind::WouldBlock
    };
    let error = fork_single_threaded().unwrap_err();
    assert_eq!(error.kind(), expected, "{error}");
    if !refused {
        assert!(error.to_string().contains("threads"), "{error}");
    }
    match answer_from_child(|_| unreachable!("no child runs"), Duration::from_secs(1), 8) {
        Err(ChildAnswerError::NotStarted(error)) => assert_eq!(error.kind(), expected),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_pseudo_terminal_has_settings_and_both_sides_are_close_on_exec() {
    let (controller, terminal): (OwnedFd, OwnedFd) = testing::open_pseudo_terminal().unwrap();
    assert!(is_close_on_exec(controller.as_raw_fd()).unwrap());
    assert!(is_close_on_exec(terminal.as_raw_fd()).unwrap());
    let mut settings = terminal_attributes(terminal.as_fd()).unwrap();
    settings.c_lflag &= !libc::ECHO;
    set_terminal_attributes(&terminal, &settings).unwrap();
    assert_eq!(
        terminal_attributes(&terminal).unwrap().c_lflag & libc::ECHO,
        0
    );
    let root = tempfile::tempdir().unwrap();
    let plain = File::create(root.path().join("plain")).unwrap();
    assert!(terminal_attributes(&plain).is_err());
}

#[test]
fn an_adopted_descriptor_is_a_new_copy_and_the_number_stays_open() {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    let adopted = testing::adopt_inherited(stream.as_raw_fd()).unwrap();
    assert_ne!(adopted.as_raw_fd(), stream.as_raw_fd());
    assert!(is_close_on_exec(adopted.as_raw_fd()).unwrap());
    drop(adopted);
    assert!(is_open(stream.as_raw_fd()));
    peer.write_all(b"y").unwrap();
    assert_eq!(bytes_waiting(&stream).unwrap(), 1);
    assert!(testing::adopt_inherited(-1).is_err());
}
