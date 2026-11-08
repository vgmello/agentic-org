//! `office.json`: the plaintext, secret-free file that marks an office as finished and tells the
//! host (and `org-agent`) which office this folder is. The server writes it LAST on `--create`
//! (temporary file, fsync, rename, fsync of the folder), so a folder without it is an
//! unfinished create.
//!
//! The shape is the host/server boundary design's (`docs/design/office-host-boundary.md`,
//! schema v1). **Transitional:** the contract crate `org-office-launch` will own the type and
//! its checks; when it is on `v1`, this module becomes a thin adapter over it.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::OfficeError;
use crate::model::{OfficeInfo, OfficeRole};
use org_proto::OrganizationName;

pub(crate) const FILE_NAME: &str = "office.json";
const TEMP_NAME: &str = "office.json.tmp";
/// The most the host reads.
const MAX_BYTES: u64 = 64 * 1024;
const VERSION: u32 = 1;
/// The longest office or organization id.
pub(crate) const MAX_ID_CHARS: usize = 64;

#[derive(Serialize, Deserialize)]
struct Raw {
    v: u32,
    office_id: String,
    office_name: String,
    role: String,
    org_id: String,
    org_name: String,
    created_at: String,
}

/// Whether `id` is a plain id: 1 to 64 ASCII letters and digits.
pub(crate) fn is_plain_id(id: &str) -> bool {
    (1..=MAX_ID_CHARS).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Whether `text` looks like `YYYY-MM-DDTHH:MM:SS.mmmZ`.
fn is_timestamp(text: &str) -> bool {
    const SHAPE: &[u8; 24] = b"dddd-dd-ddTdd:dd:dd.dddZ";
    text.len() == 24
        && text.bytes().zip(SHAPE).all(|(c, &s)| {
            if s == b'd' {
                c.is_ascii_digit()
            } else {
                c == s
            }
        })
}

/// The checks every read and write makes, so the server never writes a file it would refuse.
fn check(info: &OfficeInfo) -> Result<(), OfficeError> {
    // The name rule is the contract's (`org_proto::OrganizationName`, the host's reader uses
    // it too), and a name is stored trimmed.
    for name in [&info.office_name, &info.org_name] {
        let parsed = OrganizationName::parse(name).map_err(OfficeError::InvalidName)?;
        if parsed.as_str() != name.as_str() {
            return Err(OfficeError::Invalid("a name is not trimmed"));
        }
    }
    if !is_plain_id(&info.office_id) || !is_plain_id(&info.org_id) {
        Err(OfficeError::Invalid(
            "an id must be 1 to 64 letters and digits",
        ))
    } else if !is_timestamp(&info.created_at) {
        Err(OfficeError::Invalid(
            "created_at is not an RFC 3339 UTC time",
        ))
    } else {
        Ok(())
    }
}

fn encode(info: &OfficeInfo) -> Result<Vec<u8>, OfficeError> {
    check(info)?;
    let raw = Raw {
        v: VERSION,
        office_id: info.office_id.clone(),
        office_name: info.office_name.clone(),
        role: match info.role {
            OfficeRole::Hq => "hq".to_owned(),
        },
        org_id: info.org_id.clone(),
        org_name: info.org_name.clone(),
        created_at: info.created_at.clone(),
    };
    let mut bytes = serde_json::to_vec_pretty(&raw).map_err(OfficeError::storage)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<OfficeInfo, OfficeError> {
    let raw: Raw = serde_json::from_slice(bytes)
        .map_err(|_| OfficeError::Invalid("office.json is not valid"))?;
    if raw.v != VERSION {
        return Err(OfficeError::Invalid(
            "office.json has an unsupported version",
        ));
    }
    let role = match raw.role.as_str() {
        "hq" => OfficeRole::Hq,
        _ => {
            return Err(OfficeError::Invalid(
                "office.json has a role this build does not know",
            ))
        }
    };
    let info = OfficeInfo {
        office_id: raw.office_id,
        office_name: raw.office_name,
        role,
        org_id: raw.org_id,
        org_name: raw.org_name,
        created_at: raw.created_at,
    };
    check(&info)?;
    Ok(info)
}

/// Reads `<folder>/office.json`: `Ok(None)` if there is none. It must be a regular file (not a
/// link) of at most 64 KiB.
pub(crate) fn read(folder: &Path) -> Result<Option<OfficeInfo>, OfficeError> {
    let path = folder.join(FILE_NAME);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(OfficeError::storage(e)),
    };
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(OfficeError::Invalid(
            "office.json is not a regular file of a sane size",
        ));
    }
    let bytes = fs::read(&path).map_err(OfficeError::storage)?;
    decode(&bytes).map(Some)
}

/// Writes `<folder>/office.json` so that a crash leaves either no file or the whole one:
/// a fresh temporary file (never followed through a link), fsync, rename, fsync of the folder.
pub(crate) fn write(folder: &Path, info: &OfficeInfo) -> Result<(), OfficeError> {
    let bytes = encode(info)?;
    let temp = folder.join(TEMP_NAME);
    let mut file: File = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(OfficeError::storage)?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temp, folder.join(FILE_NAME)))
        .and_then(|()| File::open(folder)?.sync_all())
        .map_err(OfficeError::storage)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> OfficeInfo {
        OfficeInfo {
            office_id: "OFFICE1".into(),
            office_name: "HQ".into(),
            role: OfficeRole::Hq,
            org_id: "ORG1".into(),
            org_name: "Acme".into(),
            created_at: "2026-11-08T02:10:00.000Z".into(),
        }
    }

    #[test]
    fn it_round_trips_and_has_the_documented_shape() {
        let bytes = encode(&info()).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"v":1,"office_id":"OFFICE1","office_name":"HQ","role":"hq",
                "org_id":"ORG1","org_name":"Acme","created_at":"2026-11-08T02:10:00.000Z"})
        );
        assert_eq!(decode(&bytes).unwrap(), info());
    }

    #[test]
    fn it_refuses_what_the_host_would_refuse() {
        let mut bad = info();
        bad.office_id = "has-dash".into();
        assert!(matches!(encode(&bad), Err(OfficeError::Invalid(_))));
        let mut bad = info();
        bad.org_id = "x".repeat(65);
        assert!(matches!(encode(&bad), Err(OfficeError::Invalid(_))));
        let mut bad = info();
        bad.org_name = " Acme ".into(); // not trimmed
        assert!(matches!(encode(&bad), Err(OfficeError::Invalid(_))));
        let mut bad = info();
        bad.office_name = "".into();
        assert!(matches!(encode(&bad), Err(OfficeError::InvalidName(_))));
        let mut bad = info();
        bad.org_name = "zero\u{200B}width".into(); // a format character: the contract's rule
        assert!(matches!(encode(&bad), Err(OfficeError::InvalidName(_))));
        let mut bad = info();
        bad.created_at = "yesterday".into();
        assert!(matches!(encode(&bad), Err(OfficeError::Invalid(_))));

        let good = String::from_utf8(encode(&info()).unwrap()).unwrap();
        for broken in [
            good.replace("\"v\": 1", "\"v\": 2"),
            good.replace("\"hq\"", "\"moon\""),
            good.replace("OFFICE1", "../up"),
            "{}".to_owned(),
            "not json".to_owned(),
        ] {
            assert!(
                matches!(decode(broken.as_bytes()), Err(OfficeError::Invalid(_))),
                "{broken}"
            );
        }
    }

    #[test]
    fn reading_refuses_links_and_big_files_and_writing_never_follows_a_planted_temp() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).unwrap().is_none());

        write(dir.path(), &info()).unwrap();
        assert_eq!(read(dir.path()).unwrap().unwrap(), info());
        assert_eq!(
            std::fs::metadata(dir.path().join(FILE_NAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(!dir.path().join(TEMP_NAME).exists());

        // A link in its place is refused, not followed.
        std::fs::remove_file(dir.path().join(FILE_NAME)).unwrap();
        let outside = dir.path().join("elsewhere");
        std::fs::write(&outside, encode(&info()).unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, dir.path().join(FILE_NAME)).unwrap();
        assert!(matches!(read(dir.path()), Err(OfficeError::Invalid(_))));
        std::fs::remove_file(dir.path().join(FILE_NAME)).unwrap();

        // Too big.
        std::fs::write(dir.path().join(FILE_NAME), vec![b' '; 70 * 1024]).unwrap();
        assert!(matches!(read(dir.path()), Err(OfficeError::Invalid(_))));
        std::fs::remove_file(dir.path().join(FILE_NAME)).unwrap();

        // A planted temporary file is never written through.
        std::os::unix::fs::symlink(&outside, dir.path().join(TEMP_NAME)).unwrap();
        let before = std::fs::read(&outside).unwrap();
        assert!(write(dir.path(), &info()).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), before);
    }

    use std::os::unix::fs::PermissionsExt;
}
