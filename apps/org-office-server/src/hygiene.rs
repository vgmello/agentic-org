//! The server's fd hygiene (`org-office-launch`, "Descriptors"): its first act in `main`, before
//! any thread or child exists. The key hand-off relies on it: the server must touch no socket it
//! did not create itself, and no exception port left by the image that exec'd it may be handed its
//! task.
//!
//! 1. fds 0, 1 and 2 must be pipes (the host always gives three: control in, events out, the log).
//! 2. every other open descriptor is closed, found by LISTING them (`proc_pidinfo` with
//!    `PROC_PIDLISTFDS`), never by counting up to a limit a parent could have lowered; listed again
//!    afterwards, any fd above 2 still open is fatal.
//! 3. the thread's and the task's exception ports are reset to none, `EXC_CRASH` and
//!    `EXC_CORPSE_NOTIFY` included at task level (the only error tolerated is the kernel not
//!    taking the corpse mask from a task).
#![allow(unsafe_code)]

/// Why the server refuses to start: one fixed word each, for the log.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Refusal {
    StdinNotAPipe,
    StdoutNotAPipe,
    StderrNotAPipe,
    OpenFdsNotListed,
    InheritedFdsNotClosed,
    ExceptionPortsNotReset,
    CorpseNotifyNotReset,
}

impl Refusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StdinNotAPipe => "stdin is not a pipe",
            Self::StdoutNotAPipe => "stdout is not a pipe",
            Self::StderrNotAPipe => "stderr is not a pipe",
            Self::OpenFdsNotListed => "the open descriptors cannot be listed",
            Self::InheritedFdsNotClosed => "an inherited descriptor could not be closed",
            Self::ExceptionPortsNotReset => "the exception ports could not be reset",
            Self::CorpseNotifyNotReset => "the corpse-notify port could not be reset",
        }
    }
}

/// The exit status of a server that refuses to start because of its descriptors (`EX_CONFIG`):
/// the only thing it says. The host reads it from the status alone. (River's contract names it
/// `org_office_launch::EXIT_LAUNCH_DESCRIPTORS_REFUSED`; use that once it is on this branch.)
pub const SILENT_REFUSAL_STATUS: i32 = 78;

/// Refuses to start: exits with [`SILENT_REFUSAL_STATUS`] and does NO I/O on any inherited
/// descriptor, not even a `failed` line on a pipe. (On Darwin a read, a write or a connect on a
/// socket, even a failed or empty one, makes the caller the peer an eavesdropper's
/// `LOCAL_PEERTOKEN` shows; a socket an early resumer put on fd 0-2 would otherwise get the
/// server as its peer and, through the host's hand-off, the secrets.)
pub fn refuse(_why: Refusal) -> ! {
    // SAFETY: `_exit` runs no destructors and touches no descriptor.
    unsafe { libc::_exit(SILENT_REFUSAL_STATUS) }
}

/// Runs the whole hygiene. Call it first in `main`.
pub fn run() -> Result<(), Refusal> {
    require_pipes()?;
    close_inherited_fds()?;
    reset_exception_ports()
}

/// fds 0, 1 and 2 must be pipes.
fn require_pipes() -> Result<(), Refusal> {
    for (fd, refusal) in [
        (0, Refusal::StdinNotAPipe),
        (1, Refusal::StdoutNotAPipe),
        (2, Refusal::StderrNotAPipe),
    ] {
        if !is_pipe(fd) {
            return Err(refusal);
        }
    }
    Ok(())
}

fn is_pipe(fd: libc::c_int) -> bool {
    // SAFETY: an all-zero `stat` is a valid out-value; `fstat` writes at most its size and fails
    // harmlessly on a closed descriptor.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `fstat` on a descriptor number with a valid out-pointer.
    unsafe { libc::fstat(fd, &mut st) == 0 && st.st_mode & libc::S_IFMT == libc::S_IFIFO }
}

/// Closes every open descriptor above 2, then checks by listing again.
fn close_inherited_fds() -> Result<(), Refusal> {
    for fd in open_fds().ok_or(Refusal::OpenFdsNotListed)? {
        if fd > 2 {
            // SAFETY: closes an inherited descriptor; nothing of this process uses it yet.
            unsafe { libc::close(fd) };
        }
    }
    match open_fds() {
        Some(fds) if fds.iter().all(|&fd| fd <= 2) => Ok(()),
        _ => Err(Refusal::InheritedFdsNotClosed),
    }
}

/// This process's open descriptors, as the kernel lists them.
pub(crate) fn open_fds() -> Option<Vec<i32>> {
    let size = std::mem::size_of::<libc::proc_fdinfo>();
    // The buffer is sized from the kernel's own hint (a call with no buffer), so a kernel that
    // lists by descriptor index cannot miss a high one.
    // SAFETY: a null buffer of size 0 asks only for the size.
    let hint = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            std::ptr::null_mut(),
            0,
        )
    };
    let mut room = usize::try_from(hint)
        .ok()
        .map_or(64, |bytes| (bytes / size).max(64));
    loop {
        // SAFETY: an all-zero `proc_fdinfo` is a valid value.
        let mut infos: Vec<libc::proc_fdinfo> = vec![unsafe { std::mem::zeroed() }; room];
        let bytes = i32::try_from(room * size).ok()?;
        // SAFETY: a buffer of exactly `bytes` bytes that outlives the call; it writes at most that.
        let n = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDLISTFDS,
                0,
                infos.as_mut_ptr().cast(),
                bytes,
            )
        };
        if n <= 0 {
            return None;
        }
        let count = usize::try_from(n).ok()? / size;
        if count < room {
            return Some(infos[..count].iter().map(|info| info.proc_fd).collect());
        }
        room *= 4;
    }
}

// `<mach/exception_types.h>`: `EXC_MASK_ALL` is bits 1-9, 11 and 12; `EXC_MASK_CRASH` is bit 10
// and `EXC_MASK_CORPSE_NOTIFY` bit 13, which `EXC_MASK_ALL` leaves out.
const EXC_MASK_ALL: u32 = 0x1bfe;
const EXC_MASK_CRASH: u32 = 0x400;
const EXC_MASK_CORPSE_NOTIFY: u32 = 0x2000;
const EXCEPTION_DEFAULT: i32 = 1;
const KERN_INVALID_ARGUMENT: libc::kern_return_t = 4;
#[cfg(target_arch = "aarch64")]
const THREAD_STATE_NONE: i32 = 5;
#[cfg(not(target_arch = "aarch64"))]
const THREAD_STATE_NONE: i32 = 13;

extern "C" {
    fn task_set_exception_ports(
        task: libc::mach_port_t,
        mask: u32,
        port: libc::mach_port_t,
        behavior: i32,
        flavor: i32,
    ) -> libc::kern_return_t;
    fn thread_set_exception_ports(
        thread: libc::mach_port_t,
        mask: u32,
        port: libc::mach_port_t,
        behavior: i32,
        flavor: i32,
    ) -> libc::kern_return_t;
    /// This task's own port (what `mach_task_self()` reads).
    static mach_task_self_: libc::mach_port_t;
    fn mach_thread_self() -> libc::mach_port_t;
    fn mach_port_deallocate(
        task: libc::mach_port_t,
        name: libc::mach_port_t,
    ) -> libc::kern_return_t;
}

/// Resets the thread's and the task's exception ports to none.
pub(crate) fn reset_exception_ports() -> Result<(), Refusal> {
    let null = libc::MACH_PORT_NULL as libc::mach_port_t;
    // SAFETY: sets this task's and this thread's exception ports to none; the ports passed are
    // this task's own, valid for the calls.
    let (thread, task, corpse) = unsafe {
        let task_self = mach_task_self_;
        let me = mach_thread_self();
        // The thread call takes `EXC_MASK_ALL` only (`EXC_CRASH` is a task-level exception).
        let thread = thread_set_exception_ports(
            me,
            EXC_MASK_ALL,
            null,
            EXCEPTION_DEFAULT,
            THREAD_STATE_NONE,
        );
        mach_port_deallocate(task_self, me);
        // The task call takes `EXC_CRASH` too: a crash port left by the pre-exec image would get
        // the server's corpse, memory included, after a crash it can cause.
        let task = task_set_exception_ports(
            task_self,
            EXC_MASK_ALL | EXC_MASK_CRASH,
            null,
            EXCEPTION_DEFAULT,
            THREAD_STATE_NONE,
        );
        let corpse = task_set_exception_ports(
            task_self,
            EXC_MASK_CORPSE_NOTIFY,
            null,
            EXCEPTION_DEFAULT,
            THREAD_STATE_NONE,
        );
        (thread, task, corpse)
    };
    if thread != 0 || task != 0 {
        return Err(Refusal::ExceptionPortsNotReset);
    }
    match corpse {
        0 | KERN_INVALID_ARGUMENT => Ok(()),
        _ => Err(Refusal::CorpseNotifyNotReset),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listing_sees_a_descriptor_this_test_opens_and_not_one_it_closed() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let fd = std::os::fd::AsRawFd::as_raw_fd(&file);
        assert!(open_fds().unwrap().contains(&fd));
        drop(file);
        assert!(!open_fds().unwrap().contains(&fd));
    }

    #[test]
    fn a_pipe_is_a_pipe_and_a_file_and_a_closed_descriptor_are_not() {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `pipe` fills two descriptors.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        assert!(is_pipe(fds[0]) && is_pipe(fds[1]));
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(!is_pipe(std::os::fd::AsRawFd::as_raw_fd(&file)));
        // SAFETY: closing the two descriptors just made.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        assert!(!is_pipe(fds[0]), "a closed descriptor");
    }

    extern "C" {
        fn mach_port_allocate(
            task: libc::mach_port_t,
            right: i32,
            name: *mut libc::mach_port_t,
        ) -> libc::kern_return_t;
        fn mach_port_insert_right(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
            poly: libc::mach_port_t,
            poly_poly: u32,
        ) -> libc::kern_return_t;
        fn task_get_exception_ports(
            task: libc::mach_port_t,
            mask: u32,
            masks: *mut u32,
            count: *mut u32,
            ports: *mut libc::mach_port_t,
            behaviors: *mut i32,
            flavors: *mut i32,
        ) -> libc::kern_return_t;
    }

    /// The ports the task has for the exceptions in `mask`, as the kernel reports them.
    fn task_ports(mask: u32) -> Vec<libc::mach_port_t> {
        let mut masks = [0u32; 32];
        let mut ports = [0 as libc::mach_port_t; 32];
        let mut behaviors = [0i32; 32];
        let mut flavors = [0i32; 32];
        let mut count = 32u32;
        // SAFETY: arrays of the declared size, this task's own port.
        let rc = unsafe {
            task_get_exception_ports(
                mach_task_self_,
                mask,
                masks.as_mut_ptr(),
                &mut count,
                ports.as_mut_ptr(),
                behaviors.as_mut_ptr(),
                flavors.as_mut_ptr(),
            )
        };
        assert_eq!(rc, 0);
        ports[..count as usize]
            .iter()
            .copied()
            .filter(|&port| port != libc::MACH_PORT_NULL as libc::mach_port_t)
            .collect()
    }

    #[test]
    fn the_exception_ports_reset_clears_a_port_the_task_had_set() {
        // A port an image before the exec could have left: set one for bad access and for a
        // crash, see the kernel report them, reset, see them gone.
        // SAFETY: allocates a receive right in this task and gives it a send right.
        let port = unsafe {
            let mut port: libc::mach_port_t = 0;
            assert_eq!(mach_port_allocate(mach_task_self_, 1, &mut port), 0);
            assert_eq!(mach_port_insert_right(mach_task_self_, port, port, 20), 0);
            port
        };
        for mask in [0x2u32, EXC_MASK_CRASH] {
            // SAFETY: this task's own port and a port right just made.
            let rc = unsafe {
                task_set_exception_ports(
                    mach_task_self_,
                    mask,
                    port,
                    EXCEPTION_DEFAULT,
                    THREAD_STATE_NONE,
                )
            };
            assert_eq!(rc, 0, "set {mask:#x}");
            assert!(!task_ports(mask).is_empty(), "the kernel reports {mask:#x}");
        }
        assert_eq!(reset_exception_ports(), Ok(()));
        assert!(task_ports(0x2).is_empty(), "bad access is reset");
        assert!(task_ports(EXC_MASK_CRASH).is_empty(), "crash is reset");
    }
}
