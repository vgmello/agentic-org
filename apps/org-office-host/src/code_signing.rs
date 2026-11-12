//! Reading and checking code signatures, for the host's checks of the programs it trusts with
//! secrets: the desktop app (`app_auth`, an app token) and its office servers (`server_check`,
//! a database key), and of its own entitlements (`keys`, its Keychain access group).
//!
//! Everything here fails closed: an error is "no".

#[cfg(target_os = "macos")]
use crate::peer::AuditToken;

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

/// What the host reads from a program's signing information.
#[derive(Debug, Default)]
pub(crate) struct Signing {
    /// The code-signature flags (`kSecCodeInfoFlags`), if present.
    pub(crate) flags: Option<u32>,
    /// The entitlements (`kSecCodeInfoEntitlementsDict`): each key with its value when it's a
    /// boolean (`None` for any other kind of value). `None` when the code has no entitlements.
    pub(crate) entitlements: Option<Vec<(String, Option<bool>)>>,
    /// The `keychain-access-groups` entitlement: its groups, when it's an array of strings.
    /// `None` when it's missing or anything else.
    pub(crate) keychain_access_groups: Option<Vec<String>>,
}

/// The entitlement that lists the Keychain access groups a signed program may use.
#[cfg(target_os = "macos")]
pub(crate) const KEYCHAIN_ACCESS_GROUPS: &str = "keychain-access-groups";

/// Whether signed code may get a secret (an app token, a database key): it runs with the hardened runtime and has none of
/// the [`REJECTED_ENTITLEMENTS`] (one that's present with any value but `false` counts). Missing
/// flags fail closed.
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
pub(crate) fn code_for_audit_token(
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
pub(crate) fn meets(
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
#[cfg(all(target_os = "macos", test))]
pub(crate) fn code_satisfies(token: &AuditToken, requirement: &str) -> bool {
    code_for_audit_token(token).is_some_and(|code| meets(&code, requirement))
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
}

/// `code`'s signing information: its flags and entitlements (`SecCodeCopySigningInformation`).
/// `None` on any error.
#[cfg(target_os = "macos")]
pub(crate) fn signing(
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

/// What the host reads from a signing-information dictionary. Fails closed: an entitlements
/// value that is present but isn't a dictionary gives `None` (refused), never "no
/// entitlements".
#[cfg(target_os = "macos")]
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
                // SAFETY: the keys and values of the dictionary, retained by it.
                let key = unsafe { CFString::wrap_under_get_rule(key.cast()) }.to_string();
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
#[cfg(target_os = "macos")]
pub(crate) fn own_signing() -> Option<Signing> {
    use security_framework::os::macos::code_signing::{Flags, SecCode};
    signing(&SecCode::for_self(Flags::NONE).ok()?)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn own_signing() -> Option<Signing> {
    None
}

/// A field of `codesign -dv`'s report on this test binary (`None` if unsigned).
#[cfg(all(target_os = "macos", test))]
pub(crate) fn own_codesign_field(prefix: &str) -> Option<String> {
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

    #[test]
    fn a_developer_id_requirement_names_the_identifier_and_team() {
        assert_eq!(
            developer_id_requirement("com.ellosoft.agentic-org.org-server", "AB12CD34EF").unwrap(),
            "identifier \"com.ellosoft.agentic-org.org-server\" and anchor apple generic \
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

    /// Signing information as `SecCodeCopySigningInformation` gives it: the flags, and the
    /// entitlements as `entitlements` (any property-list value).
    #[cfg(target_os = "macos")]
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

    #[cfg(target_os = "macos")]
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

    #[cfg(target_os = "macos")]
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
            CFString::new("TEAMID1234.org.agentic.host"),
            CFString::new("TEAMID1234.other"),
        ]);
        let info = entitled(groups.into_CFType());
        assert_eq!(
            info.keychain_access_groups,
            Some(vec![
                "TEAMID1234.org.agentic.host".to_owned(),
                "TEAMID1234.other".to_owned()
            ])
        );
        // The other entitlements are still read.
        assert!(info
            .entitlements
            .unwrap()
            .contains(&("com.apple.security.get-task-allow".to_owned(), Some(false))));
        // A single string, or an array holding anything but strings: no groups.
        let one = CFString::new("TEAMID1234.org.agentic.host").into_CFType();
        assert_eq!(entitled(one).keychain_access_groups, None);
        let mixed = CFArray::from_CFTypes(&[
            CFString::new("TEAMID1234.org.agentic.host").into_CFType(),
            CFBoolean::true_value().into_CFType(),
        ]);
        assert_eq!(entitled(mixed.into_CFType()).keychain_access_groups, None);
    }

    /// The real signing information of this test binary matches what `codesign` reports, and
    /// it isn't hardened (a linker or ad-hoc signature): so the extraction works, and the
    /// decision says no for it. (No Developer ID-signed, hardened binary is at hand for the
    /// other answer; the decision itself is tested on synthetic information above.)
    #[cfg(target_os = "macos")]
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
        let token = crate::peer::audit_token(&a).unwrap();
        let code = code_for_audit_token(&token).expect("this process's code object");
        let info = signing(&code).expect("its signing information");
        assert_eq!(info.flags, Some(expected));
        assert!(!hardened_and_clean(&info));
    }
}
