use super::DiskEncryption;
use crate::Refused;

pub(super) fn disk_encryption() -> Result<Box<dyn DiskEncryption>, Refused> {
    Err(Refused::unsupported("disk encryption"))
}
