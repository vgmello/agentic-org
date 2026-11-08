//! The release key store: the Data Protection keychain, in the host's own access group.
//!
//! Each office's key is a generic password, service [`SERVICE`], account = the office id, in the
//! access group from the host's `keychain-access-groups` entitlement. It is
//! `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` and not synchronizable: it stays on this Mac
//! (it never follows a backup restore or a migration to another Mac). Code outside the access
//! group gets `errSecItemNotFound`, never a prompt.
//!
//! `create` is a direct `SecItemAdd`, so it only ever adds (`errSecDuplicateItem` is
//! [`KeyStoreError::Exists`]); `load` and `delete` go through `security_framework::passwords`.
//! Every `Vec` a key passes through is zeroed. The copies inside Core Foundation (the `CFData`
//! the key is added from or read into) are freed by Core Foundation and can't be zeroed; reading
//! them needs local memory access as the same user.
//!
//! The unit tests never touch the Keychain: they check [`item_attributes`], a pure function.
//! Only a signed host with the entitlement can use this store, so it's checked on a signed
//! build (see the host-keys runbook).

// Debug builds never choose this store (`keys::store_for`); its tests still run there.
#![cfg_attr(debug_assertions, allow(dead_code))]

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use org_office_launch::is_plain_id;
use org_proto::{DbKey, DB_KEY_BYTES};
use security_framework::passwords::{self, PasswordOptions};
use security_framework_sys::access_control::kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly;
use security_framework_sys::base::{errSecDuplicateItem, errSecItemNotFound, errSecSuccess};
use security_framework_sys::item::{
    kSecAttrAccessGroup, kSecAttrAccount, kSecAttrService, kSecAttrSynchronizable, kSecClass,
    kSecClassGenericPassword, kSecUseDataProtectionKeychain, kSecValueData,
};
use security_framework_sys::keychain_item::SecItemAdd;

use super::{KeyStore, KeyStoreError};
use crate::secrets::zero;

/// The service every office key is filed under (ADR-0002); the account is the office id. It is
/// persisted state: changing it later orphans every office's key. (The server's former items used
/// `org.agentic.office`; this store never reads them.)
pub(crate) const SERVICE: &str = "org.agentic.office-key";

#[link(name = "Security", kind = "framework")]
extern "C" {
    /// The item attribute for when an item can be read. `security-framework-sys` exports its
    /// values but not the key itself.
    static kSecAttrAccessible: CFStringRef;
}

/// The Data Protection keychain, in one access group.
pub(crate) struct Keychain {
    group: String,
}

impl Keychain {
    /// The store for the access group `group` (one of the host's `keychain-access-groups`).
    pub(crate) fn new(group: &str) -> Self {
        Self {
            group: group.to_owned(),
        }
    }

    /// The query for office `office_id`'s item: its class, service and account, in this access
    /// group, in the Data Protection keychain.
    fn options(&self, office_id: &str) -> PasswordOptions {
        let mut options = PasswordOptions::new_generic_password(SERVICE, office_id);
        options.use_protected_keychain();
        options.set_access_group(&self.group);
        options
    }
}

/// The attributes of office `office_id`'s item in access group `group`, without its data: a
/// generic password, [`SERVICE`], account = the office id, the Data Protection keychain, the access
/// group, readable after the first unlock on this device only, and not synchronizable. Pure: it
/// builds Core Foundation values and never calls the Keychain.
pub(crate) fn item_attributes(office_id: &str, group: &str) -> Vec<(CFString, CFType)> {
    // SAFETY: every `kSec…` here is an immutable CFString constant that Security.framework
    // exports for the life of the process; `wrap_under_get_rule` retains it.
    unsafe {
        let constant = |name: CFStringRef| CFString::wrap_under_get_rule(name);
        vec![
            (
                constant(kSecClass),
                constant(kSecClassGenericPassword).into_CFType(),
            ),
            (
                constant(kSecAttrService),
                CFString::new(SERVICE).into_CFType(),
            ),
            (
                constant(kSecAttrAccount),
                CFString::new(office_id).into_CFType(),
            ),
            (
                constant(kSecUseDataProtectionKeychain),
                CFBoolean::true_value().into_CFType(),
            ),
            (
                constant(kSecAttrAccessGroup),
                CFString::new(group).into_CFType(),
            ),
            (
                constant(kSecAttrAccessible),
                constant(kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly).into_CFType(),
            ),
            (
                constant(kSecAttrSynchronizable),
                CFBoolean::false_value().into_CFType(),
            ),
        ]
    }
}

impl KeyStore for Keychain {
    fn create(&self, office_id: &str, key: &DbKey) -> Result<(), KeyStoreError> {
        if !is_plain_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        let mut attributes = item_attributes(office_id, &self.group);
        // SAFETY: `kSecValueData` is an immutable constant (as in `item_attributes`).
        let value_key = unsafe { CFString::wrap_under_get_rule(kSecValueData) };
        attributes.push((
            value_key,
            CFData::from_buffer(key.expose_bytes()).into_CFType(),
        ));
        let item = CFDictionary::from_CFType_pairs(&attributes);
        // `SecItemAdd`, never an update: an existing item is refused, so a key is never replaced.
        // SAFETY: `item` is a valid dictionary that outlives the call; a null result pointer asks
        // for no result.
        let status = unsafe { SecItemAdd(item.as_concrete_TypeRef(), std::ptr::null_mut()) };
        if status == errSecSuccess {
            Ok(())
        } else if status == errSecDuplicateItem {
            Err(KeyStoreError::Exists)
        } else {
            Err(KeyStoreError::Os(status))
        }
    }

    fn load(&self, office_id: &str) -> Result<Option<Box<DbKey>>, KeyStoreError> {
        if !is_plain_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        match passwords::generic_password(self.options(office_id)) {
            Ok(mut data) => {
                let key = <&[u8; DB_KEY_BYTES]>::try_from(data.as_slice())
                    .map(|bytes| Box::new(DbKey::from_bytes(*bytes)))
                    .map_err(|_| KeyStoreError::Corrupt);
                zero(&mut data);
                key.map(Some)
            }
            Err(e) if e.code() == errSecItemNotFound => Ok(None),
            // The OSStatus only: never a message built from the item.
            Err(e) => Err(KeyStoreError::Os(e.code())),
        }
    }

    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError> {
        if !is_plain_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        match passwords::delete_generic_password_options(self.options(office_id)) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == errSecItemNotFound => Ok(()),
            Err(e) => Err(KeyStoreError::Os(e.code())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
    const GROUP: &str = "TEAMID1234.org.agentic.host";

    /// The value of `name` in `attributes`, if it's there once.
    fn value(attributes: &[(CFString, CFType)], name: CFStringRef) -> CFType {
        // SAFETY: an immutable framework constant (see `item_attributes`).
        let name = unsafe { CFString::wrap_under_get_rule(name) };
        let found: Vec<&CFType> = attributes
            .iter()
            .filter(|(key, _)| *key == name)
            .map(|(_, value)| value)
            .collect();
        assert_eq!(found.len(), 1, "{name} appears {} times", found.len());
        found[0].clone()
    }

    fn string(value: &CFType) -> String {
        value.downcast::<CFString>().expect("a string").to_string()
    }

    #[test]
    fn the_service_is_the_one_the_adr_fixes() {
        // Persisted state: changing it orphans every office's key (ADR-0002).
        assert_eq!(SERVICE, "org.agentic.office-key");
    }

    // SAFETY (each `unsafe` below): reads an immutable framework constant.
    #[test]
    fn an_item_is_a_generic_password_for_this_office_in_the_data_protection_keychain() {
        let attributes = item_attributes(A, GROUP);
        assert_eq!(attributes.len(), 7);
        let class = value(&attributes, unsafe { kSecClass });
        assert_eq!(string(&class), unsafe {
            CFString::wrap_under_get_rule(kSecClassGenericPassword).to_string()
        });
        assert_eq!(
            string(&value(&attributes, unsafe { kSecAttrService })),
            SERVICE
        );
        assert_eq!(string(&value(&attributes, unsafe { kSecAttrAccount })), A);
        assert_eq!(
            string(&value(&attributes, unsafe { kSecAttrAccessGroup })),
            GROUP
        );
        assert_eq!(
            value(&attributes, unsafe { kSecUseDataProtectionKeychain }),
            CFBoolean::true_value().into_CFType()
        );
    }

    #[test]
    fn an_item_stays_on_this_mac_and_is_readable_after_the_first_unlock() {
        let attributes = item_attributes(A, GROUP);
        let accessible = value(&attributes, unsafe { kSecAttrAccessible });
        assert_eq!(string(&accessible), unsafe {
            CFString::wrap_under_get_rule(kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly)
                .to_string()
        });
        assert_eq!(
            value(&attributes, unsafe { kSecAttrSynchronizable }),
            CFBoolean::false_value().into_CFType()
        );
    }

    #[test]
    fn the_attributes_never_carry_a_key() {
        let attributes = item_attributes(A, GROUP);
        // SAFETY: an immutable framework constant.
        let data = unsafe { CFString::wrap_under_get_rule(kSecValueData) };
        assert!(attributes.iter().all(|(key, _)| *key != data));
    }

    #[test]
    fn ids_that_arent_plain_are_refused_before_the_keychain() {
        let store = Keychain::new(GROUP);
        let key = DbKey::from_bytes([1; 32]);
        for id in ["", "../x", "a b"] {
            assert!(matches!(
                store.create(id, &key),
                Err(KeyStoreError::NotPlainId)
            ));
            assert!(matches!(store.load(id), Err(KeyStoreError::NotPlainId)));
            assert!(matches!(store.delete(id), Err(KeyStoreError::NotPlainId)));
        }
    }
}
