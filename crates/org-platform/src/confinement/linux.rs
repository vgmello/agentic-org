//! GNU/Linux has no web confinement adapter yet. Increment X4 writes it (Landlock and
//! seccomp, Linux host design section 3.3). Until then every confinement function is
//! the refusing adapter's, so Linux refuses exactly as a target without an adapter
//! does.

pub(super) use super::refused::{confine_web, supported, verify_ready_report, verify_web};
// The one function Linux already has: the same `getentropy` loop as on macOS.
pub(super) use super::shared::fill_entropy;

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;

    use crate::confinement::{self, Enforcement};
    use crate::Refused;

    const REFUSAL: &str =
        "web process confinement is not available on linux in this release; nothing was changed";

    fn assert_refused<T>(result: io::Result<T>) {
        let error = result.err().expect("Linux has no adapter yet");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert_eq!(error.to_string(), REFUSAL);
    }

    #[test]
    fn linux_refuses_until_its_adapter_is_written() {
        assert_eq!(
            confinement::web_confinement_supported(),
            Err(Refused::unsupported("web process confinement"))
        );
        let report = Enforcement {
            landlock_abi: Some(3),
            seccomp: true,
        };
        assert_refused(confinement::verify_ready_report(&report));
        assert_refused(confinement::verify_web(
            std::process::id(),
            &report,
            &[Path::new("/")],
        ));
        assert_refused(confinement::confine_web(Path::new(
            "/nonexistent/rendezvous",
        )));
    }
}
