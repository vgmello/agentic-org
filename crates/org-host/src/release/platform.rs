//! Static payload checks precede publication. Loaded process identity remains a
//! separate kernel/audit-token check before any secret handoff.
use super::{files, Error};
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

#[cfg(target_os = "macos")]
fn static_code(
    file: &File,
) -> Result<security_framework::os::macos::code_signing::SecStaticCode, Error> {
    use core_foundation::url::CFURL;
    use security_framework::os::macos::code_signing::{Flags, SecStaticCode};
    use std::os::fd::AsRawFd;
    // The module-controlled descriptor path addresses the held inode, never a
    // mutable staged/sibling pathname. Verify support with the real kernel test.
    let path = format!("/dev/fd/{}", file.as_raw_fd());
    let url = CFURL::from_path(std::path::Path::new(&path), false).ok_or(Error::UnsafeInput)?;
    SecStaticCode::from_path(&url, Flags::NONE).map_err(|_| Error::PlatformAdmission)
}

#[cfg(target_os = "macos")]
fn platform_signature(file: &File, entry: &FileEntry) -> Result<(), Error> {
    use security_framework::os::macos::code_signing::{Flags, SecRequirement};
    use std::str::FromStr;
    let team = option_env!("ORG_APP_TEAM_ID").ok_or(Error::PlatformAdmission)?;
    let requirement = match entry.path() {
        "agentic-org" => {
            crate::code_signing::developer_id_requirement(crate::HOST_SIGNING_ID, team)
        }
        "agentic-org-office" => {
            crate::code_signing::developer_id_requirement(crate::SERVER_SIGNING_ID, team)
        }
        "agentic-org-web" => {
            crate::code_signing::developer_id_requirement(crate::GATEWAY_SIGNING_ID, team)
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
    let requirement =
        SecRequirement::from_str(&requirement).map_err(|_| Error::PlatformAdmission)?;
    let code = static_code(file)?;
    code.check_validity(Flags::NONE, &requirement)
        .map_err(|_| Error::PlatformAdmission)?;
    let (signing, hash) =
        crate::code_signing::static_signing(&code).ok_or(Error::PlatformAdmission)?;
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
#[cfg(not(target_os = "macos"))]
fn platform_signature(_: &File, _: &FileEntry) -> Result<(), Error> {
    // Linux platform custody/confinement admission has not been established.
    Err(Error::UnsupportedTarget)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn static_signature_query_addresses_the_held_descriptor_not_replaced_path() {
        let dir = tempfile::tempdir_in("/private/tmp").unwrap();
        let path = dir.path().join("image");
        std::fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
        let file = File::open(&path).unwrap();
        let original = static_code(&file).unwrap();
        let (_, expected) = crate::code_signing::static_signing(&original).unwrap();
        std::fs::rename(&path, dir.path().join("retained")).unwrap();
        std::fs::write(&path, b"unsigned replacement").unwrap();
        let held = static_code(&file).unwrap();
        let (_, actual) = crate::code_signing::static_signing(&held).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual, org_socket::image::own_loaded_cdhash().unwrap());
        let replacement = File::open(&path).unwrap();
        // Creating a static-code object alone does not establish a signature.
        assert!(crate::code_signing::static_signing(&static_code(&replacement).unwrap()).is_none());
    }
}
