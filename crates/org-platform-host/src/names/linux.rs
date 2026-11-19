//! GNU/Linux: names are bytes.

use std::path::Path;

use org_platform::Refused;

pub(super) fn paths_may_alias(_a: &Path, _b: &Path) -> Result<bool, Refused> {
    Ok(false)
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn equivalent_spellings_are_one_name() -> Result<bool, Refused> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fact_for_tests_is_what_the_comparison_answers() {
        assert_eq!(
            equivalent_spellings_are_one_name(),
            paths_may_alias(Path::new("/d/caf\u{e9}"), Path::new("/d/cafe\u{301}"))
        );
        assert_eq!(equivalent_spellings_are_one_name(), Ok(false));
    }

    #[test]
    fn names_are_bytes_so_no_other_spelling_is_the_same_path() {
        assert!(
            !paths_may_alias(Path::new("/data/CAF\u{c9}"), Path::new("/data/cafe\u{301}")).unwrap()
        );
        assert!(!paths_may_alias(Path::new("/data/a"), Path::new("/data/A/child")).unwrap());
    }
}
