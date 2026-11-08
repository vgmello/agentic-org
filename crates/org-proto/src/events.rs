use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::types::Organization;

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
