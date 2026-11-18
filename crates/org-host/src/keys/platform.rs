//! The release office secrets key adapter for this build target, with its preconditions: the
//! Data Protection keychain on macOS, and [`UnsupportedKeys`](super::unsupported::UnsupportedKeys)
//! on every other operating system, which refuses each call with a reported reason. This is the
//! only `cfg(target_os)` in key custody: callers use [`release_keys`] on every target and never
//! branch on the platform.

use std::sync::Arc;

#[cfg(target_os = "macos")]
use crate::code_signing::hardened_and_clean;
use crate::code_signing::Signing;

use super::{OfficeSecretsKeys, StoreError};

/// The release adapter for this build target.
///
/// On macOS: the Keychain in the access group `group` and, with an `instance_id`, scoped to that
/// instance's items, once the group is present ([`StoreError::NoKeychainGroup`] otherwise) and
/// the host's own signing information (`own_signing`, read only after the group check) shows it
/// may use that group and can be trusted with the keys ([`self_trusted`]).
///
/// On every other operating system: the refusing adapter, chosen before any Keychain check, so
/// the operator is told this operating system has no office secrets key store rather than a
/// macOS-only reason. `group` and `own_signing` are not consulted there.
#[cfg_attr(debug_assertions, allow(dead_code))]
pub(super) fn release_keys(
    group: Option<&str>,
    instance_id: Option<&str>,
    own_signing: impl FnOnce() -> Option<Signing>,
) -> Result<Arc<dyn OfficeSecretsKeys>, StoreError> {
    #[cfg(target_os = "macos")]
    {
        let group = match group {
            Some(group) if !group.trim().is_empty() => group,
            _ => return Err(StoreError::NoKeychainGroup),
        };
        self_trusted(group, own_signing().as_ref())?;
        Ok(Arc::new(match instance_id {
            Some(id) => super::keychain::Keychain::scoped(group, id),
            None => super::keychain::Keychain::new(group),
        }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (group, instance_id, own_signing);
        Ok(Arc::new(super::unsupported::UnsupportedKeys))
    }
}

/// The decision, pure: a host whose own signing information is `own` may keep keys in the
/// access group `group` only if that group is in its `keychain-access-groups`, and it runs
/// hardened with none of the entitlements that let other code in
/// (`code_signing::hardened_and_clean`). Information that couldn't be read trusts nothing.
#[cfg(target_os = "macos")]
#[cfg_attr(debug_assertions, allow(dead_code))]
fn self_trusted(group: &str, own: Option<&Signing>) -> Result<(), StoreError> {
    // Its own signature can't be read: nothing about it can be trusted.
    let Some(own) = own else {
        return Err(StoreError::NotHardened);
    };
    let entitled = own.keychain_access_groups.as_deref();
    if !group_entitled(group, entitled) {
        return Err(StoreError::KeychainGroupNotEntitled {
            group: group.to_owned(),
        });
    }
    if !hardened_and_clean(own) {
        return Err(StoreError::NotHardened);
    }
    Ok(())
}

/// Whether the access group `group` is one of the host's own `entitled` groups (exactly). No
/// entitlement, or one that couldn't be read, entitles nothing.
#[cfg(target_os = "macos")]
#[cfg_attr(debug_assertions, allow(dead_code))]
fn group_entitled(group: &str, entitled: Option<&[String]>) -> bool {
    entitled.is_some_and(|groups| groups.iter().any(|g| g == group))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On macOS the release adapter is the Keychain: choosing it touches no item. On every other
    /// operating system it is the refusing adapter, never a fallback store.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_release_adapter_for_this_target_is_chosen_without_touching_a_store() {
        let own = Signing {
            flags: Some(crate::code_signing::HARDENED_RUNTIME),
            entitlements: Some(Default::default()),
            keychain_access_groups: Some(vec!["TEAMID1234.org.agentic.host".to_owned()]),
        };
        let keys = release_keys(
            Some("TEAMID1234.org.agentic.host"),
            Some("INSTANCEA"),
            || Some(own),
        );
        assert!(keys.is_ok());
    }

    /// Another operating system gets the refusing adapter whatever the group and signing: no
    /// macOS reason (a missing group, an unsigned host) is ever reported there, and the signing
    /// information is never read.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn another_operating_system_gets_the_refusing_adapter_before_any_keychain_check() {
        for group in [None, Some(""), Some("TEAMID1234.org.agentic.host")] {
            let mut asked = false;
            let keys = release_keys(group, Some("INSTANCEA"), || {
                asked = true;
                None
            })
            .unwrap_or_else(|e| panic!("{group:?}: {e:?}"));
            assert!(!asked, "the signing information was read");
            assert!(matches!(
                keys.list(),
                Err(super::super::KeyStoreError::Unsupported)
            ));
            assert!(matches!(
                keys.load("01JAAAAAAAAAAAAAAAAAAAAAAA"),
                Err(super::super::KeyStoreError::Unsupported)
            ));
        }
    }

    /// The macOS preconditions: the access group, its entitlement and the host's own hardening.
    #[cfg(target_os = "macos")]
    mod keychain_preconditions {
        use super::super::*;

        /// The release adapter for `group`, unscoped.
        fn release_keys_unscoped(
            group: Option<&str>,
            own_signing: impl FnOnce() -> Option<Signing>,
        ) -> Result<Arc<dyn OfficeSecretsKeys>, StoreError> {
            release_keys(group, None, own_signing)
        }

        const GROUP: &str = "TEAMID1234.org.agentic.host";
        const HARDENED: u32 = crate::code_signing::HARDENED_RUNTIME;

        /// Signing information with these `flags`, access `groups` and boolean `entitlements`.
        fn signing(flags: Option<u32>, groups: &[&str], entitlements: &[(&str, bool)]) -> Signing {
            Signing {
                flags,
                entitlements: Some(
                    entitlements
                        .iter()
                        .map(|(key, value)| ((*key).to_owned(), Some(*value)))
                        .collect(),
                ),
                keychain_access_groups: Some(groups.iter().map(|g| (*g).to_owned()).collect()),
            }
        }

        #[test]
        fn a_release_build_without_an_access_group_has_no_store() {
            let mut asked = false;
            for group in [None, Some(""), Some("  ")] {
                let store = release_keys_unscoped(group, || {
                    asked = true;
                    Some(signing(Some(HARDENED), &[GROUP], &[]))
                });
                assert!(
                    matches!(store, Err(StoreError::NoKeychainGroup)),
                    "{group:?}"
                );
            }
            assert!(!asked, "the signing information was read without a group");
        }

        #[test]
        fn a_release_build_needs_its_access_group_in_its_own_entitlement() {
            for groups in [
                vec![],
                vec!["TEAMID1234.other"],
                vec![
                    "TEAMID1234.org.agentic.host.x",
                    "teamid1234.org.agentic.host",
                ],
            ] {
                let own = signing(Some(HARDENED), &groups, &[]);
                let err = self_trusted(GROUP, Some(&own)).unwrap_err();
                assert_eq!(
                    err,
                    StoreError::KeychainGroupNotEntitled {
                        group: GROUP.to_owned()
                    },
                    "{groups:?}"
                );
            }
            // Signing information that couldn't be read trusts nothing.
            assert_eq!(self_trusted(GROUP, None), Err(StoreError::NotHardened));
            let entitled = signing(Some(HARDENED), &["TEAMID1234.other", GROUP], &[]);
            assert_eq!(self_trusted(GROUP, Some(&entitled)), Ok(()));
            // Choosing the store touches no Keychain item.
            assert!(release_keys_unscoped(Some(GROUP), || Some(entitled)).is_ok());
        }

        /// M1: an entitled host that isn't hardened, or lets other code in, could give every key
        /// away: refused.
        #[test]
        fn a_release_build_must_itself_be_hardened_and_closed_to_other_code() {
            let entitled =
                |flags, entitlements: &[(&str, bool)]| signing(flags, &[GROUP], entitlements);
            for own in [
                entitled(None, &[]),
                entitled(Some(0x2), &[]),
                entitled(
                    Some(HARDENED),
                    &[(
                        "com.apple.security.cs.allow-dyld-environment-variables",
                        true,
                    )],
                ),
                entitled(
                    Some(HARDENED),
                    &[("com.apple.security.cs.disable-library-validation", true)],
                ),
                entitled(
                    Some(HARDENED),
                    &[("com.apple.security.get-task-allow", true)],
                ),
            ] {
                assert_eq!(
                    self_trusted(GROUP, Some(&own)),
                    Err(StoreError::NotHardened),
                    "{own:?}"
                );
                assert!(matches!(
                    release_keys_unscoped(Some(GROUP), || Some(own)),
                    Err(StoreError::NotHardened)
                ));
            }
            let clean = entitled(
                Some(HARDENED | 0x2),
                &[("com.apple.security.get-task-allow", false)],
            );
            assert_eq!(self_trusted(GROUP, Some(&clean)), Ok(()));
        }
    }
}
