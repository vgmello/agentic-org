//! What a code signature must show before the host trusts a program with a secret: the desktop
//! app (`app_auth`, its `admin` and `recovery` calls), its office servers (`server_check`, an
//! office secrets key), and the host itself (`keys`, its Keychain access group).
//!
//! Pure Developer ID requirement and hardened-runtime rules live in `org_host_release::signing`
//! so release admission and host checks share one policy.

use crate::peer::Identity;

pub(crate) use org_host_release::signing::{
    developer_id_requirement, hardened_and_clean, HARDENED,
};
#[cfg(test)]
pub(crate) use org_host_release::signing::{HARDENED_RUNTIME, REJECTED_ENTITLEMENTS};
pub(crate) use org_platform_host::admission::Signing;

/// Whether the process `peer` names satisfies `requirement` and runs hardened code without the
/// rejected debugging/injection entitlements, all checked on one code object. Fails closed:
/// `false` on any error, and always on a system without code signatures.
pub(crate) fn satisfies_hardened(peer: &Identity, requirement: &str) -> bool {
    peer.satisfies(requirement, &HARDENED)
}

/// This process's own signing information (through `SecCodeCopySelf`): its flags (the
/// hardened runtime) and its entitlements (`keychain-access-groups` among them). `None` when it
/// can't be read.
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
        assert!(!ok(Some(0x2), None));
        assert!(!ok(None, None));
        for key in REJECTED_ENTITLEMENTS {
            assert!(!ok(hardened, entitlements(&[(key, Some(true))])), "{key}");
            assert!(!ok(hardened, entitlements(&[(key, None)])), "{key}");
        }
    }
}
