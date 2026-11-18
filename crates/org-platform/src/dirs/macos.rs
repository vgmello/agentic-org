use crate::Refused;
use std::path::{Path, PathBuf};

pub(super) fn application_base(home: &Path) -> Result<PathBuf, Refused> {
    Ok(home.join("Library/Application Support/agentic.org"))
}
