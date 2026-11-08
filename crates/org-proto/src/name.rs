//! The organization-name rule, in one place for every server that checks it.
//!
//! The host checks a new organization's name and its uniqueness on this machine; the office
//! server checks it again before storing it. Both call [`OrganizationName::parse`], so the two
//! can't drift. The rule is the one [`ORGANIZATION_NAME_MAX_CHARS`] documents, which the app's
//! TypeScript pre-check mirrors.

use std::fmt;

use crate::{ApiError, ErrorCode, ORGANIZATION_NAME_MAX_CHARS};

/// A valid organization name: trimmed, 1..=[`ORGANIZATION_NAME_MAX_CHARS`] characters, no
/// control characters, and no format characters (Cf) or default-ignorable code points, plus
/// U+2028/U+2029, except U+200C, U+200D, U+FE0E and U+FE0F. [`OrganizationName::parse`] has the
/// details.
///
/// Refusing those characters stops invisible look-alikes made with them: "Acme" followed by a
/// tag character can't become a second "Acme". It doesn't stop every pair of names that look
/// the same: homoglyphs (a Latin "a" and a Cyrillic "а"), composed and decomposed spellings
/// (there's no normalization), spaces other than U+0020 inside a name (U+00A0, the nearly
/// invisible U+200A hair space), visible blanks such as U+2800 Braille pattern blank, and the
/// four allowed characters, which are invisible outside the scripts and emoji that need them
/// ("Acme" followed by U+FE0F), still make different names.
///
/// Only [`OrganizationName::parse`] makes one, so holding one means the name passed the rule.
///
/// It deliberately has no `PartialEq`, `Eq` or `Hash`: a derived comparison would be
/// case-sensitive, which is the wrong test for duplicates. Compare [`OrganizationName::key`]s.
///
/// ```compile_fail,E0369
/// let a = org_proto::OrganizationName::parse("Acme").unwrap();
/// let b = org_proto::OrganizationName::parse("Acme").unwrap();
/// let _same = a == b;
/// ```
///
/// ```compile_fail,E0277
/// fn needs_hash<T: std::hash::Hash>(_: &T) {}
/// needs_hash(&org_proto::OrganizationName::parse("Acme").unwrap());
/// ```
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
    /// - It must have no format characters (Unicode General_Category Cf) or default-ignorable
    ///   code points (`Default_Ignorable_Code_Point`), plus U+2028/U+2029 (line and paragraph
    ///   separator), except U+200C (zero-width non-joiner), which Persian and Indic names need,
    ///   and U+200D (zero-width joiner), U+FE0E and U+FE0F (text and emoji presentation
    ///   selectors), which emoji need. That covers zero-width spaces and joiners other than
    ///   U+200C and U+200D, direction marks, embeddings, overrides and isolates, the byte-order
    ///   mark, tag characters (U+E0001 and U+E0020–U+E007F), Hangul fillers and the other
    ///   variation selectors, and the code points Unicode reserves as default-ignorable but
    ///   hasn't assigned yet. It also refuses a few visible format marks, such as the Arabic
    ///   number signs (U+0600–U+0605) and end of ayah (U+06DD). The ranges come from Unicode
    ///   17.0.
    ///
    /// The last rule stops invisible look-alikes made with these characters, and stops a name
    /// from using direction controls to reorder the text around it. Right-to-left letters still
    /// follow the normal bidirectional rules, so show names isolated (e.g. `<bdi>`, or U+2068
    /// first strong isolate and U+2069 pop directional isolate). Names travel to other Macs
    /// (invitations), so this matters. It doesn't make look-alike names impossible: homoglyphs,
    /// composed and decomposed spellings (there's no normalization), spaces other than U+0020
    /// (U+00A0, U+200A), visible blanks such as U+2800, and U+200C, U+200D, U+FE0E and U+FE0F
    /// used where no script or emoji needs them still pass, and are different names.
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
    /// lowercase of the trimmed name (`str::to_lowercase`). It isn't full Unicode case folding:
    /// "STRASSE" and "straße" differ, and so does a Greek word ending in final sigma ("ς") from
    /// the same word typed with "σ" at the end. "İ" (dotted capital I) lowercases to "i̇" (i and
    /// a combining dot), so it differs from "i" too, as it would under full case folding; only
    /// Turkish and Azerbaijani rules match them. There's no Unicode normalization either, so a
    /// composed and a decomposed spelling of the same accented name are different keys.
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
    /// The name contains a format character (Cf) or a default-ignorable code point, or U+2028
    /// or U+2029, other than U+200C, U+200D, U+FE0E and U+FE0F. See [`OrganizationName::parse`].
    InvisibleChar,
}

impl NameError {
    /// The message for people, the same text the app's pre-check shows.
    ///
    /// [`NameError::InvisibleChar`] is new: the app's pre-check doesn't have it yet and will
    /// follow, with the same text.
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

/// Characters a name may not contain: the union of General_Category=Cf and
/// `Default_Ignorable_Code_Point` (Unicode 17.0), plus U+2028 and U+2029, minus U+200C (zero-width
/// non-joiner), which Persian and Indic names need, and U+200D (zero-width joiner), U+FE0E and
/// U+FE0F (text and emoji presentation selectors), which emoji need.
/// Default-ignorable code points that aren't assigned yet (U+2065, U+FFF0–U+FFF8 and most of
/// U+E0000–U+E0FFF) are included, so characters Unicode assigns there later are refused too.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}' // soft hyphen
            | '\u{034F}' // combining grapheme joiner
            | '\u{0600}'..='\u{0605}' // Arabic number signs and other prepended marks
            | '\u{061C}' // Arabic letter mark
            | '\u{06DD}' // Arabic end of ayah
            | '\u{070F}' // Syriac abbreviation mark
            | '\u{0890}'..='\u{0891}' // Arabic pound and piastre mark above
            | '\u{08E2}' // Arabic disputed end of ayah
            | '\u{115F}'..='\u{1160}' // Hangul choseong and jungseong fillers
            | '\u{17B4}'..='\u{17B5}' // Khmer inherent vowels
            | '\u{180B}'..='\u{180F}' // Mongolian free variation selectors, vowel separator
            | '\u{200B}' // zero-width space
            | '\u{200E}'..='\u{200F}' // left-to-right and right-to-left marks
            | '\u{2028}'..='\u{202E}' // line and paragraph separators, embeddings, overrides
            | '\u{2060}'..='\u{206F}' // word joiner, invisible operators, isolates, deprecated
            | '\u{3164}' // Hangul filler
            | '\u{FE00}'..='\u{FE0D}' // variation selectors 1-14
            | '\u{FEFF}' // zero-width no-break space (byte-order mark)
            | '\u{FFA0}' // halfwidth Hangul filler
            | '\u{FFF0}'..='\u{FFFB}' // reserved, interlinear annotation
            | '\u{110BD}' // Kaithi number sign
            | '\u{110CD}' // Kaithi number sign above
            | '\u{13430}'..='\u{1343F}' // Egyptian hieroglyph format controls
            | '\u{1BCA0}'..='\u{1BCA3}' // shorthand format controls
            | '\u{1D173}'..='\u{1D17A}' // musical symbol beams, ties, slurs and phrases
            | '\u{E0000}'..='\u{E0FFF}' // tags, variation selectors 17-256, reserved
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

    /// The refused set, written out on its own (Unicode 17.0's Cf and
    /// `Default_Ignorable_Code_Point` data, plus U+2028/U+2029, minus U+200C, U+200D, U+FE0E and
    /// U+FE0F)
    /// so a slip in `is_invisible` doesn't also slip here.
    const REFUSED: &[(u32, u32)] = &[
        (0x00AD, 0x00AD),
        (0x034F, 0x034F),
        (0x0600, 0x0605),
        (0x061C, 0x061C),
        (0x06DD, 0x06DD),
        (0x070F, 0x070F),
        (0x0890, 0x0891),
        (0x08E2, 0x08E2),
        (0x115F, 0x1160),
        (0x17B4, 0x17B5),
        (0x180B, 0x180F),
        (0x200B, 0x200B),
        (0x200E, 0x200F),
        (0x2028, 0x202E),
        (0x2060, 0x206F),
        (0x3164, 0x3164),
        (0xFE00, 0xFE0D),
        (0xFEFF, 0xFEFF),
        (0xFFA0, 0xFFA0),
        (0xFFF0, 0xFFFB),
        (0x110BD, 0x110BD),
        (0x110CD, 0x110CD),
        (0x13430, 0x1343F),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0000, 0xE0FFF),
    ];

    #[test]
    fn every_refused_code_point_is_refused_inside_a_name() {
        let mut checked = 0;
        for &(first, last) in REFUSED {
            for cp in first..=last {
                let c = char::from_u32(cp).unwrap();
                let raw = format!("Ac{c}me");
                assert_eq!(
                    OrganizationName::parse(&raw).unwrap_err(),
                    NameError::InvisibleChar,
                    "U+{cp:04X}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 4204);
        assert_eq!(
            NameError::InvisibleChar.message(),
            "The name can't contain invisible or direction-changing characters."
        );
    }

    /// `is_invisible` matches the refused ranges exactly, across every Unicode scalar value.
    #[test]
    fn is_invisible_is_exactly_the_refused_ranges() {
        for c in (0..=0x10FFFF).filter_map(char::from_u32) {
            let cp = c as u32;
            let refused = REFUSED.iter().any(|&(a, b)| (a..=b).contains(&cp));
            assert_eq!(is_invisible(c), refused, "U+{cp:04X}");
        }
    }

    #[test]
    fn allowed_characters_and_the_neighbours_of_each_range_are_accepted() {
        // The four exceptions: U+200C, which Persian and Indic names need, and the three emoji
        // need.
        let mut ok = vec![0x200C, 0x200D, 0xFE0E, 0xFE0F];
        // The code point just before and after each refused range. Skipped because they're
        // unassigned in Unicode 17.0 (U+DFFFF is a noncharacter): U+05FF, U+070E, U+0892,
        // U+FEFE, U+FF00, U+FFEF, U+110CC, U+110CE, U+1BCA4, U+DFFFF and U+E1000. None is a
        // control character.
        ok.extend([
            0x00AC, 0x00AE, // around U+00AD
            0x034E, 0x0350, // around U+034F
            0x0606, // after U+0600–U+0605
            0x061B, 0x061D, // around U+061C
            0x06DC, 0x06DE, // around U+06DD
            0x0710, // after U+070F
            0x088F, // before U+0890–U+0891
            0x08E1, 0x08E3, // around U+08E2
            0x115E, 0x1161, // around U+115F–U+1160
            0x17B3, 0x17B6, // around U+17B4–U+17B5
            0x180A, 0x1810, // around U+180B–U+180F
            0x200A, // before U+200B (U+200C, after it, is above)
            0x2010, // after U+200E–U+200F
            0x2027, 0x202F, // around U+2028–U+202E
            0x205F, 0x2070, // around U+2060–U+206F
            0x3163, 0x3165, // around U+3164
            0xFDFF, // before U+FE00–U+FE0D (U+FE0E, after it, is above)
            0xFF9F, 0xFFA1, // around U+FFA0
            0xFFFC, // after U+FFF0–U+FFFB
            0x110BC, 0x110BE, // around U+110BD
            0x1342F, 0x13440, // around U+13430–U+1343F
            0x1BC9F, // before U+1BCA0–U+1BCA3
            0x1D172, 0x1D17B, // around U+1D173–U+1D17A
        ]);
        for cp in ok {
            let c = char::from_u32(cp).unwrap();
            assert!(!c.is_control(), "U+{cp:04X}");
            let raw = format!("Ac{c}me");
            assert_eq!(
                OrganizationName::parse(&raw).unwrap().as_str(),
                raw,
                "U+{cp:04X}"
            );
        }
        // Emoji: a zero-width joiner sequence, and a heart with the emoji presentation selector.
        // Persian: "می‌خواهم" ("I want"), whose ZWNJ keeps the prefix "می" from joining.
        for name in [
            "👩\u{200d}💻 Labs",
            "\u{2764}\u{fe0f}",
            "Acme \u{2764}\u{fe0f}",
            "\u{645}\u{6cc}\u{200c}\u{62e}\u{648}\u{627}\u{647}\u{645}",
        ] {
            assert_eq!(OrganizationName::parse(name).unwrap().as_str(), name);
        }
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
        // Not full case folding: "STRASSE"/"straße" and "ΟΔΟΣ"/"οδοσ" fold together but have
        // different keys. "İ" lowercases (and folds) to "i̇", so it differs from "i" too.
        for (x, y) in [("STRASSE", "straße"), ("ΟΔΟΣ", "οδοσ"), ("İ", "i")] {
            assert_ne!(
                OrganizationName::parse(x).unwrap().key(),
                OrganizationName::parse(y).unwrap().key(),
                "{x} {y}"
            );
        }
    }

    /// The canonical vectors; the app's pre-check moves to this file (follow-up).
    #[test]
    fn shared_vectors() {
        let file: serde_json::Value =
            serde_json::from_str(include_str!("../test-vectors/organization-names.json")).unwrap();
        let vectors = file["vectors"].as_array().unwrap();
        assert_eq!(vectors.len(), 24);
        for v in vectors {
            let input = v["input"].as_str().unwrap();
            let error = v.get("error").map(|e| e.as_str().expect(input));
            match v.get("name").and_then(|n| n.as_str()) {
                Some(name) => {
                    assert_eq!(error, None, "{input:?}: an accepted vector has no error");
                    assert_eq!(OrganizationName::parse(input).unwrap().as_str(), name);
                }
                None => {
                    let expected = match error {
                        Some("empty") => NameError::Empty,
                        Some("too_long") => NameError::TooLong,
                        Some("control") => NameError::ControlChar,
                        Some("invisible") => NameError::InvisibleChar,
                        other => panic!("{input:?}: bad or missing error {other:?}"),
                    };
                    assert_eq!(
                        OrganizationName::parse(input).unwrap_err(),
                        expected,
                        "{input:?}"
                    );
                }
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
