//! The refusing confinement adapter: nothing is confined, connected or verified, and
//! no report is accepted. Nothing here falls back to another system's mechanism.

use crate::Refused;

/// The capability every refusal names.
pub(super) const CAPABILITY: &str = "web process confinement";

pub(super) fn supported() -> Result<(), Refused> {
    Err(Refused::unsupported(CAPABILITY))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_without_an_adapter_refuses_with_the_reason() {
        let Refused::Unsupported { capability, os } = supported().unwrap_err();
        assert_eq!(capability, "web process confinement");
        assert_eq!(os, std::env::consts::OS);
    }
}
