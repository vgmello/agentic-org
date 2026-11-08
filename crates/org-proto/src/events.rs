use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::types::Organization;

/// One kind of server-pushed event: its wire name and the shape of its data.
pub trait EventKind {
    const NAME: &'static str;
    type Data: Serialize + DeserializeOwned;
}

/// `organization.created`: an organization was created on this office server.
pub struct OrganizationCreated;

impl EventKind for OrganizationCreated {
    const NAME: &'static str = "organization.created";
    type Data = Organization;
}
