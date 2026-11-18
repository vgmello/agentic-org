//! Static payload checks precede publication. Loaded process identity remains a
//! separate kernel/audit-token check before any secret handoff.
use super::{files, Error};
use org_platform_host::admission::{StaticImageError, StaticSignature};
use org_release::{Architecture, FileEntry, Platform, VerifiedManifest};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

pub(super) fn verify(root: &files::Directory, manifest: &VerifiedManifest) -> Result<(), Error> {
    for entry in manifest
        .manifest()
        .files()
        .iter()
        .filter(|f| f.role() == org_release::Role::Executable)
    {
        let mut file = root.payload(entry.path())?;
        let before = files::identity(&file)?;
        header(
            &mut file,
            manifest.manifest().platform(),
            manifest.manifest().architecture(),
        )?;
        platform_signature(&file, entry)?;
        if before != files::identity(&file)? {
            return Err(Error::UnsafeInput);
        }
    }
    Ok(())
}

fn header(file: &mut File, platform: Platform, architecture: Architecture) -> Result<(), Error> {
    file.seek(SeekFrom::Start(0))?;
    let mut header = [0_u8; 64];
    file.read_exact(&mut header)?;
    let cpu = match architecture {
        Architecture::Aarch64 => 0x0100_000c_u32,
        Architecture::X86_64 => 0x0100_0007,
    };
    match platform {
        Platform::Macos
            if header[..4] == [0xcf, 0xfa, 0xed, 0xfe]
                && u32::from_le_bytes(
                    header[4..8].try_into().map_err(|_| Error::InvalidRelease)?,
                ) == cpu
                && header[12..16] == 2_u32.to_le_bytes() =>
        {
            Ok(())
        }
        Platform::Linux
            if header[..7] == *b"\x7fELF\x02\x01\x01"
                && matches!(u16::from_le_bytes([header[16], header[17]]), 2 | 3)
                && u16::from_le_bytes([header[18], header[19]])
                    == match architecture {
                        Architecture::Aarch64 => 183,
                        Architecture::X86_64 => 62,
                    } =>
        {
            Ok(())
        }
        _ => Err(Error::InvalidRelease),
    }
}

/// What a release executable must be, per file: signed with the app team's Developer ID
/// under its own signing identifier, hardened, with the manifest's exact CodeDirectory hash,
/// and (but for the host) without a Keychain access group. The platform establishes the
/// signature of the held file; a build target without such a check is outside the release
/// profile.
fn platform_signature(file: &File, entry: &FileEntry) -> Result<(), Error> {
    // Linux platform custody/confinement admission has not been established.
    let images = org_platform_host::admission::static_images()
        .map_err(|_refused| Error::UnsupportedTarget)?;
    let team = option_env!("ORG_APP_TEAM_ID").ok_or(Error::PlatformAdmission)?;
    let requirement = match entry.path() {
        "agentic-org" => {
            crate::code_signing::developer_id_requirement(crate::HOST_SIGNING_ID, team)
        }
        "agentic-org-office" => {
            crate::code_signing::developer_id_requirement(crate::OFFICE_SIGNING_ID, team)
        }
        "agentic-org-web" => {
            crate::code_signing::developer_id_requirement(crate::WEB_SIGNING_ID, team)
        }
        "agent-os" => {
            // No existing agent-os identifier grants host authority. The signed
            // manifest binds its exact hash; require the selected Developer ID.
            crate::code_signing::developer_id_requirement(crate::HOST_SIGNING_ID, team).map(|s| {
                s.replacen(
                    &format!("identifier \"{}\" and ", crate::HOST_SIGNING_ID),
                    "",
                    1,
                )
            })
        }
        _ => return Err(Error::InvalidRelease),
    }
    .map_err(|_| Error::PlatformAdmission)?;
    // The held descriptor addresses the held inode, never a mutable staged/sibling
    // pathname.
    let StaticSignature {
        signing,
        cdhash: hash,
    } = images
        .signature(file, &requirement)
        .map_err(|error| match error {
            StaticImageError::Path => Error::UnsafeInput,
            StaticImageError::Refused => Error::PlatformAdmission,
        })?;
    if !crate::code_signing::hardened_and_clean(&signing)
        || entry.macos_cdhash() != Some(&hash)
        || (entry.path() != "agentic-org"
            && signing
                .entitlements
                .iter()
                .flatten()
                .any(|(key, _)| key == "keychain-access-groups"))
    {
        return Err(Error::PlatformAdmission);
    }
    Ok(())
}
