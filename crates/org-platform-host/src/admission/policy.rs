//! The host's policy for an image, as plain data: what a child must be, and what its
//! signing information must show.

use std::path::PathBuf;

#[cfg(doc)]
use super::own_image;

/// The image a child must be: the host's policy, as plain data.
///
/// The host builds one per program it starts (the office worker, the web process) and
/// one for itself ([`own_image`]). An adapter uses the fields its system has a check
/// for and ignores the others; it never invents a missing one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedImage {
    /// The pinned path of the executable.
    pub program: PathBuf,
    /// The SHA-256 of the executable's bytes, from the verified release manifest.
    /// The Linux rule: the bytes a child executes must have it, and the host's own
    /// executable too ([`own_image`]). `None` is admitted only with `development`,
    /// and never for the host's own image. macOS does not read it.
    pub sha256: Option<[u8; 32]>,
    /// macOS: the CodeDirectory hash the loaded image must have, from the verified
    /// release manifest. `None` pins nothing.
    pub macos_cdhash: Option<[u8; 20]>,
    /// macOS: the code requirement text the image's code object must satisfy (a
    /// Developer ID requirement). `None` is allowed only with `development`.
    pub macos_requirement: Option<String>,
    /// macOS: what the image's signing information must show, checked on the same
    /// code object as the requirement. Read only when `macos_requirement` is set.
    pub macos_signing: SigningRule,
    /// A debug build's own build output: the publisher check is skipped when no
    /// requirement (macOS) or digest (Linux) is given. A pin still applies, and
    /// refuses an image the system cannot identify.
    pub development: bool,
}

/// What signing information must show before its code is trusted, as plain data.
///
/// The names and bits are the consumer's policy. [`SigningRule::allows`] is the whole
/// meaning of a rule, on every system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigningRule {
    /// Code-signature flag bits that must all be set. Signing information without
    /// flags is refused, whatever this is.
    pub required_flags: u32,
    /// Entitlements that refuse the code when present with any value but `false`.
    pub rejected_entitlements: &'static [&'static str],
    /// Entitlements that refuse the code when present at all, whatever their value.
    pub absent_entitlements: &'static [&'static str],
}

impl SigningRule {
    /// Whether `signing` shows every required flag and none of the rejected
    /// entitlements (one that is present with any value but `false` counts). Missing
    /// flags fail closed.
    pub fn flags_and_rejections_pass(&self, signing: &Signing) -> bool {
        let Some(flags) = signing.flags else {
            return false;
        };
        if flags & self.required_flags != self.required_flags {
            return false;
        }
        !signing.entitlements.iter().flatten().any(|(key, value)| {
            self.rejected_entitlements.contains(&key.as_str()) && *value != Some(false)
        })
    }

    /// Whether `signing` holds none of the entitlements that must be absent.
    pub fn absences_pass(&self, signing: &Signing) -> bool {
        !signing
            .entitlements
            .iter()
            .flatten()
            .any(|(key, _)| self.absent_entitlements.contains(&key.as_str()))
    }

    /// The whole rule: [`Self::flags_and_rejections_pass`] and [`Self::absences_pass`].
    pub fn allows(&self, signing: &Signing) -> bool {
        self.flags_and_rejections_pass(signing) && self.absences_pass(signing)
    }
}

/// What the system reports about a program's code signature. Plain facts: the
/// consumer's [`SigningRule`] decides what they allow.
#[derive(Debug, Default)]
pub struct Signing {
    /// The code-signature flags (`kSecCodeInfoFlags`), if present.
    pub flags: Option<u32>,
    /// The entitlements (`kSecCodeInfoEntitlementsDict`): each key with its value when it's a
    /// boolean (`None` for any other kind of value). `None` when the code has no entitlements.
    pub entitlements: Option<Vec<(String, Option<bool>)>>,
    /// The `keychain-access-groups` entitlement: its groups, when it's an array of strings.
    /// `None` when it's missing or anything else.
    pub keychain_access_groups: Option<Vec<String>>,
}
