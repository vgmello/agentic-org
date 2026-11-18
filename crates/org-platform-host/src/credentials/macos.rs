//! macOS: the Data Protection Keychain, and the directory store.
//!
//! Host-only Data Protection Keychain custody; every query binds service and access group.
//! Immutable CFData copies are released by Core Foundation; owned [`Secret`] buffers zero on drop.
//!
//! Every item is a generic password of the store's service in its access group, and its
//! account is the item's name. It is `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`
//! and not synchronizable: it stays on this Mac (it never follows a backup restore or a
//! migration to another Mac). Code outside the access group gets `errSecItemNotFound`,
//! never a prompt.
//!
//! An add is a direct `SecItemAdd`, so it only ever adds (`errSecDuplicateItem` is
//! [`CredentialError::Exists`]). A replace updates the item's data, adds the item if
//! there is none, and updates once more if another writer added it in between. The
//! unit tests never touch the Keychain: they check the attributes, and drive the
//! operations through [`Calls`] with a recording fake. Only a signed host with the
//! entitlement can use the real store, so it is checked on a signed build.

use super::{
    directory, CredentialError, CredentialLocation, ItemName, KeychainItems, Listing, Secret,
    WriteError, MAX_ITEM_BYTES,
};
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    boolean::CFBoolean,
    data::CFData,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
use org_platform::Refused;
use security_framework_sys::{
    access_control::kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
    base::{
        errSecDuplicateItem as DUPLICATE, errSecItemNotFound as MISSING, errSecSuccess as SUCCESS,
    },
    item::{
        kSecAttrAccessGroup, kSecAttrAccount, kSecAttrService, kSecAttrSynchronizable, kSecClass,
        kSecClassGenericPassword, kSecMatchLimit, kSecMatchLimitAll, kSecReturnAttributes,
        kSecReturnData, kSecUseDataProtectionKeychain, kSecValueData,
    },
    keychain_item::{SecItemAdd, SecItemCopyMatching, SecItemDelete, SecItemUpdate},
};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::sync::Arc;

type Attributes = Vec<(CFString, CFType)>;
type Dictionary = CFDictionary<CFString, CFType>;
trait Calls: Send + Sync {
    fn add(&self, attributes: &Dictionary) -> Result<(), i32>;
    fn update(&self, query: &Dictionary, changes: &Dictionary) -> Result<(), i32>;
    fn copy_matching(&self, query: &Dictionary) -> Result<Option<CFType>, i32>;
    fn delete(&self, query: &Dictionary) -> Result<(), i32>;
}
fn dictionary(attributes: Attributes) -> Dictionary {
    CFDictionary::from_CFType_pairs(&attributes)
}
#[link(name = "Security", kind = "framework")]
extern "C" {
    /// The item attribute for when an item can be read. `security-framework-sys` exports its
    /// values but not the key itself.
    static kSecAttrAccessible: CFStringRef;
}

/// The store this system opens for a location.
pub(super) enum Store {
    Keychain(Keychain),
    Directory(directory::Store),
}

pub(super) fn open(location: CredentialLocation<'_>) -> Result<Store, CredentialError> {
    match location {
        CredentialLocation::Keychain(items) => Ok(Store::Keychain(Keychain::new(&items))),
        CredentialLocation::Directory(location) => {
            directory::Store::open(&location).map(Store::Directory)
        }
    }
}

pub(super) fn protected_store() -> Result<(), Refused> {
    Ok(())
}

/// Flush a published file to the device once more, after its directory was flushed.
pub(super) fn flush_published(file: &File) -> io::Result<()> {
    // SAFETY: retained live file; F_FULLFSYNC has no pointer argument.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

impl Store {
    pub(super) fn add(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        match self {
            Self::Keychain(store) => store.add(name, value).map_err(WriteError::from),
            Self::Directory(store) => store.add(name, value),
        }
    }

    pub(super) fn replace(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        match self {
            Self::Keychain(store) => store.replace(name, value).map_err(WriteError::from),
            Self::Directory(store) => store.replace(name, value),
        }
    }

    pub(super) fn load(&self, name: &ItemName) -> Result<Option<Secret>, CredentialError> {
        match self {
            Self::Keychain(store) => store.load(name),
            Self::Directory(store) => store.load(name),
        }
    }

    pub(super) fn delete(&self, name: &ItemName) -> Result<(), CredentialError> {
        match self {
            Self::Keychain(store) => store.delete(name),
            Self::Directory(store) => store.delete(name),
        }
    }

    pub(super) fn list(&self, is_item: &dyn Fn(&str) -> bool) -> Result<Listing, CredentialError> {
        match self {
            Self::Keychain(store) => store.list(is_item),
            Self::Directory(store) => store.list(is_item),
        }
    }
}

pub(super) struct Keychain {
    group: String,
    service: String,
    calls: Arc<dyn Calls>,
}
impl Keychain {
    fn new(items: &KeychainItems<'_>) -> Self {
        Self {
            group: items.access_group.to_owned(),
            service: items.service.to_owned(),
            calls: Arc::new(System),
        }
    }
    fn query(&self, name: Option<&ItemName>) -> Attributes {
        // SAFETY: these are immutable Security.framework CFString constants.
        unsafe {
            let c = |p| CFString::wrap_under_get_rule(p);
            let mut attrs = vec![
                (c(kSecClass), c(kSecClassGenericPassword).into_CFType()),
                (
                    c(kSecAttrService),
                    CFString::new(&self.service).into_CFType(),
                ),
                (
                    c(kSecAttrAccessGroup),
                    CFString::new(&self.group).into_CFType(),
                ),
                (
                    c(kSecUseDataProtectionKeychain),
                    CFBoolean::true_value().into_CFType(),
                ),
            ];
            if let Some(name) = name {
                attrs.push((
                    c(kSecAttrAccount),
                    CFString::new(name.as_str()).into_CFType(),
                ));
            }
            attrs
        }
    }
    fn add_policy() -> Attributes {
        // SAFETY: immutable Security.framework CFString constants.
        unsafe {
            let c = |p| CFString::wrap_under_get_rule(p);
            vec![
                (
                    c(kSecAttrAccessible),
                    c(kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly).into_CFType(),
                ),
                (
                    c(kSecAttrSynchronizable),
                    CFBoolean::false_value().into_CFType(),
                ),
            ]
        }
    }
    /// What a write sends: the item's query, the change of its data alone, and the whole
    /// item (the query, the write policy and the data) for an add.
    fn write(&self, name: &ItemName, value: &[u8]) -> (Dictionary, Dictionary, Dictionary) {
        // SAFETY: immutable Security.framework CFString constant.
        let key = unsafe { CFString::wrap_under_get_rule(kSecValueData) };
        let data = CFData::from_buffer(value).into_CFType();
        let changes = dictionary(vec![(key.clone(), data.clone())]);
        let query = dictionary(self.query(Some(name)));
        let mut add = self.query(Some(name));
        add.extend(Self::add_policy());
        add.push((key, data));
        let add = dictionary(add);
        (query, changes, add)
    }

    fn add(&self, name: &ItemName, value: &[u8]) -> Result<(), CredentialError> {
        if value.len() > MAX_ITEM_BYTES {
            return Err(CredentialError::TooLarge);
        }
        let (_, _, add) = self.write(name, value);
        // `SecItemAdd`, never an update: an existing item is refused, so a value is never replaced.
        match self.calls.add(&add) {
            Ok(()) => Ok(()),
            Err(DUPLICATE) => Err(CredentialError::Exists),
            Err(e) => Err(CredentialError::Os(e)),
        }
    }
    fn replace(&self, name: &ItemName, value: &[u8]) -> Result<(), CredentialError> {
        if value.len() > MAX_ITEM_BYTES {
            return Err(CredentialError::TooLarge);
        }
        let (query, changes, add) = self.write(name, value);
        match self.calls.update(&query, &changes) {
            Ok(()) => Ok(()),
            Err(MISSING) => match self.calls.add(&add) {
                Err(DUPLICATE) => self
                    .calls
                    .update(&query, &changes)
                    .map_err(CredentialError::Os),
                result => result.map_err(CredentialError::Os),
            },
            Err(e) => Err(CredentialError::Os(e)),
        }
    }
    fn load(&self, name: &ItemName) -> Result<Option<Secret>, CredentialError> {
        let mut query = self.query(Some(name));
        // SAFETY: immutable Security.framework CFString constant.
        query.push((
            unsafe { CFString::wrap_under_get_rule(kSecReturnData) },
            CFBoolean::true_value().into_CFType(),
        ));
        let item = match self.calls.copy_matching(&dictionary(query)) {
            Ok(Some(item)) => item,
            Err(MISSING) => return Ok(None),
            Err(status) => return Err(CredentialError::Os(status)),
            Ok(None) => return Err(CredentialError::Malformed),
        };
        let data = item
            .downcast::<CFData>()
            .ok_or(CredentialError::Malformed)?;
        if data.len() > MAX_ITEM_BYTES as isize {
            return Err(CredentialError::Corrupt);
        }
        Ok(Some(Secret::from(data.bytes().to_vec())))
    }
    fn delete(&self, name: &ItemName) -> Result<(), CredentialError> {
        match self.calls.delete(&dictionary(self.query(Some(name)))) {
            Ok(_) | Err(MISSING) => Ok(()),
            Err(status) => Err(CredentialError::Os(status)),
        }
    }
    fn list(&self, is_item: &dyn Fn(&str) -> bool) -> Result<Listing, CredentialError> {
        let mut query = self.query(None);
        // SAFETY: immutable Security.framework CFString constants.
        unsafe {
            query.push((
                CFString::wrap_under_get_rule(kSecReturnAttributes),
                CFBoolean::true_value().into_CFType(),
            ));
            query.push((
                CFString::wrap_under_get_rule(kSecMatchLimit),
                CFString::wrap_under_get_rule(kSecMatchLimitAll).into_CFType(),
            ));
        }
        let items = match self.calls.copy_matching(&dictionary(query)) {
            Ok(Some(items)) => items,
            Err(MISSING) => return Ok(Listing::default()),
            Err(status) => return Err(CredentialError::Os(status)),
            Ok(None) => return Err(CredentialError::Malformed),
        };
        let items = items
            .downcast::<CFArray>()
            .ok_or(CredentialError::Malformed)?;
        // SAFETY: immutable Security.framework CFString constant.
        let account = unsafe { CFString::wrap_under_get_rule(kSecAttrAccount) };
        let mut listing = Listing::default();
        for raw in items.iter() {
            // SAFETY: Security returns an array of retained CF objects.
            let item = unsafe { CFType::wrap_under_get_rule(*raw) };
            let attributes = item
                .downcast::<CFDictionary>()
                .ok_or(CredentialError::Malformed)?;
            let name = attributes
                .find(account.as_CFTypeRef())
                .and_then(|raw| {
                    // SAFETY: Security attributes are CF objects, not arbitrary pointers.
                    let value = unsafe { CFType::wrap_under_get_rule(*raw) };
                    value.downcast::<CFString>()
                })
                .map(|v| v.to_string())
                .filter(|name| super::valid_name(name) && is_item(name));
            match name {
                Some(name) => listing.names.push(ItemName(name)),
                None => listing.ignored += 1,
            }
        }
        listing.names.sort();
        listing.names.dedup();
        Ok(listing)
    }
}

struct System;
fn status(status: i32) -> Result<(), i32> {
    if status == SUCCESS {
        Ok(())
    } else {
        Err(status)
    }
}
impl Calls for System {
    fn add(&self, attributes: &Dictionary) -> Result<(), i32> {
        // SAFETY: dictionary lives through call; no result requested.
        status(unsafe { SecItemAdd(attributes.as_concrete_TypeRef(), std::ptr::null_mut()) })
    }
    fn update(&self, query: &Dictionary, changes: &Dictionary) -> Result<(), i32> {
        // SAFETY: both dictionaries live through call.
        status(unsafe { SecItemUpdate(query.as_concrete_TypeRef(), changes.as_concrete_TypeRef()) })
    }
    fn copy_matching(&self, query: &Dictionary) -> Result<Option<CFType>, i32> {
        let mut result = std::ptr::null();
        // SAFETY: dictionary lives through call; result receives a create-rule CF object.
        let code = unsafe { SecItemCopyMatching(query.as_concrete_TypeRef(), &mut result) };
        // SAFETY: a nonnull result is retained for the caller by SecItemCopyMatching.
        let result = (!result.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(result) });
        status(code)?;
        Ok(result)
    }
    fn delete(&self, query: &Dictionary) -> Result<(), i32> {
        // SAFETY: dictionary lives through call.
        status(unsafe { SecItemDelete(query.as_concrete_TypeRef()) })
    }
}

#[cfg(test)]
mod tests;
