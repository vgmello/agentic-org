//! macOS: names fold case and canonical Unicode spelling, as APFS and HFS+ may.

use std::path::{Component, Path};

use org_platform::Refused;

pub(super) fn paths_may_alias(a: &Path, b: &Path) -> Result<bool, Refused> {
    // Compare components, not string prefixes: APFS/HFS+ also equate canonical
    // Unicode spellings. No localized, diacritic-ignoring or width-ignoring flags.
    // Existing registered targets retain their proven distinct inode ownership
    // on case-sensitive volumes; this conservative comparison is for new claims.
    Ok(a.components()
        .zip(b.components())
        .all(|(left, right)| match (left, right) {
            (Component::Normal(left), Component::Normal(right)) => {
                use core_foundation::base::{CFComparisonResult, TCFType};
                use core_foundation::string::{
                    kCFCompareCaseInsensitive, kCFCompareNonliteral, CFString, CFStringCompare,
                };
                let left = CFString::new(&left.to_string_lossy());
                let right = CFString::new(&right.to_string_lossy());
                // SAFETY: both owned strings remain live throughout this comparison.
                unsafe {
                    CFStringCompare(
                        left.as_concrete_TypeRef(),
                        right.as_concrete_TypeRef(),
                        kCFCompareCaseInsensitive | kCFCompareNonliteral,
                    ) == CFComparisonResult::EqualTo
                }
            }
            (left, right) => left == right,
        }))
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn equivalent_spellings_are_one_name() -> Result<bool, Refused> {
    Ok(true)
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
        assert_eq!(equivalent_spellings_are_one_name(), Ok(true));
    }

    #[test]
    fn unavailable_path_comparison_preserves_component_and_accent_boundaries() {
        assert!(paths_may_alias(
            Path::new("/data/CAF\u{c9}"),
            Path::new("/data/cafe\u{301}/child"),
        )
        .unwrap());
        assert!(!paths_may_alias(Path::new("/data/caf\u{e9}"), Path::new("/data/cafe")).unwrap());
        assert!(
            !paths_may_alias(Path::new("/data/caf\u{e9}"), Path::new("/data/caf\u{e9}2"),).unwrap()
        );
    }
}
