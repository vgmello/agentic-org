//! Which file names this system's filesystems may treat as the same.

use std::path::Path;

use org_platform::Refused;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

// Unused in a build for macOS or GNU/Linux, where only its tests call it.
#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

/// Whether this system's filesystems may take `a` and `b`, compared component by
/// component up to the shorter one, for the same names although they are spelled
/// differently. A consumer that cannot open either path uses it to keep a new claim
/// away from a path that may be the other one under another spelling.
///
/// macOS compares names without regard to case and with canonically equivalent Unicode
/// spellings equal, as APFS and HFS+ may. GNU/Linux answers `false`: names are bytes,
/// and an instance root is never on a case-folding directory
/// (`org_platform::fs::check_instance_root`). Paths of which one is a plain prefix of
/// the other are the consumer's own comparison; this function adds only the system's
/// folding.
pub fn paths_may_alias(a: &Path, b: &Path) -> Result<bool, Refused> {
    native::paths_may_alias(a, b)
}

/// Facts of this system's file names for a consumer's tests (`test-support`),
/// re-exported from `crate::testing`.
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod test_support {
    use org_platform::Refused;

    /// Whether this system's filesystems may take the composed and the decomposed
    /// Unicode spelling of a name (`é` as one code point, or as `e` with a combining
    /// accent) for the same name, as a fixed fact of the adapter: `true` on macOS,
    /// `false` on GNU/Linux, where names are bytes. A consumer's test of its own rule
    /// about such spellings expects exactly the outcome this names; it does not ask
    /// [`paths_may_alias`](super::paths_may_alias), the function its code calls. The
    /// refusing adapter refuses.
    pub fn equivalent_spellings_are_one_name() -> Result<bool, Refused> {
        super::native::equivalent_spellings_are_one_name()
    }
}

/// What every refusing adapter in this capability answers.
fn refusal() -> Refused {
    Refused::Unsupported {
        capability: "file name comparison",
        os: std::env::consts::OS,
    }
}
