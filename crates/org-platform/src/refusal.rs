//! Why a platform capability refused. Shared by every contract in the crate.

use std::fmt;
use std::io;

/// A platform capability that will not run here. A refusal is final for this build
/// and system: callers report it and stop the affected function. They never retry
/// with another system's behaviour (ADR-0017 decision 3).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refused {
    /// The build target has no adapter for the capability.
    Unsupported {
        /// The contract's module name, for example `service`.
        capability: &'static str,
        /// The build target's operating system, as `std::env::consts::OS` names it.
        os: &'static str,
    },
}

impl Refused {
    /// The refusal every refusing adapter returns: `capability` has no adapter on the
    /// operating system this build targets.
    pub fn unsupported(capability: &'static str) -> Self {
        Self::Unsupported {
            capability,
            os: std::env::consts::OS,
        }
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { capability, os } => {
                write!(f, "{capability} has no adapter on {os}")
            }
        }
    }
}

impl std::error::Error for Refused {}

/// For contracts whose functions return `io::Result`: the refusal as an
/// [`io::ErrorKind::Unsupported`] error that keeps the [`Refused`] as its source.
impl From<Refused> for io::Error {
    fn from(refused: Refused) -> Self {
        io::Error::new(io::ErrorKind::Unsupported, refused)
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::Refused;

    #[test]
    fn a_refusal_becomes_an_unsupported_io_error_that_keeps_its_reason() {
        let error = io::Error::from(Refused::unsupported("fs"));
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        let source = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<Refused>())
            .expect("the refusal is the error's source");
        assert_eq!(source, &Refused::unsupported("fs"));
        assert_eq!(
            error.to_string(),
            format!("fs has no adapter on {}", std::env::consts::OS)
        );
    }

    #[test]
    fn an_unsupported_refusal_names_the_capability_and_the_build_target() {
        let refused = Refused::unsupported("service");
        assert_eq!(
            refused,
            Refused::Unsupported {
                capability: "service",
                os: std::env::consts::OS,
            }
        );
        assert_eq!(
            refused.to_string(),
            format!("service has no adapter on {}", std::env::consts::OS)
        );
    }
}
