//! What counts as an organization name. The office owns this rule; clients treat the
//! server as the source of truth.

use std::fmt;

pub const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    ControlCharacter,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the name is empty"),
            Self::TooLong => write!(f, "the name is longer than {MAX_NAME_CHARS} characters"),
            Self::ControlCharacter => f.write_str("the name contains a control character"),
        }
    }
}

/// A validated organization name: trimmed, 1..=64 characters, no control characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrganizationName(String);

impl OrganizationName {
    pub(crate) fn parse(raw: &str) -> Result<Self, NameError> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name.chars().any(char::is_control) {
            return Err(NameError::ControlCharacter);
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(NameError::TooLong);
        }
        Ok(Self(name.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The key two names are compared by: "Acme" and " acme " are the same organization.
    ///
    /// Limits, deliberately: plain Unicode lowercasing, no normalization (composed and
    /// decomposed "É" differ), and only control characters (category Cc) are rejected, so
    /// zero-width and bidi characters are allowed. The key is stored, so a change here
    /// needs a migration.
    #[allow(dead_code)] // the HQ rules compare names by this key
    pub(crate) fn uniqueness_key(&self) -> String {
        self.0.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_accepts_ordinary_names() {
        assert_eq!(
            OrganizationName::parse("  Acme Co  ").unwrap().as_str(),
            "Acme Co"
        );
        assert!(OrganizationName::parse("Ünïcode 組織").is_ok());
    }

    #[test]
    fn rejects_empty_control_and_overlong() {
        assert_eq!(OrganizationName::parse(" \t "), Err(NameError::Empty));
        assert_eq!(
            OrganizationName::parse("a\nb"),
            Err(NameError::ControlCharacter)
        );
        assert_eq!(
            OrganizationName::parse("a\0b"),
            Err(NameError::ControlCharacter)
        );
        assert!(OrganizationName::parse(&"x".repeat(MAX_NAME_CHARS)).is_ok());
        assert_eq!(
            OrganizationName::parse(&"x".repeat(MAX_NAME_CHARS + 1)),
            Err(NameError::TooLong)
        );
        // Characters, not bytes.
        assert!(OrganizationName::parse(&"組".repeat(MAX_NAME_CHARS)).is_ok());
    }

    #[test]
    fn uniqueness_ignores_case_and_outer_space() {
        let a = OrganizationName::parse("Acme").unwrap();
        let b = OrganizationName::parse("  aCME ").unwrap();
        assert_eq!(a.uniqueness_key(), b.uniqueness_key());
    }
}
