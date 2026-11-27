//! Pure code-signing identity and Developer ID requirement policy shared between release
//! admission and host peer/server checks.

use org_platform_host::admission::{Signing, SigningRule};

/// The host's own signing identifier (ADR-0003 §2.1), for the bundle's signing of
/// the agentic-org executable in host mode. The signing identifier remains stable
/// across the executable migration; agent-os must never receive this entitlement.
pub const HOST_SIGNING_ID: &str = "com.vgmello.agentic-org.org-office-host";
/// The office worker's signing identifier (ADR-0003 §2.1): a release host gives an office's key
/// only to an office worker signed with it by the app's team.
pub const OFFICE_SIGNING_ID: &str = "com.vgmello.agentic-org.office";
/// The browser service has no office-worker or Keychain authority (ADR-0008).
pub const WEB_SIGNING_ID: &str = "com.vgmello.agentic-org.web";

/// The Developer ID markers: the signing certificate is a Developer ID Application certificate
/// (leaf extension 1.2.840.113635.100.6.1.13) issued by the Developer ID intermediate
/// (1.2.840.113635.100.6.2.6). So only a notarizable Developer ID signature passes, not
/// a development or distribution certificate of the same team.
pub const DEVELOPER_ID: &str = "certificate 1[field.1.2.840.113635.100.6.2.6] exists and \
                                certificate leaf[field.1.2.840.113635.100.6.1.13] exists";

/// The code-signature flag of the hardened runtime (`kSecCodeSignatureRuntime`).
pub const HARDENED_RUNTIME: u32 = 0x0001_0000;

/// Entitlements that let another process or library reach into a program (a debugger, injected
/// libraries, `DYLD_*` variables): an app or an office server with any of them could be made to
/// hand its secrets over, so it isn't trusted with one.
pub const REJECTED_ENTITLEMENTS: [&str; 3] = [
    "com.apple.security.get-task-allow",
    "com.apple.security.cs.allow-dyld-environment-variables",
    "com.apple.security.cs.disable-library-validation",
];

/// The rule every program trusted with a secret must meet, as the platform applies it: the
/// hardened runtime and none of the [`REJECTED_ENTITLEMENTS`].
pub const HARDENED: SigningRule = SigningRule {
    required_flags: HARDENED_RUNTIME,
    rejected_entitlements: &REJECTED_ENTITLEMENTS,
    absent_entitlements: &[],
};

/// The requirement for code signed as `identifier` with the Developer ID of team `team_id`.
/// The identifier must be a reverse-DNS name (ASCII letters, digits, `.` and `-`) and the
/// team id exactly 10 uppercase ASCII letters or digits, so neither can change the requirement's
/// meaning.
pub fn developer_id_requirement(identifier: &str, team_id: &str) -> Result<String, String> {
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

/// Whether signed code may get a secret: it runs with the hardened runtime and has none of
/// the [`REJECTED_ENTITLEMENTS`] (one that's present with any value but `false` counts).
/// Missing flags fail closed.
pub fn hardened_and_clean(signing: &Signing) -> bool {
    HARDENED.flags_and_rejections_pass(signing)
}
