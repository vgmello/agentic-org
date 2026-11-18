//! The refusing confinement adapter: nothing is confined, connected or verified, and
//! no report is accepted. Nothing here falls back to another system's mechanism.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

use super::Enforcement;
use crate::Refused;

/// The capability every refusal names.
pub(super) const CAPABILITY: &str = "web process confinement";

pub(super) fn supported() -> Result<(), Refused> {
    Err(Refused::unsupported(CAPABILITY))
}

pub(super) fn confine_web(_rendezvous: &Path) -> io::Result<(UnixStream, Enforcement)> {
    Err(Refused::unsupported(CAPABILITY).into())
}

pub(super) fn verify_ready_report(_enforcement: &Enforcement) -> io::Result<()> {
    Err(Refused::unsupported(CAPABILITY).into())
}

pub(super) fn verify_web(
    _pid: u32,
    _enforcement: &Enforcement,
    _critical: &[&Path],
) -> io::Result<()> {
    Err(Refused::unsupported(CAPABILITY).into())
}

pub(super) fn fill_entropy(_bytes: &mut [u8]) -> io::Result<()> {
    Err(Refused::unsupported(CAPABILITY).into())
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use super::*;

    fn assert_refused<T>(result: io::Result<T>) {
        let error = result.err().expect("the refusing adapter never succeeds");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        let reason = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<Refused>())
            .expect("the refusal is the error's inner error");
        assert_eq!(reason, &Refused::unsupported(CAPABILITY));
    }

    const REPORTS: [Enforcement; 3] = [
        Enforcement {
            landlock_abi: None,
            seccomp: false,
        },
        Enforcement {
            landlock_abi: Some(3),
            seccomp: true,
        },
        Enforcement {
            landlock_abi: Some(7),
            seccomp: false,
        },
    ];

    #[test]
    fn a_target_without_an_adapter_refuses_with_the_reason() {
        let Refused::Unsupported { capability, os } = supported().unwrap_err();
        assert_eq!(capability, "web process confinement");
        assert_eq!(os, std::env::consts::OS);
        for report in &REPORTS {
            assert_refused(verify_ready_report(report));
        }
    }

    #[test]
    fn nothing_connects_to_the_rendezvous() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rendezvous");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        assert_refused(confine_web(&path));
        assert_refused(confine_web(Path::new("relative")));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock,
            "the refusing adapter connected to the rendezvous"
        );
    }

    #[test]
    fn no_entropy_is_written() {
        let mut bytes = [0u8; 300];
        assert_refused(fill_entropy(&mut bytes));
        assert_eq!(bytes, [0u8; 300]);
        assert_refused(fill_entropy(&mut []));
    }

    #[test]
    fn no_child_is_verified_whatever_it_reports() {
        let own = std::process::id();
        let root = Path::new("/");
        for report in &REPORTS {
            assert_refused(verify_web(own, report, &[root]));
            assert_refused(verify_web(own, report, &[]));
            assert_refused(verify_web(0, report, &[root]));
        }
    }
}
