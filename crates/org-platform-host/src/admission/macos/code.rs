//! Reading and checking code signatures through the Security framework, for the image check of
//! a started child, a connection's peer, an executable file and this process itself.
//!
//! Everything here fails closed: an error is "no".

use std::fs::File;

use super::AuditToken;
use crate::admission::{Signing, SigningRule, StaticImageError};

/// The entitlement that lists the Keychain access groups a signed program may use.
pub(super) const KEYCHAIN_ACCESS_GROUPS: &str = "keychain-access-groups";

/// Whether the process `token` names satisfies `requirement` and shows the flags of `rule`
/// without its rejected entitlements, all checked on one code object. Fails closed.
pub(super) fn satisfies_hardened(
    token: &AuditToken,
    requirement: &str,
    rule: &SigningRule,
) -> bool {
    code_for_audit_token(token).is_some_and(|code| {
        meets(&code, requirement)
            && signing(&code).is_some_and(|s| rule.flags_and_rejections_pass(&s))
    })
}

/// Whether the code of the process `token` names holds none of the entitlements `absent`,
/// whatever their value. Fails closed: code whose signing information can't be read holds them.
pub(super) fn without_entitlements(token: &AuditToken, absent: &[&str]) -> bool {
    code_for_audit_token(token)
        .and_then(|code| signing(&code))
        .is_some_and(|s| {
            !s.entitlements
                .iter()
                .flatten()
                .any(|(key, _)| absent.contains(&key.as_str()))
        })
}

/// The code object of the process `token` names. `None` on any error, including a token whose
/// process has exited or `exec`ed since.
pub(super) fn code_for_audit_token(
    token: &AuditToken,
) -> Option<security_framework::os::macos::code_signing::SecCode> {
    use core_foundation::base::TCFType;
    use core_foundation::data::CFData;
    use security_framework::os::macos::code_signing::{Flags, GuestAttributes, SecCode};

    let token = CFData::from_buffer(token);
    let mut attributes = GuestAttributes::new();
    attributes.set_audit_token(token.as_concrete_TypeRef());
    SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE).ok()
}

/// Whether `code` is valid and satisfies `requirement`. Fails closed.
pub(super) fn meets(
    code: &security_framework::os::macos::code_signing::SecCode,
    requirement: &str,
) -> bool {
    use security_framework::os::macos::code_signing::{Flags, SecRequirement};
    use std::str::FromStr;

    SecRequirement::from_str(requirement)
        .is_ok_and(|requirement| code.check_validity(Flags::NONE, &requirement).is_ok())
}

/// Whether the process `token` names satisfies `requirement` (no hardening check: what the
/// plumbing test can show on an unsigned-by-Developer-ID test binary).
#[cfg(test)]
pub(super) fn code_satisfies(token: &AuditToken, requirement: &str) -> bool {
    code_for_audit_token(token).is_some_and(|code| meets(&code, requirement))
}

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecCodeCopySigningInformation(
        code: *const std::ffi::c_void,
        flags: u32,
        information: *mut core_foundation::dictionary::CFDictionaryRef,
    ) -> i32;
    static kSecCodeInfoFlags: core_foundation::string::CFStringRef;
    static kSecCodeInfoEntitlementsDict: core_foundation::string::CFStringRef;
    static kSecCodeInfoUnique: core_foundation::string::CFStringRef;
}

/// `code`'s signing information: its flags and entitlements (`SecCodeCopySigningInformation`).
/// `None` on any error.
pub(super) fn signing(
    code: &security_framework::os::macos::code_signing::SecCode,
) -> Option<Signing> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::CFString;

    /// `kSecCSSigningInformation | kSecCSRequirementInformation`.
    const INFORMATION: u32 = (1 << 1) | (1 << 2);

    let mut information: CFDictionaryRef = std::ptr::null();
    // SAFETY: a valid code object (a SecCode is accepted where a SecStaticCode is expected), and
    // an out-pointer the call fills with a dictionary we then own (create rule).
    let status = unsafe {
        SecCodeCopySigningInformation(
            code.as_concrete_TypeRef().cast(),
            INFORMATION,
            &mut information,
        )
    };
    if status != 0 || information.is_null() {
        return None;
    }
    // SAFETY: a dictionary the call created for us.
    let information: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(information) };
    parse_signing(&information)
}

/// Static signed-file information, never evidence of the image a process loaded.
/// The caller must use the separate kernel/audit-token adapter for that purpose.
pub(super) fn static_signing(
    code: &security_framework::os::macos::code_signing::SecStaticCode,
) -> Option<(Signing, [u8; 20])> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::data::CFData;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::CFString;
    let mut information: CFDictionaryRef = std::ptr::null();
    // SAFETY: a live static code object and create-rule dictionary out-pointer.
    let status = unsafe {
        SecCodeCopySigningInformation(
            code.as_concrete_TypeRef().cast(),
            (1 << 1) | (1 << 2),
            &mut information,
        )
    };
    if status != 0 || information.is_null() {
        return None;
    }
    // SAFETY: SecCodeCopySigningInformation succeeded and returned a non-null create-rule CFDictionaryRef.
    let information: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(information) };
    // SAFETY: the Security framework owns this process-lifetime constant.
    let key = unsafe { CFString::wrap_under_get_rule(kSecCodeInfoUnique) };
    let hash = information.find(&key)?.downcast::<CFData>()?;
    let hash: [u8; 20] = hash.bytes().try_into().ok()?;
    Some((parse_signing(&information)?, hash))
}

/// What the host reads from a signing-information dictionary. Fails closed: an entitlements
/// value that is present but isn't a dictionary gives `None` (refused), never "no
/// entitlements".
fn parse_signing(
    information: &core_foundation::dictionary::CFDictionary<
        core_foundation::string::CFString,
        core_foundation::base::CFType,
    >,
) -> Option<Signing> {
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;

    // SAFETY: framework constants, valid for the process's life.
    let (flags_key, entitlements_key) = unsafe {
        (
            CFString::wrap_under_get_rule(kSecCodeInfoFlags),
            CFString::wrap_under_get_rule(kSecCodeInfoEntitlementsDict),
        )
    };
    let flags = information
        .find(&flags_key)
        .and_then(|value| value.downcast::<CFNumber>())
        .and_then(|number| number.to_i64())
        .and_then(|number| u32::try_from(number).ok());
    let entitlements = match information.find(&entitlements_key) {
        None => None,
        Some(value) => Some(value.downcast::<CFDictionary>()?),
    };
    let mut keychain_access_groups = None;
    let entitlements = entitlements.map(|dictionary| {
        // SAFETY: an entitlements dictionary maps strings to property-list values.
        let dictionary: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(dictionary.as_concrete_TypeRef()) };
        let (keys, values) = dictionary.get_keys_and_values();
        keys.into_iter()
            .zip(values)
            .map(|(key, value)| {
                // SAFETY: `key` is a valid CFStringRef entry from the live entitlements dictionary.
                let key = unsafe { CFString::wrap_under_get_rule(key.cast()) }.to_string();
                // SAFETY: `value` is a valid CFTypeRef entry from the live entitlements dictionary.
                let value = unsafe { CFType::wrap_under_get_rule(value) };
                if key == KEYCHAIN_ACCESS_GROUPS {
                    keychain_access_groups = value.downcast::<CFArray>().and_then(|array| {
                        array
                            .iter()
                            .map(|item| {
                                // SAFETY: an item of the array, retained by it.
                                unsafe { CFType::wrap_under_get_rule(*item) }
                                    .downcast::<CFString>()
                                    .map(|group| group.to_string())
                            })
                            .collect::<Option<Vec<String>>>()
                    });
                }
                (key, value.downcast::<CFBoolean>().map(bool::from))
            })
            .collect()
    });
    Some(Signing {
        flags,
        entitlements,
        keychain_access_groups,
    })
}

/// This process's own signing information (through `SecCodeCopySelf`): its flags (the
/// hardened runtime) and its entitlements (`keychain-access-groups` among them). `None` when it
/// can't be read. An unsigned test binary has no hardened runtime and no access groups, so the
/// live answer a release host needs can only be seen on a signed build.
pub(super) fn own_signing() -> Option<Signing> {
    use security_framework::os::macos::code_signing::{Flags, SecCode};
    signing(&SecCode::for_self(Flags::NONE).ok()?)
}

/// The static code object of the held `file`.
pub(super) fn static_code(
    file: &File,
) -> Result<security_framework::os::macos::code_signing::SecStaticCode, StaticImageError> {
    use core_foundation::url::CFURL;
    use security_framework::os::macos::code_signing::{Flags, SecStaticCode};
    use std::os::fd::AsRawFd;
    // The module-controlled descriptor path addresses the held inode, never a
    // mutable staged/sibling pathname. Verify support with the real kernel test.
    let path = format!("/dev/fd/{}", file.as_raw_fd());
    let url = CFURL::from_path(std::path::Path::new(&path), false).ok_or(StaticImageError::Path)?;
    SecStaticCode::from_path(&url, Flags::NONE).map_err(|_| StaticImageError::Refused)
}

/// A field of `codesign -dv`'s report on this test binary (`None` if unsigned).
#[cfg(test)]
pub(super) fn own_codesign_field(prefix: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/codesign")
        .arg("-dv")
        .arg(std::env::current_exe().ok()?)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find_map(|line| {
            line.split_whitespace()
                .find_map(|word| word.strip_prefix(prefix))
                .map(str::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The code-signature flag of the hardened runtime (`kSecCodeSignatureRuntime`): test data
    /// for a consumer's rule.
    const HARDENED_RUNTIME: u32 = 0x0001_0000;
    /// A consumer's rule, as test data: hardened code that no debugger may attach to.
    const RULE: SigningRule = SigningRule {
        required_flags: HARDENED_RUNTIME,
        rejected_entitlements: &["com.apple.security.get-task-allow"],
        absent_entitlements: &[],
    };

    fn hardened_and_clean(signing: &Signing) -> bool {
        RULE.flags_and_rejections_pass(signing)
    }

    /// Signing information as `SecCodeCopySigningInformation` gives it: the flags, and the
    /// entitlements as `entitlements` (any property-list value).
    fn information(
        flags: u32,
        entitlements: Option<core_foundation::base::CFType>,
    ) -> core_foundation::dictionary::CFDictionary<
        core_foundation::string::CFString,
        core_foundation::base::CFType,
    > {
        use core_foundation::base::TCFType;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::number::CFNumber;
        use core_foundation::string::CFString;
        // SAFETY: framework constants, valid for the process's life.
        let (flags_key, entitlements_key) = unsafe {
            (
                CFString::wrap_under_get_rule(kSecCodeInfoFlags),
                CFString::wrap_under_get_rule(kSecCodeInfoEntitlementsDict),
            )
        };
        let mut pairs = vec![(flags_key, CFNumber::from(i64::from(flags)).into_CFType())];
        if let Some(entitlements) = entitlements {
            pairs.push((entitlements_key, entitlements));
        }
        CFDictionary::from_CFType_pairs(&pairs)
    }

    #[test]
    fn entitlements_that_arent_a_dictionary_fail_closed() {
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;
        let hardened = HARDENED_RUNTIME | 0x2;
        // No entitlements at all: read as none, and clean.
        let info = parse_signing(&information(hardened, None)).unwrap();
        assert!(info.entitlements.is_none());
        assert!(hardened_and_clean(&info));
        // Entitlements that aren't a dictionary: refused, never read as "none".
        let bogus = CFString::new("com.apple.security.get-task-allow").into_CFType();
        assert!(parse_signing(&information(hardened, Some(bogus))).is_none());
    }

    #[test]
    fn the_keychain_access_groups_are_read_only_from_an_array_of_strings() {
        use core_foundation::array::CFArray;
        use core_foundation::base::TCFType;
        use core_foundation::boolean::CFBoolean;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::string::CFString;
        let entitled = |value: core_foundation::base::CFType| {
            let pairs = [
                (CFString::new(KEYCHAIN_ACCESS_GROUPS), value),
                (
                    CFString::new("com.apple.security.get-task-allow"),
                    CFBoolean::false_value().into_CFType(),
                ),
            ];
            parse_signing(&information(
                HARDENED_RUNTIME,
                Some(CFDictionary::from_CFType_pairs(&pairs).into_CFType()),
            ))
            .unwrap()
        };
        let groups = CFArray::from_CFTypes(&[
            CFString::new("TEAMID1234.example.host"),
            CFString::new("TEAMID1234.other"),
        ]);
        let info = entitled(groups.into_CFType());
        assert_eq!(
            info.keychain_access_groups,
            Some(vec![
                "TEAMID1234.example.host".to_owned(),
                "TEAMID1234.other".to_owned()
            ])
        );
        // The other entitlements are still read.
        assert!(info
            .entitlements
            .unwrap()
            .contains(&("com.apple.security.get-task-allow".to_owned(), Some(false))));
        // A single string, or an array holding anything but strings: no groups.
        let one = CFString::new("TEAMID1234.example.host").into_CFType();
        assert_eq!(entitled(one).keychain_access_groups, None);
        let mixed = CFArray::from_CFTypes(&[
            CFString::new("TEAMID1234.example.host").into_CFType(),
            CFBoolean::true_value().into_CFType(),
        ]);
        assert_eq!(entitled(mixed.into_CFType()).keychain_access_groups, None);
    }

    /// The real signing information of this test binary matches what `codesign` reports, and
    /// it isn't hardened (a linker or ad-hoc signature): so the extraction works, and the
    /// decision says no for it. (No Developer ID-signed, hardened binary is at hand for the
    /// other answer; the decision itself is tested on synthetic information above.)
    #[test]
    fn the_signing_information_of_this_process_is_read_as_codesign_shows_it() {
        let Some(expected) = own_codesign_field("flags=") else {
            eprintln!("skipped: this test binary has no code signature");
            return;
        };
        let expected = u32::from_str_radix(
            expected.split('(').next().unwrap().trim_start_matches("0x"),
            16,
        )
        .unwrap();
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let token = super::super::audit_token(&a).unwrap();
        let code = code_for_audit_token(&token).expect("this process's code object");
        let info = signing(&code).expect("its signing information");
        assert_eq!(info.flags, Some(expected));
        assert!(!hardened_and_clean(&info));
    }
}
