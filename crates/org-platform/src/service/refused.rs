//! The refusing service adapter: no service manager and no foreground mode.

use super::{ServiceManager, ServiceMode};
use crate::Refused;

pub(super) fn service(_mode: ServiceMode) -> Result<Box<dyn ServiceManager>, Refused> {
    Err(Refused::unsupported("service"))
}
