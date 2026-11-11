//! Host-only Data Protection Keychain custody; every query binds service and access group.
//! Immutable CFData copies are released by Core Foundation; owned Value buffers zero on drop.

use super::{Error, HostSecrets, Name, Value, MAX_BYTES};
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    boolean::CFBoolean,
    data::CFData,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
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
    static kSecAttrAccessible: CFStringRef;
}

pub(super) struct Keychain {
    group: String,
    service: String,
    calls: Arc<dyn Calls>,
}
impl Keychain {
    pub(super) fn new(group: &str, instance: &str) -> Result<Self, Error> {
        if org_proto::ulid_time_ms(instance).is_none() {
            return Err(Error::Unsafe);
        }
        Ok(Self {
            group: group.to_owned(),
            service: format!("org.agentic.host-secret.instance.{instance}"),
            calls: Arc::new(System),
        })
    }
    fn query(&self, name: Option<&Name>) -> Attributes {
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
}

impl HostSecrets for Keychain {
    fn put(&self, name: &Name, value: &Value) -> Result<(), Error> {
        if value.expose().len() > MAX_BYTES {
            return Err(Error::TooLarge);
        }
        // SAFETY: immutable Security.framework CFString constant.
        let key = unsafe { CFString::wrap_under_get_rule(kSecValueData) };
        let data = CFData::from_buffer(value.expose()).into_CFType();
        let changes = dictionary(vec![(key.clone(), data.clone())]);
        let query = dictionary(self.query(Some(name)));
        let mut add = self.query(Some(name));
        add.extend(Self::add_policy());
        add.push((key, data));
        let add = dictionary(add);
        if name.as_str().starts_with("tls.") {
            return match self.calls.add(&add) {
                Ok(()) | Err(DUPLICATE) => Ok(()),
                Err(e) => Err(Error::Os(e)),
            };
        }
        match self.calls.update(&query, &changes) {
            Ok(()) => Ok(()),
            Err(MISSING) => match self.calls.add(&add) {
                Err(DUPLICATE) => self.calls.update(&query, &changes).map_err(Error::Os),
                result => result.map_err(Error::Os),
            },
            Err(e) => Err(Error::Os(e)),
        }
    }
    fn load(&self, name: &Name) -> Result<Option<Value>, Error> {
        let mut query = self.query(Some(name));
        // SAFETY: immutable Security.framework CFString constant.
        query.push((
            unsafe { CFString::wrap_under_get_rule(kSecReturnData) },
            CFBoolean::true_value().into_CFType(),
        ));
        let item = match self.calls.copy_matching(&dictionary(query)) {
            Ok(Some(item)) => item,
            Err(MISSING) => return Ok(None),
            Err(status) => return Err(Error::Os(status)),
            Ok(None) => return Err(Error::Corrupt),
        };
        let data = item.downcast::<CFData>().ok_or(Error::Corrupt)?;
        if data.len() > MAX_BYTES as isize {
            return Err(Error::Corrupt);
        }
        Value::new(data.bytes().to_vec()).map(Some)
    }
    fn delete(&self, name: &Name) -> Result<(), Error> {
        match self.calls.delete(&dictionary(self.query(Some(name)))) {
            Ok(_) | Err(MISSING) => Ok(()),
            Err(status) => Err(Error::Os(status)),
        }
    }
    fn list(&self) -> Result<Vec<Name>, Error> {
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
            Err(MISSING) => return Ok(vec![]),
            Err(status) => return Err(Error::Os(status)),
            Ok(None) => return Err(Error::Corrupt),
        };
        let items = items.downcast::<CFArray>().ok_or(Error::Corrupt)?;
        // SAFETY: immutable Security.framework CFString constant.
        let account = unsafe { CFString::wrap_under_get_rule(kSecAttrAccount) };
        let mut names = Vec::new();
        for raw in items.iter() {
            // SAFETY: Security returns an array of retained CF objects.
            let item = unsafe { CFType::wrap_under_get_rule(*raw) };
            let attributes = item.downcast::<CFDictionary>().ok_or(Error::Corrupt)?;
            let name = attributes
                .find(account.as_CFTypeRef())
                .and_then(|raw| {
                    // SAFETY: Security attributes are CF objects, not arbitrary pointers.
                    let value = unsafe { CFType::wrap_under_get_rule(*raw) };
                    value.downcast::<CFString>()
                })
                .and_then(|v| Name::parse(&v.to_string()).ok());
            match name {
                Some(name) => names.push(name),
                None => crate::host_log::log("ignored an invalid host-secret item name"),
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
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
#[path = "keychain_tests.rs"]
mod tests;
