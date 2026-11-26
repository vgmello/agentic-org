//! Durable binding/authority file helpers: loading, optional JSON reads, atomic writes and
//! private directories.

use std::fs;
use std::path::Path;

use crate::error::OfficeError;
use crate::private_fs::{private_dir, tighten_dir, write_private_atomic};
use crate::providers::OfficeConnectionBinding;

pub(super) fn load_all_durable_bindings(
    bindings_dir: &Path,
    org_id: &str,
    office_id: &str,
) -> Result<Vec<OfficeConnectionBinding>, OfficeError> {
    let mut out = Vec::new();
    if !bindings_dir.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(bindings_dir).map_err(OfficeError::storage)? {
        let entry = entry.map_err(OfficeError::storage)?;
        let path = entry.path().join("binding.yaml");
        if path.is_file() {
            if let Some(b) = read_json_optional::<OfficeConnectionBinding>(&path)? {
                if b.organization_id == org_id && b.office_id == office_id {
                    out.push(b);
                }
            }
        }
    }
    Ok(out)
}

pub(super) fn read_json_optional<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, OfficeError> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(OfficeError::storage)?;
    let parsed = serde_json::from_slice(&bytes).map_err(OfficeError::storage)?;
    Ok(Some(parsed))
}

pub(super) fn write_json_atomic<T: serde::Serialize>(
    path: &Path,
    val: &T,
) -> Result<(), OfficeError> {
    let bytes = serde_json::to_vec_pretty(val).map_err(OfficeError::storage)?;
    write_private_atomic(path, &bytes).map_err(OfficeError::storage)
}

pub(super) fn ensure_dir_0700(dir: &Path) -> Result<(), OfficeError> {
    private_dir(dir).map_err(OfficeError::storage)?;
    tighten_dir(dir).map_err(OfficeError::storage)
}
