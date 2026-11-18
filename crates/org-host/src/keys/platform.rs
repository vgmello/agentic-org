//! The release office secrets key adapter for this build target: the Data Protection keychain on
//! macOS, and [`UnsupportedKeys`](super::unsupported::UnsupportedKeys) on every other operating
//! system, which refuses each call with a reported reason. This is the only `cfg(target_os)` in
//! key custody: callers use [`release_keys`] on every target and never branch on the platform.

use std::sync::Arc;

use super::OfficeSecretsKeys;

/// The release adapter for this build target, in the Keychain access group `group` and, with an
/// `instance_id`, scoped to that instance's items.
#[cfg_attr(debug_assertions, allow(dead_code))]
pub(super) fn release_keys(group: &str, instance_id: Option<&str>) -> Arc<dyn OfficeSecretsKeys> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(match instance_id {
            Some(id) => super::keychain::Keychain::scoped(group, id),
            None => super::keychain::Keychain::new(group),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (group, instance_id);
        Arc::new(super::unsupported::UnsupportedKeys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On macOS the release adapter is the Keychain: choosing it touches no item. On every other
    /// operating system it is the refusing adapter, never a fallback store.
    #[test]
    fn the_release_adapter_for_this_target_is_chosen_without_touching_a_store() {
        let keys = release_keys("TEAMID1234.org.agentic.host", Some("INSTANCEA"));
        #[cfg(not(target_os = "macos"))]
        assert!(matches!(
            keys.list(),
            Err(super::super::KeyStoreError::Unsupported)
        ));
        #[cfg(target_os = "macos")]
        drop(keys);
    }
}
