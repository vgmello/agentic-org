//! Why a platform capability refused. Shared by every contract in the crate.

use std::fmt;

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

#[cfg(test)]
mod tests {
    use super::Refused;

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
