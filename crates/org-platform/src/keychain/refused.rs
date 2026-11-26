//! The refusing keychain adapter: nothing is looked up or started.

use super::SecretBytes;
use crate::Refused;
use std::time::Duration;

pub(super) fn find_generic_password(
    _service: &str,
    _max_bytes: usize,
    _timeout: Duration,
) -> Result<Option<SecretBytes>, Refused> {
    Err(Refused::unsupported("keychain"))
}
