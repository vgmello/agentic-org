use crate::Refused;
use std::path::{Path, PathBuf};

pub(super) fn application_base(_home: &Path) -> Result<PathBuf, Refused> {
    Err(Refused::unsupported("instance locations"))
}
