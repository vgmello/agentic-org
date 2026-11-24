//! `office.json`: the plaintext, secret-free file that marks an office as finished and tells the
//! host (and `agent-os`) which office this folder is. The server writes it LAST on `--create`
//! (`org_store::publish_one`: a fresh temporary file, fsync, rename, fsync of the folder),
//! so a folder without it is an unfinished create.
//!
//! The shape is the host/server boundary design's (`docs/design/office-host-boundary.md`),
//! version 2: UUIDv7 office and organization ids. The office writes and reads no other version;
//! the plain ids of version 1 belong to offices this build refuses. **Transitional:** the
//! contract crate `org-office-launch` will own the type and its checks; when it is on `v1`, this
//! module becomes a thin adapter over it.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::OfficeError;
use crate::model::{OfficeInfo, OfficeRole};
use org_proto::OrganizationName;

pub(crate) const FILE_NAME: &str = "office.json";
/// The most the host reads.
const MAX_BYTES: u64 = 64 * 1024;
const VERSION: u32 = 2;

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
    if !(org_proto::is_uuid_v7(&info.office_id) && org_proto::is_uuid_v7(&info.org_id)) {
        Err(OfficeError::Invalid(
            "an id must be a canonical lowercase UUIDv7",
        ))
    } else if !is_timestamp(&info.created_at) {
        Err(OfficeError::Invalid(
            "created_at is not an RFC 3339 UTC time",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn encode(info: &OfficeInfo) -> Result<Vec<u8>, OfficeError> {
    check(info)?;
    let raw = Raw {
        v: VERSION,
        office_id: info.office_id.clone(),
        office_name: info.office_name.clone(),
        role: match info.role {
            OfficeRole::Hq => "hq".to_owned(),
            OfficeRole::Satellite => "satellite".to_owned(),
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
        "satellite" => OfficeRole::Satellite,
        _ => {
            return Err(OfficeError::Invalid(
                "office.json has a role this build does not know",
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

    const OFFICE: &str = "01900000-0000-7000-8000-000000000001";
    const ORG: &str = "01900000-0000-7000-8000-000000000002";

    fn info() -> OfficeInfo {
        OfficeInfo {
            office_id: OFFICE.into(),
            office_name: "HQ".into(),
            role: OfficeRole::Hq,
            org_id: ORG.into(),
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
            serde_json::json!({"v":2,"office_id":OFFICE,"office_name":"HQ","role":"hq",
                "org_id":ORG,"org_name":"Acme","created_at":"2026-11-08T02:10:00.000Z"})
        );
        assert_eq!(decode(&bytes).unwrap(), info());
    }

    #[test]
    fn it_refuses_what_the_host_would_refuse() {
        // A plain id (version 1's), an upper-case UUID and a UUID of another version.
        for id in [
            "OFFICE1",
            "01900000-0000-7000-8000-00000000000A",
            "01900000-0000-4000-8000-000000000001",
        ] {
            let mut bad = info();
            bad.office_id = id.into();
            assert!(matches!(encode(&bad), Err(OfficeError::Invalid(_))), "{id}");
            let mut bad = info();
            bad.org_id = id.into();
            assert!(matches!(encode(&bad), Err(OfficeError::Invalid(_))), "{id}");
        }
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
            // Version 1, even with plain ids: an office this build refuses.
            good.replace("\"v\": 2", "\"v\": 1"),
            good.replace("\"v\": 2", "\"v\": 1")
                .replace(OFFICE, "OFFICE1")
                .replace(ORG, "ORG1"),
            good.replace("\"v\": 2", "\"v\": 3"),
            good.replace("\"hq\"", "\"moon\""),
            good.replace(OFFICE, "../up"),
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
    fn reading_refuses_links_and_big_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).unwrap().is_none());

        std::fs::write(dir.path().join(FILE_NAME), encode(&info()).unwrap()).unwrap();
        assert_eq!(read(dir.path()).unwrap().unwrap(), info());

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
    }
}
