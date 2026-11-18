//! The refusing adapter: no rule is known for this system's filesystems.

use std::path::Path;

use org_platform::Refused;

pub(super) fn paths_may_alias(_a: &Path, _b: &Path) -> Result<bool, Refused> {
    Err(super::refusal())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_comparison_refuses_and_names_the_capability() {
        let refused = paths_may_alias(Path::new("/a"), Path::new("/a")).unwrap_err();
        let Refused::Unsupported { capability, os } = refused;
        assert_eq!(capability, "file name comparison");
        assert_eq!(os, std::env::consts::OS);
    }
}
