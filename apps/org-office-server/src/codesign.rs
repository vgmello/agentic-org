//! Checking a peer's code signature, for the one check the server makes of who it received the
//! app token and the database key from: the host (`peer`). A mirror of the pieces of
//! `apps/org-office-host/src/code_signing.rs` it needs (the server must not depend on the host
//! crate); a fix to either lands in both until they share a crate (`org-codesign`, a follow-up).
//!
//! Everything fails closed: an error is "no".
#![allow(unsafe_code)]

/// The requirement for code signed as `identifier` with the Developer ID of team `team_id`. The
/// identifier must be a reverse-DNS name (ASCII letters, digits, `.` and `-`) and the team id
/// exactly 10 uppercase ASCII letters or digits, so neither can change the requirement's meaning.
pub(crate) fn developer_id_requirement(identifier: &str, team_id: &str) -> Result<String, String> {
    let identifier_ok = !identifier.is_empty()
        && identifier.len() <= 255
        && !identifier.starts_with(['.', '-'])
        && identifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if !identifier_ok {
        return Err("the signing identifier isn't well formed".to_owned());
    }
    let team_ok = team_id.len() == 10
        && team_id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if !team_ok {
        return Err("the team id isn't 10 uppercase letters or digits".to_owned());
    }
    Ok(format!(
        "identifier \"{identifier}\" and anchor apple generic and {DEVELOPER_ID} and \
         certificate leaf[subject.OU] = \"{team_id}\""
    ))
}

/// The Developer ID markers: a Developer ID Application certificate issued by the Developer ID
/// intermediate, so only a notarizable Developer ID signature passes.
pub(crate) const DEVELOPER_ID: &str = "certificate 1[field.1.2.840.113635.100.6.2.6] exists and \
                            certificate leaf[field.1.2.840.113635.100.6.1.13] exists";

/// The code-signature flag of the hardened runtime (`kSecCodeSignatureRuntime`).
pub(crate) const HARDENED_RUNTIME: u32 = 0x0001_0000;

/// Entitlements that let another process reach into a program: one with any of them is not
/// trusted with a secret.
pub(crate) const REJECTED_ENTITLEMENTS: [&str; 3] = [
    "com.apple.security.get-task-allow",
    "com.apple.security.cs.allow-dyld-environment-variables",
    "com.apple.security.cs.disable-library-validation",
];

/// What is read from a program's signing information.
#[derive(Debug, Default)]
pub(crate) struct Signing {
    /// The code-signature flags (`kSecCodeInfoFlags`), if present.
    pub(crate) flags: Option<u32>,
    /// The entitlements: each key with its value when it's a boolean (`None` for any other kind
    /// of value). `None` when the code has no entitlements.
    pub(crate) entitlements: Option<Vec<(String, Option<bool>)>>,
    /// The team identifier (`kSecCodeInfoTeamIdentifier`), if present.
    pub(crate) team_id: Option<String>,
}

/// Whether signed code may get a secret: the hardened runtime and none of the
/// [`REJECTED_ENTITLEMENTS`] (one present with any value but `false` counts). Missing flags
/// fail closed.
pub(crate) fn hardened_and_clean(signing: &Signing) -> bool {
    let Some(flags) = signing.flags else {
        return false;
    };
    if flags & HARDENED_RUNTIME == 0 {
        return false;
    }
    !signing
        .entitlements
        .iter()
        .flatten()
        .any(|(key, value)| REJECTED_ENTITLEMENTS.contains(&key.as_str()) && *value != Some(false))
}

/// An `audit_token_t`: eight 32-bit words.
pub(crate) const AUDIT_TOKEN_BYTES: usize = 32;
pub(crate) type AuditToken = [u8; AUDIT_TOKEN_BYTES];

/// Whether the process `token` names satisfies `requirement` and runs hardened code without the
/// [`REJECTED_ENTITLEMENTS`], all checked on one code object. Fails closed.
#[cfg(target_os = "macos")]
pub(crate) fn satisfies_hardened(token: &AuditToken, requirement: &str) -> bool {
    code_for_audit_token(token).is_some_and(|code| {
        meets(&code, requirement) && signing(&code).is_some_and(|s| hardened_and_clean(&s))
    })
}

/// The code object of the process `token` names. `None` on any error, including a token whose
/// process has exited or `exec`ed since.
#[cfg(target_os = "macos")]
fn code_for_audit_token(
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
#[cfg(target_os = "macos")]
fn meets(code: &security_framework::os::macos::code_signing::SecCode, requirement: &str) -> bool {
    use security_framework::os::macos::code_signing::{Flags, SecRequirement};
    use std::str::FromStr;

    SecRequirement::from_str(requirement)
        .is_ok_and(|requirement| code.check_validity(Flags::NONE, &requirement).is_ok())
}

#[cfg(target_os = "macos")]
#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecCodeCopySigningInformation(
        code: *const std::ffi::c_void,
        flags: u32,
        information: *mut core_foundation::dictionary::CFDictionaryRef,
    ) -> i32;
    static kSecCodeInfoFlags: core_foundation::string::CFStringRef;
    static kSecCodeInfoEntitlementsDict: core_foundation::string::CFStringRef;
    static kSecCodeInfoTeamIdentifier: core_foundation::string::CFStringRef;
}

/// `code`'s signing information: flags, entitlements, team identifier. `None` on any error.
#[cfg(target_os = "macos")]
fn signing(code: &security_framework::os::macos::code_signing::SecCode) -> Option<Signing> {
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

/// Fails closed: an entitlements value that is present but isn't a dictionary gives `None`
/// (refused), never "no entitlements".
#[cfg(target_os = "macos")]
fn parse_signing(
    information: &core_foundation::dictionary::CFDictionary<
        core_foundation::string::CFString,
        core_foundation::base::CFType,
    >,
) -> Option<Signing> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;

    // SAFETY: framework constants, valid for the process's life.
    let (flags_key, entitlements_key, team_key) = unsafe {
        (
            CFString::wrap_under_get_rule(kSecCodeInfoFlags),
            CFString::wrap_under_get_rule(kSecCodeInfoEntitlementsDict),
            CFString::wrap_under_get_rule(kSecCodeInfoTeamIdentifier),
        )
    };
    let flags = information
        .find(&flags_key)
        .and_then(|value| value.downcast::<CFNumber>())
        .and_then(|number| number.to_i64())
        .and_then(|number| u32::try_from(number).ok());
    let team_id = information
        .find(&team_key)
        .and_then(|value| value.downcast::<CFString>())
        .map(|team| team.to_string());
    let entitlements = match information.find(&entitlements_key) {
        None => None,
        Some(value) => Some(value.downcast::<CFDictionary>()?),
    };
    let entitlements = entitlements.map(|dictionary| {
        // SAFETY: an entitlements dictionary maps strings to property-list values.
        let dictionary: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(dictionary.as_concrete_TypeRef()) };
        let (keys, values) = dictionary.get_keys_and_values();
        keys.into_iter()
            .zip(values)
            .map(|(key, value)| {
                // SAFETY: the keys and values of the dictionary, retained by it.
                let key = unsafe { CFString::wrap_under_get_rule(key.cast()) }.to_string();
                let value = unsafe { CFType::wrap_under_get_rule(value) };
                (key, value.downcast::<CFBoolean>().map(bool::from))
            })
            .collect()
    });
    Some(Signing {
        flags,
        entitlements,
        team_id,
    })
}

/// This process's own signing information (through `SecCodeCopySelf`). `None` when it can't be
/// read, and for an unsigned binary.
#[cfg(target_os = "macos")]
pub(crate) fn own_signing() -> Option<Signing> {
    use security_framework::os::macos::code_signing::{Flags, SecCode};
    signing(&SecCode::for_self(Flags::NONE).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_requirement_names_the_identifier_and_team_and_refuses_odd_ones() {
        let requirement =
            developer_id_requirement("com.ellosoft.agentic-org.org-office-host", "AB12CD34EF")
                .unwrap();
        assert!(requirement.starts_with("identifier \"com.ellosoft.agentic-org.org-office-host\""));
        assert!(requirement.contains("anchor apple generic"));
        assert!(requirement.ends_with("certificate leaf[subject.OU] = \"AB12CD34EF\""));
        for team in ["", "ab12cd34ef", "AB12CD34E", "AB12CD34EFG", "AB12CD34E\""] {
            assert!(developer_id_requirement("a.b", team).is_err(), "{team:?}");
        }
        for identifier in ["", ".a", "-a", "a b", "a\"b", "a\nb"] {
            assert!(
                developer_id_requirement(identifier, "AB12CD34EF").is_err(),
                "{identifier:?}"
            );
        }
    }

    #[test]
    fn code_is_hardened_and_clean_only_with_the_runtime_flag_and_no_reaching_in_entitlements() {
        let signing =
            |flags: Option<u32>, entitlements: Option<Vec<(&str, Option<bool>)>>| Signing {
                flags,
                entitlements: entitlements.map(|list| {
                    list.into_iter()
                        .map(|(key, value)| (key.to_owned(), value))
                        .collect()
                }),
                team_id: None,
            };
        assert!(hardened_and_clean(&signing(Some(HARDENED_RUNTIME), None)));
        assert!(!hardened_and_clean(&signing(None, None)), "no flags");
        assert!(!hardened_and_clean(&signing(Some(0), None)), "not hardened");
        for key in REJECTED_ENTITLEMENTS {
            // Present as true, or as anything that isn't a boolean: refused. Only `false` passes.
            assert!(!hardened_and_clean(&signing(
                Some(HARDENED_RUNTIME),
                Some(vec![(key, Some(true))])
            )));
            assert!(!hardened_and_clean(&signing(
                Some(HARDENED_RUNTIME),
                Some(vec![(key, None)])
            )));
            assert!(hardened_and_clean(&signing(
                Some(HARDENED_RUNTIME),
                Some(vec![(key, Some(false))])
            )));
        }
        assert!(hardened_and_clean(&signing(
            Some(HARDENED_RUNTIME),
            Some(vec![("com.apple.security.network.client", Some(true))])
        )));
    }
}
