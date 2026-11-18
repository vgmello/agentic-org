//! What a code signature must show before the host trusts a program with a secret: the desktop
//! app (`app_auth`, an app token), its office servers (`server_check`, a database key), and the
//! host itself (`keys`, its Keychain access group).
//!
//! This is the policy: the Developer ID requirement text, the hardened-runtime flag and the
//! rejected entitlements. Signatures are read and checked by the platform's admission adapter
//! (`org_platform_host::admission`), which takes this policy as data.
//!
//! Everything here fails closed: an error is "no".

use crate::peer::Identity;
use org_platform_host::admission::SigningRule;

/// The requirement for code signed as `identifier` with the Developer ID of team `team_id`
/// ([`DEVELOPER_ID`]). The identifier must be a reverse-DNS name (ASCII letters, digits, `.`
/// and `-`) and the team id exactly 10 uppercase ASCII letters or digits, so neither can change
/// the requirement's meaning:
///
/// ```text
/// identifier "<identifier>" and anchor apple generic
///   and certificate 1[field.1.2.840.113635.100.6.2.6] exists
///   and certificate leaf[field.1.2.840.113635.100.6.1.13] exists
///   and certificate leaf[subject.OU] = "<team id>"
/// ```
pub(crate) fn developer_id_requirement(identifier: &str, team_id: &str) -> Result<String, String> {
    let identifier_ok = !identifier.is_empty()
        && identifier.len() <= 255
        && !identifier.starts_with(['.', '-'])
        && identifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if !identifier_ok {
        return Err(format!(
            "the signing identifier {identifier:?} isn't well formed"
        ));
    }
    let team_ok = team_id.len() == 10
        && team_id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if !team_ok {
        return Err(format!(
            "the team id {team_id:?} isn't 10 uppercase letters or digits"
        ));
    }
    Ok(format!(
        "identifier \"{identifier}\" and anchor apple generic and {DEVELOPER_ID} and \
         certificate leaf[subject.OU] = \"{team_id}\""
    ))
}

/// The Developer ID markers: the signing certificate is a Developer ID Application certificate
/// (leaf extension 1.2.840.113635.100.6.1.13) issued by the Developer ID intermediate
/// (1.2.840.113635.100.6.2.6). So only a notarizable Developer ID signature passes, not
/// a development or distribution certificate of the same team.
pub(crate) const DEVELOPER_ID: &str = "certificate 1[field.1.2.840.113635.100.6.2.6] exists and \
                            certificate leaf[field.1.2.840.113635.100.6.1.13] exists";

/// The code-signature flag of the hardened runtime (`kSecCodeSignatureRuntime`).
pub(crate) const HARDENED_RUNTIME: u32 = 0x0001_0000;

/// Entitlements that let another process or library reach into a program (a debugger, injected
/// libraries, `DYLD_*` variables): an app or an office server with any of them could be made to
/// hand its secrets over, so it isn't trusted with one.
pub(crate) const REJECTED_ENTITLEMENTS: [&str; 3] = [
    "com.apple.security.get-task-allow",
    "com.apple.security.cs.allow-dyld-environment-variables",
    "com.apple.security.cs.disable-library-validation",
];

/// What the host reads from a program's signing information: its code-signature flags, its
/// entitlements and its Keychain access groups, as the platform reports them.
pub(crate) use org_platform_host::admission::Signing;

/// The rule every program trusted with a secret must meet, as the platform applies it: the
/// hardened runtime and none of the [`REJECTED_ENTITLEMENTS`].
pub(crate) const HARDENED: SigningRule = SigningRule {
    required_flags: HARDENED_RUNTIME,
    rejected_entitlements: &REJECTED_ENTITLEMENTS,
    absent_entitlements: &[],
};

/// Whether signed code may get a secret (an app token, a database key): it runs with the hardened runtime and has none of
/// the [`REJECTED_ENTITLEMENTS`] (one that's present with any value but `false` counts). Missing
/// flags fail closed.
pub(crate) fn hardened_and_clean(signing: &Signing) -> bool {
    HARDENED.flags_and_rejections_pass(signing)
}

/// Whether the process `peer` names satisfies `requirement` and runs hardened code without the
/// [`REJECTED_ENTITLEMENTS`], all checked on one code object. Fails closed: `false` on any
/// error, and always on a system without code signatures.
pub(crate) fn satisfies_hardened(peer: &Identity, requirement: &str) -> bool {
    peer.satisfies(requirement, &HARDENED)
}

/// This process's own signing information (through `SecCodeCopySelf`): its flags (the
/// hardened runtime) and its entitlements (`keychain-access-groups` among them). `None` when it
/// can't be read. An unsigned test binary has no hardened runtime and no access groups, so the
/// live answer a release host needs can only be seen on a signed build.
pub(crate) fn own_signing() -> Option<Signing> {
    org_platform_host::admission::own_signing()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_developer_id_requirement_names_the_identifier_and_team() {
        assert_eq!(
            developer_id_requirement("com.vgmello.agentic-org.office", "AB12CD34EF").unwrap(),
            "identifier \"com.vgmello.agentic-org.office\" and anchor apple generic \
             and certificate 1[field.1.2.840.113635.100.6.2.6] exists and \
             certificate leaf[field.1.2.840.113635.100.6.1.13] exists and \
             certificate leaf[subject.OU] = \"AB12CD34EF\""
        );
        for team in ["", "ab12cd34ef", "AB12CD34E", "AB12CD34EFG", "AB12CD34\"F"] {
            assert!(developer_id_requirement("org.x", team).is_err(), "{team}");
        }
        for identifier in ["", ".org.x", "org.x\" or anchor apple", "org x", "org.é"] {
            assert!(
                developer_id_requirement(identifier, "AB12CD34EF").is_err(),
                "{identifier}"
            );
        }
    }

    fn entitlements(pairs: &[(&str, Option<bool>)]) -> Option<Vec<(String, Option<bool>)>> {
        Some(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    }

    #[test]
    fn only_hardened_code_without_the_rejected_entitlements_passes() {
        let hardened = Some(HARDENED_RUNTIME | 0x2);
        let ok = |flags, entitlements| {
            hardened_and_clean(&Signing {
                flags,
                entitlements,
                keychain_access_groups: None,
            })
        };
        assert!(ok(hardened, None));
        assert!(ok(
            hardened,
            entitlements(&[
                ("com.apple.security.network.client", Some(true)),
                ("com.apple.security.get-task-allow", Some(false)),
            ])
        ));
        // No hardened runtime, or no flags at all.
        assert!(!ok(Some(0x2), None));
        assert!(!ok(None, None));
        // Each rejected entitlement, set true or with a value that isn't a boolean.
        for key in REJECTED_ENTITLEMENTS {
            assert!(!ok(hardened, entitlements(&[(key, Some(true))])), "{key}");
            assert!(!ok(hardened, entitlements(&[(key, None)])), "{key}");
        }
    }
}
