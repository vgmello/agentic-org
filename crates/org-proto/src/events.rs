use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::types::{OfficeInfo, Organization};

/// One kind of server-pushed event: its wire name and the shape of its data.
pub trait EventKind {
    const NAME: &'static str;
    type Data: Serialize + DeserializeOwned;
}

/// `organization.created`: sent by this machine's host to its subscribers when an
/// organization (and its HQ office) is created here, so the app's menu refreshes live.
pub struct OrganizationCreated;

impl EventKind for OrganizationCreated {
    const NAME: &'static str = "organization.created";
    type Data = Organization;
}

/// The data of an `office.status_changed` event: the office's whole updated record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeStatusChangedData {
    pub office: OfficeInfo,
}

/// `office.status_changed`: sent by this machine's host to its subscribers whenever an
/// office's `status`, `status_reason` or `next_restart_at` changes, so the app shows live
/// office status without polling `office.list`. The data is the whole updated
/// [`OfficeInfo`]; replace the stored record with it.
///
/// The host's `subscribe` has no `from_position`, so to miss nothing: subscribe first, then
/// call `office.list` on the same connection. Events after its response are newer than it.
///
/// Sent on `host.sock` only (the host's `subscribe`); office servers never send it.
pub struct OfficeStatusChanged;

impl EventKind for OfficeStatusChanged {
    const NAME: &'static str = "office.status_changed";
    type Data = OfficeStatusChangedData;
}
