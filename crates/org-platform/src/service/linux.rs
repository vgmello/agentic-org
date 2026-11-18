//! The GNU/Linux service adapter. Foreground mode only: the systemd user unit for
//! registered mode is written here by increment X5.

use super::{Foreground, ServiceManager, ServiceMode};
use crate::Refused;

pub(super) fn service(mode: ServiceMode) -> Result<Box<dyn ServiceManager>, Refused> {
    match mode {
        ServiceMode::Foreground => Ok(Box::new(Foreground)),
        ServiceMode::Registered => Err(super::registered_mode_unwritten()),
    }
}
