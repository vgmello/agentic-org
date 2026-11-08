//! The organization-name rule, in one place for every server that checks it.
//!
//! The host checks a new organization's name and its uniqueness on this machine; the office
//! server checks it again before storing it. Both call [`OrganizationName::parse`], so the two
//! can't drift. The rule is the one [`ORGANIZATION_NAME_MAX_CHARS`] documents, which the app's
//! TypeScript pre-check mirrors.

use std::fmt;

use crate::{ApiError, ErrorCode, ORGANIZATION_NAME_MAX_CHARS};

/// A valid organization name: trimmed, 1..=[`ORGANIZATION_NAME_MAX_CHARS`] characters, no
/// control characters.
///
/// Only [`OrganizationName::parse`] makes one, so holding one means the name passed the rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrganizationName {
    name: String,
    key: String,
}

impl OrganizationName {
    /// Checks `raw` against the rule and returns the name as it would be stored.
    ///
    /// - Leading and trailing Unicode `White_Space` is removed (`str::trim`).
    /// - The result must have 1..=[`ORGANIZATION_NAME_MAX_CHARS`] Unicode scalar values.
    /// - It must have no control characters (`char::is_control`: U+0000–U+001F and
    ///   U+007F–U+009F).
    pub fn parse(raw: &str) -> Result<Self, NameError> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name.chars().count() > ORGANIZATION_NAME_MAX_CHARS {
            return Err(NameError::TooLong);
        }
        if name.chars().any(char::is_control) {
            return Err(NameError::ControlChar);
        }
        Ok(Self {
            name: name.to_owned(),
            key: name.to_lowercase(),
        })
    }

    /// The trimmed name, as it's stored and shown.
    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// The key two names are compared by for uniqueness, which ignores case: the Unicode
    /// lowercase of the trimmed name (`str::to_lowercase`). There's no Unicode normalization,
    /// so a composed and a decomposed spelling of the same accented name are different keys.
    ///
    /// Compare keys, never names. The key is only for comparing; store and show
    /// [`OrganizationName::as_str`].
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The trimmed name, by value.
    pub fn into_string(self) -> String {
        self.name
    }
}

impl fmt::Display for OrganizationName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Why a name breaks the rule. On the wire every case is [`ErrorCode::InvalidName`], with
/// [`NameError::message`] as the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NameError {
    /// Nothing is left after trimming.
    Empty,
    /// More than [`ORGANIZATION_NAME_MAX_CHARS`] characters after trimming.
    TooLong,
    /// The name contains a control character.
    ControlChar,
}

impl NameError {
    /// The message for people, the same text the app's pre-check shows.
    pub fn message(&self) -> String {
        match self {
            Self::Empty => "Give the organization a name.".to_owned(),
            Self::TooLong => format!("Use {ORGANIZATION_NAME_MAX_CHARS} characters or fewer."),
            Self::ControlChar => "The name can't contain control characters.".to_owned(),
        }
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for NameError {}

impl From<NameError> for ApiError {
    fn from(e: NameError) -> Self {
        ApiError::new(ErrorCode::InvalidName, e.message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_white_space_and_keeps_inner_spaces() {
        let n = OrganizationName::parse(" \t\u{3000}Acme  Labs\u{85}\n").unwrap();
        assert_eq!(n.as_str(), "Acme  Labs");
        assert_eq!(n.to_string(), "Acme  Labs");
        assert_eq!(n.clone().into_string(), "Acme  Labs");
    }

    #[test]
    fn empty_or_only_white_space_is_empty() {
        assert_eq!(OrganizationName::parse(""), Err(NameError::Empty));
        assert_eq!(
            OrganizationName::parse(" \u{a0}\u{2028} "),
            Err(NameError::Empty)
        );
    }

    #[test]
    fn length_counts_chars_after_trimming() {
        let max = "é".repeat(ORGANIZATION_NAME_MAX_CHARS);
        assert_eq!(
            OrganizationName::parse(&format!("  {max}  "))
                .unwrap()
                .as_str(),
            max
        );
        let over = "a".repeat(ORGANIZATION_NAME_MAX_CHARS + 1);
        assert_eq!(OrganizationName::parse(&over), Err(NameError::TooLong));
    }

    #[test]
    fn control_characters_are_refused_inside_but_trimmed_at_the_edges() {
        for c in ['\u{0}', '\u{1b}', '\u{7f}', '\u{9f}', '\t'] {
            let raw = format!("Ac{c}me");
            assert_eq!(
                OrganizationName::parse(&raw),
                Err(NameError::ControlChar),
                "{c:?}"
            );
        }
        // Tab and U+0085 are White_Space, so they're trimmed at the edges first.
        assert_eq!(
            OrganizationName::parse("\tAcme\u{85}").unwrap().as_str(),
            "Acme"
        );
        // U+FEFF isn't White_Space (unlike JavaScript's trim) and isn't a control character.
        assert_eq!(
            OrganizationName::parse("\u{feff}Acme").unwrap().as_str(),
            "\u{feff}Acme"
        );
    }

    #[test]
    fn key_ignores_case_and_edges() {
        let a = OrganizationName::parse("  ACME Labs ").unwrap();
        let b = OrganizationName::parse("acme labs").unwrap();
        assert_eq!(a.key(), b.key());
        assert_ne!(a.as_str(), b.as_str());
        assert_eq!(
            OrganizationName::parse("ÉCOLE").unwrap().key(),
            OrganizationName::parse("école").unwrap().key()
        );
        assert_ne!(a.key(), OrganizationName::parse("Acme Lab").unwrap().key());
    }

    #[test]
    fn errors_map_to_invalid_name_with_the_app_texts() {
        let e: ApiError = NameError::Empty.into();
        assert_eq!(e.code, ErrorCode::InvalidName);
        assert_eq!(e.message, "Give the organization a name.");
        assert_eq!(NameError::TooLong.message(), "Use 64 characters or fewer.");
        assert_eq!(
            NameError::ControlChar.to_string(),
            "The name can't contain control characters."
        );
    }
}
