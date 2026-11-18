use super::{DiskEncryption, Wording};
use crate::Refused;

pub(super) fn disk_encryption() -> Result<Box<dyn DiskEncryption>, Refused> {
    Err(Refused::unsupported("disk encryption"))
}

pub(super) const WORDING: Wording = Wording {
    name: "not available",
    setting: "disk encryption for the disk that holds this folder",
};
