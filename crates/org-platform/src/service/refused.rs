use super::{Placement, ServiceManager};
use crate::Refused;
use std::path::Path;

pub(super) fn user_placement(_home: &Path) -> Result<Placement, Refused> {
    Err(Refused::unsupported("service registration"))
}

pub(super) fn registered(_placement: Placement) -> Result<Box<dyn ServiceManager>, Refused> {
    Err(Refused::unsupported("service registration"))
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn registered_with_programs(
    _placement: Placement,
    _programs: &Path,
    _runtime_dir: Option<std::path::PathBuf>,
) -> Result<Box<dyn ServiceManager>, Refused> {
    Err(Refused::unsupported(
        "service registration through stand-in programs",
    ))
}
