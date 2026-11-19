//! The creation intent, `hq-creation.v1.json`, version 2: the request an HQ creation
//! froze before it touched anything under the data roots. A retry of the same request
//! resumes from it; another request is refused. Opening checks the office against it.
//!
//! The file keeps its name so the host's catalog still knows an unfinished create by it;
//! its content is version 2, with the organization's data roots. It records the request
//! as it was received: `operation` is the launch binding's ULID, not the store operation
//! it maps to.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::DataRoots;
use crate::{error::OfficeError, model::OfficeInfo};

pub(crate) const FILE: &str = "hq-creation.v1.json";
/// The most an intent may hold.
const LIMIT: u64 = 16 * 1024;
pub(crate) const VERSION: u32 = 2;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Intent {
    pub version: u32,
    pub instance: String,
    /// The journal incarnation, writer epoch and machine the host reserved for this
    /// creation: a retry under another reservation is another request.
    pub journal_incarnation: String,
    pub writer_epoch: u64,
    pub machine_id: String,
    /// The launch binding's operation: a ULID.
    pub operation: String,
    /// The owner member: the creator's person record.
    pub owner: String,
    pub office_id: String,
    pub office_name: String,
    pub org_id: String,
    pub org_name: String,
    pub created_at: String,
    pub durable_root: PathBuf,
    pub runtime_root: PathBuf,
    pub bootstrap: org_proto::selected_bootstrap::PreparedV1,
}

/// An intent this build does not read: the office was made by an older one.
pub(crate) fn older_build() -> OfficeError {
    OfficeError::Invalid("this office was made by an older build")
}

impl Intent {
    pub(crate) fn info(&self) -> OfficeInfo {
        OfficeInfo {
            office_id: self.office_id.clone(),
            office_name: self.office_name.clone(),
            role: crate::OfficeRole::Hq,
            org_id: self.org_id.clone(),
            org_name: self.org_name.clone(),
            created_at: self.created_at.clone(),
        }
    }

    pub(crate) fn roots(&self) -> DataRoots {
        DataRoots {
            durable: self.durable_root.clone(),
            runtime: self.runtime_root.clone(),
        }
    }

    /// Whether `other` is the same request, whenever it was made.
    pub(crate) fn same_request(&self, other: &Intent) -> bool {
        let mut other = other.clone();
        other.created_at.clone_from(&self.created_at);
        *self == other
    }
}

/// `folder`'s intent: `None` when there is none, an older build's error when it is not
/// version 2.
pub(crate) fn read(folder: &Path) -> Result<Option<Intent>, OfficeError> {
    let Some(bytes) = org_store::read_one(folder, FILE, LIMIT).map_err(OfficeError::storage)?
    else {
        return Ok(None);
    };
    let intent: Intent = serde_json::from_slice(&bytes).map_err(|_| older_build())?;
    if intent.version != VERSION {
        return Err(older_build());
    }
    Ok(Some(intent))
}

/// Freezes `intent` in `folder`: never replaced. The same bytes there already are success.
pub(crate) fn write(folder: &Path, intent: &Intent) -> Result<(), OfficeError> {
    let bytes = serde_json::to_vec(intent).map_err(OfficeError::storage)?;
    if bytes.len() as u64 > LIMIT {
        return Err(OfficeError::Invalid("the creation request is too large"));
    }
    org_store::publish_one(folder, FILE, &bytes, true).map_err(OfficeError::storage)
}
