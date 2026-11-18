//! The macOS service adapter. Foreground mode only: the per-user launchd agent for
//! registered mode is written here by increment A2.

use super::{Foreground, ServiceManager, ServiceMode};
use crate::Refused;

pub(super) fn service(mode: ServiceMode) -> Result<Box<dyn ServiceManager>, Refused> {
    match mode {
        ServiceMode::Foreground => Ok(Box::new(Foreground)),
        ServiceMode::Registered => Err(super::registered_mode_unwritten()),
    }
}
