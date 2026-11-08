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
///
/// It deliberately has no `PartialEq`, `Eq` or `Hash`: a derived comparison would be
/// case-sensitive, which is the wrong test for duplicates. Compare [`OrganizationName::key`]s.
#[derive(Debug, Clone)]
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
    /// - It must have no invisible or direction-changing characters (the list is on the private `is_invisible` below), so
    ///   two names that look the same can't be two organizations, and a name can't reverse how
    ///   the text around it displays. Names travel to other Macs (invitations), so this matters.
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
        if name.chars().any(is_invisible) {
            return Err(NameError::InvisibleChar);
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
    /// lowercase of the trimmed name (`str::to_lowercase`). It isn't full Unicode case folding
    /// ("STRASSE" and "straße" differ), and there's no Unicode normalization, so a composed and a
    /// decomposed spelling of the same accented name are different keys.
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
    /// The name contains an invisible or direction-changing character.
    InvisibleChar,
}

impl NameError {
    /// The message for people, the same text the app's pre-check shows.
    pub fn message(&self) -> String {
        match self {
            Self::Empty => "Give the organization a name.".to_owned(),
            Self::TooLong => format!("Use {ORGANIZATION_NAME_MAX_CHARS} characters or fewer."),
            Self::ControlChar => "The name can't contain control characters.".to_owned(),
            Self::InvisibleChar => {
                "The name can't contain invisible or direction-changing characters.".to_owned()
            }
        }
    }
}

/// Characters a name may not contain because they're invisible or change text direction: the
/// Unicode format characters (category Cf) that matter here, and the line and paragraph
/// separators. U+200D (zero-width joiner) is allowed, because emoji sequences need it.
///
/// - U+00AD soft hyphen, U+061C Arabic letter mark, U+180E Mongolian vowel separator
/// - U+200B–U+200C and U+200E–U+200F: zero-width space and non-joiner, direction marks
/// - U+2028–U+202E: line and paragraph separators, direction embeddings and overrides
/// - U+2060–U+2064 and U+2066–U+206F: word joiner, invisible operators, direction isolates
///   and deprecated format characters
/// - U+FEFF zero-width no-break space (byte-order mark), U+FFF9–U+FFFB interlinear annotation
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200C}'
            | '\u{200E}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
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
        assert_eq!(OrganizationName::parse("").unwrap_err(), NameError::Empty);
        assert_eq!(
            OrganizationName::parse(" \u{a0}\u{2028} ").unwrap_err(),
            NameError::Empty
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
        assert_eq!(
            OrganizationName::parse(&over).unwrap_err(),
            NameError::TooLong
        );
    }

    #[test]
    fn control_characters_are_refused_inside_but_trimmed_at_the_edges() {
        for c in ['\u{0}', '\u{1b}', '\u{7f}', '\u{9f}', '\t'] {
            let raw = format!("Ac{c}me");
            assert_eq!(
                OrganizationName::parse(&raw).unwrap_err(),
                NameError::ControlChar,
                "{c:?}"
            );
        }
        // Tab and U+0085 are White_Space, so they're trimmed at the edges first.
        assert_eq!(
            OrganizationName::parse("\tAcme\u{85}").unwrap().as_str(),
            "Acme"
        );
        // U+FEFF isn't White_Space (unlike JavaScript's trim), so it isn't trimmed: it's refused.
        assert_eq!(
            OrganizationName::parse("\u{feff}Acme").unwrap_err(),
            NameError::InvisibleChar
        );
    }

    #[test]
    fn invisible_and_direction_changing_characters_are_refused() {
        for c in [
            '\u{ad}', '\u{61c}', '\u{180e}', '\u{200b}', '\u{200c}', '\u{200e}', '\u{200f}',
            '\u{2028}', '\u{2029}', '\u{202a}', '\u{202e}', '\u{2060}', '\u{2064}', '\u{2066}',
            '\u{2069}', '\u{206f}', '\u{feff}', '\u{fff9}', '\u{fffb}',
        ] {
            let raw = format!("Ac{c}me");
            assert_eq!(
                OrganizationName::parse(&raw).unwrap_err(),
                NameError::InvisibleChar,
                "{c:?}"
            );
        }
        // The neighbours of the refused ranges are fine, and so is an emoji sequence (U+200D).
        for ok in [
            "Ac\u{200d}me",
            "👩\u{200d}💻 Labs",
            "Ac\u{2027}me",
            "Ac\u{205f}me",
            "Ac\u{fffc}me",
        ] {
            assert!(OrganizationName::parse(ok).is_ok(), "{ok:?}");
        }
        assert_eq!(
            NameError::InvisibleChar.message(),
            "The name can't contain invisible or direction-changing characters."
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

    /// The vectors the app's TypeScript pre-check is tested against too.
    #[test]
    fn shared_vectors() {
        let file: serde_json::Value =
            serde_json::from_str(include_str!("../test-vectors/organization-names.json")).unwrap();
        let vectors = file["vectors"].as_array().unwrap();
        assert!(vectors.len() >= 14);
        for v in vectors {
            let input = v["input"].as_str().unwrap();
            match v.get("name").and_then(|n| n.as_str()) {
                Some(name) => assert_eq!(OrganizationName::parse(input).unwrap().as_str(), name),
                None => assert!(OrganizationName::parse(input).is_err(), "{input:?}"),
            }
        }
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
