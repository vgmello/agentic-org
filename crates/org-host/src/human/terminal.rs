//! Bounded secret input from the controlling terminal, with restoration before
//! pending termination/job-control signals are unblocked. Never reads stdin/env.
use org_host_control::Password;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::fd::AsRawFd,
};
struct Signals(libc::sigset_t);
impl Signals {
    fn block() -> Result<Self, String> {
        let mut set = std::mem::MaybeUninit::<libc::sigset_t>::zeroed();
        let mut old = std::mem::MaybeUninit::<libc::sigset_t>::zeroed();
        // SAFETY: `MaybeUninit::zeroed()` initializes every byte of `set` and `old`
        // (necessary on 64-bit Linux where `pthread_sigmask` writes only the 8-byte
        // kernel mask prefix of the 128-byte `libc::sigset_t`); `sigemptyset`,
        // `sigaddset`, and `pthread_sigmask` take valid writable pointers and supported
        // signal constants, and `old.assume_init()` runs only after success.
        let old = unsafe {
            if libc::sigemptyset(set.as_mut_ptr()) != 0 {
                return Err("signal mask unavailable".into());
            }
            for signal in [
                libc::SIGINT,
                libc::SIGTERM,
                libc::SIGHUP,
                libc::SIGQUIT,
                libc::SIGTSTP,
            ] {
                if libc::sigaddset(set.as_mut_ptr(), signal) != 0 {
                    return Err("signal mask unavailable".into());
                }
            }
            if libc::pthread_sigmask(libc::SIG_BLOCK, set.as_ptr(), old.as_mut_ptr()) != 0 {
                return Err("signal mask unavailable".into());
            }
            old.assume_init()
        };
        Ok(Self(old))
    }
    fn pending(&self) -> bool {
        let mut set = std::mem::MaybeUninit::<libc::sigset_t>::zeroed();
        // SAFETY: `MaybeUninit::zeroed()` initializes all bytes of `set` (including any
        // tail bytes left untouched by Linux `sigpending`); short-circuit `||` ensures
        // `sigismember` only reads `set` after `sigpending` returns 0.
        unsafe {
            libc::sigpending(set.as_mut_ptr()) != 0
                || [
                    libc::SIGINT,
                    libc::SIGTERM,
                    libc::SIGHUP,
                    libc::SIGQUIT,
                    libc::SIGTSTP,
                ]
                .iter()
                .any(|s| libc::sigismember(set.as_ptr(), *s) == 1)
        }
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        // SAFETY: restoring the initialized previous mask on the same thread.
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut()) };
    }
}
struct Restore(i32, libc::termios);
impl Drop for Restore {
    fn drop(&mut self) {
        // SAFETY: terminal descriptor outlives guard and settings came from tcgetattr.
        unsafe { libc::tcsetattr(self.0, libc::TCSANOW, &self.1) };
    }
}
pub(super) fn password() -> Result<Password, String> {
    secret(b"Password: ")
}
pub(super) fn secret(label: &[u8]) -> Result<Password, String> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| "password requires a controlling terminal")?;
    let signals = Signals::block()?;
    let mut old = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: live terminal and valid writable storage.
    if unsafe { libc::tcgetattr(tty.as_raw_fd(), old.as_mut_ptr()) } != 0 {
        return Err("terminal unavailable".into());
    }
    // SAFETY: successful tcgetattr initialized old.
    let old = unsafe { old.assume_init() };
    let mut hidden = old;
    hidden.c_lflag &= !libc::ECHO;
    // SAFETY: live terminal and initialized settings.
    if unsafe { libc::tcsetattr(tty.as_raw_fd(), libc::TCSANOW, &hidden) } != 0 {
        return Err("cannot protect password entry".into());
    }
    let restore = Restore(tty.as_raw_fd(), old);
    let mut bytes = Vec::new();
    let result = (|| {
        tty.write_all(label).map_err(|_| "terminal unavailable")?;
        tty.flush().map_err(|_| "terminal unavailable")?;
        loop {
            if signals.pending() {
                return Err("password entry cancelled");
            }
            let mut poll = libc::pollfd {
                fd: tty.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one initialized pollfd outlives poll's bounded wait.
            let ready = unsafe { libc::poll(&mut poll, 1, 100) };
            if ready < 0 {
                return Err("terminal read failed");
            }
            if ready == 0 {
                continue;
            }
            let mut b = [0];
            tty.read_exact(&mut b)
                .map_err(|_| "password input failed")?;
            if b[0] == b'\n' {
                break;
            }
            if bytes.len() >= 1024 {
                return Err("password exceeds input budget");
            }
            bytes.push(b[0]);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| "password must be UTF-8")?;
        Password::new(text.into())
    })();
    crate::secrets::zero(&mut bytes);
    drop(restore);
    let _ = tty.write_all(b"\n");
    drop(signals);
    result.map_err(str::to_owned)
}
