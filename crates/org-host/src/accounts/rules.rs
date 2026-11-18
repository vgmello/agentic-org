//! Host-authoritative username and password rules for browser registration and
//! password changes (A3 plan R2, Revision 3 item 5). Clients may pre-check, but
//! only these decide.
use unicode_normalization::UnicodeNormalization;

use super::identity::LoginKey;

/// A username is 1..=64 Unicode scalar values after NFC.
pub(crate) const USERNAME_MAX_SCALARS: usize = 64;
/// A password is at least 12 Unicode scalar values after NFC.
pub(crate) const PASSWORD_MIN_SCALARS: usize = 12;
/// A password is at most 1024 UTF-8 bytes as received.
pub(crate) const PASSWORD_MAX_BYTES: usize = 1024;

/// Which username rule failed; the reply names it as `{field, rule}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UsernameRule {
    Length,
    EdgeWhitespace,
    Control,
}

/// A checked username: its NFC display form and its lookup key.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Username {
    display: String,
    key: LoginKey,
}
impl Username {
    pub(crate) fn new(input: &str) -> Result<Self, UsernameRule> {
        let display: String = input.nfc().collect();
        let scalars = display.chars().count();
        if scalars == 0 || scalars > USERNAME_MAX_SCALARS {
            return Err(UsernameRule::Length);
        }
        if display.chars().any(is_invisible_or_control) {
            return Err(UsernameRule::Control);
        }
        let edge = |c: Option<char>| c.is_some_and(char::is_whitespace);
        if edge(display.chars().next()) || edge(display.chars().next_back()) {
            return Err(UsernameRule::EdgeWhitespace);
        }
        // Case folding can expand a short name past the key's byte bound.
        let key = LoginKey::new(&display).map_err(|_| UsernameRule::Length)?;
        Ok(Self { display, key })
    }
    pub(crate) fn display(&self) -> &str {
        &self.display
    }
    pub(crate) fn key(&self) -> &LoginKey {
        &self.key
    }
}

/// General Category Format (Cf) code points, Unicode 16.0, as inclusive
/// ranges. Listed explicitly so the rule needs no Unicode-data dependency; a
/// Cf character added by a later Unicode version is not refused until it is
/// added here.
const FORMAT_RANGES: &[(char, char)] = &[
    ('\u{00AD}', '\u{00AD}'),
    ('\u{0600}', '\u{0605}'),
    ('\u{061C}', '\u{061C}'),
    ('\u{06DD}', '\u{06DD}'),
    ('\u{070F}', '\u{070F}'),
    ('\u{0890}', '\u{0891}'),
    ('\u{08E2}', '\u{08E2}'),
    ('\u{180E}', '\u{180E}'),
    ('\u{200B}', '\u{200F}'),
    ('\u{202A}', '\u{202E}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206F}'),
    ('\u{FEFF}', '\u{FEFF}'),
    ('\u{FFF9}', '\u{FFFB}'),
    ('\u{110BD}', '\u{110BD}'),
    ('\u{110CD}', '\u{110CD}'),
    ('\u{13430}', '\u{1343F}'),
    ('\u{1BCA0}', '\u{1BCA3}'),
    ('\u{1D173}', '\u{1D17A}'),
    ('\u{E0001}', '\u{E0001}'),
    ('\u{E0020}', '\u{E007F}'),
];

/// A username refuses controls (Cc), format characters (Cf: zero-width,
/// joiners, bidirectional embeddings, overrides and isolates, BOM) and the
/// line and paragraph separators U+2028/U+2029, so two names that look the
/// same cannot differ only by invisible characters.
fn is_invisible_or_control(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{2028}' | '\u{2029}')
        || FORMAT_RANGES.iter().any(|&(lo, hi)| (lo..=hi).contains(&c))
}

/// Which password rule failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PasswordRule {
    TooShort,
    TooLong,
}

/// Checks a new password before it is hashed. The byte bound is checked first,
/// so an oversized input is never normalized.
pub(crate) fn check_new_password(password: &str) -> Result<(), PasswordRule> {
    if password.len() > PASSWORD_MAX_BYTES {
        return Err(PasswordRule::TooLong);
    }
    if password.nfc().count() < PASSWORD_MIN_SCALARS {
        return Err(PasswordRule::TooShort);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_1_to_64_nfc_scalars_without_edge_space_or_controls() {
        // Visible neighbours of the refused ranges stay allowed.
        for good in [
            "a",
            "Ålice",
            "ada lovelace",
            &"é".repeat(64),
            "al\u{2010}ice",
            "al\u{2027}ice\u{2030}",
            "\u{1F469}\u{1F4BB}",
        ] {
            assert_eq!(Username::new(good).unwrap().display(), good);
        }
        // Decomposed input is counted and stored after NFC.
        let decomposed = "e\u{301}".repeat(64);
        assert_eq!(
            Username::new(&decomposed).unwrap().display(),
            "é".repeat(64)
        );
        assert_eq!(
            Username::new("Ålice").unwrap().key(),
            &LoginKey::new("A\u{30A}LICE").unwrap()
        );
        for (bad, rule) in [
            ("", UsernameRule::Length),
            (&"a".repeat(65)[..], UsernameRule::Length),
            (" alice", UsernameRule::EdgeWhitespace),
            ("alice\u{3000}", UsernameRule::EdgeWhitespace),
            ("al\u{7}ice", UsernameRule::Control),
            ("al\nice", UsernameRule::Control),
            ("al\u{7F}ice", UsernameRule::Control),
            ("al\u{85}ice", UsernameRule::Control),
            // Zero-width space, non-joiner, joiner, LRM, RLM.
            ("al\u{200B}ice", UsernameRule::Control),
            ("al\u{200C}ice", UsernameRule::Control),
            ("al\u{200D}ice", UsernameRule::Control),
            ("\u{1F469}\u{200D}\u{1F4BB}", UsernameRule::Control),
            ("alice\u{200E}", UsernameRule::Control),
            ("al\u{200F}ice", UsernameRule::Control),
            // Bidirectional embeddings and overrides (U+202E spoofs "alice.exe").
            ("al\u{202A}ice", UsernameRule::Control),
            ("\u{202E}exe.ecila", UsernameRule::Control),
            // Isolates, word joiner, invisible operators, BOM, soft hyphen, tags.
            ("al\u{2066}ice\u{2069}", UsernameRule::Control),
            ("al\u{2060}ice", UsernameRule::Control),
            ("al\u{2064}ice", UsernameRule::Control),
            ("\u{FEFF}alice", UsernameRule::Control),
            ("al\u{AD}ice", UsernameRule::Control),
            ("alice\u{E0041}", UsernameRule::Control),
            // Line and paragraph separators.
            ("al\u{2028}ice", UsernameRule::Control),
            ("al\u{2029}ice", UsernameRule::Control),
        ] {
            assert_eq!(Username::new(bad), Err(rule), "{bad:?}");
        }
        // 64 scalars whose fold (U+1F80 -> U+1F00 U+03B9, 5 bytes) exceeds the
        // key's 256-byte bound.
        assert_eq!(
            Username::new(&"\u{1F80}".repeat(64)),
            Err(UsernameRule::Length)
        );
    }

    #[test]
    fn passwords_are_12_nfc_scalars_to_1024_bytes() {
        assert_eq!(
            check_new_password(&"a".repeat(11)),
            Err(PasswordRule::TooShort)
        );
        assert_eq!(check_new_password(&"a".repeat(12)), Ok(()));
        // 11 decomposed pairs are 22 scalars before NFC but 11 after.
        assert_eq!(
            check_new_password(&"e\u{301}".repeat(11)),
            Err(PasswordRule::TooShort)
        );
        assert_eq!(check_new_password(&"é".repeat(12)), Ok(()));
        // No byte minimum: 12 four-byte scalars pass; 1024 bytes pass, 1025 do not.
        assert_eq!(check_new_password(&"😀".repeat(12)), Ok(()));
        assert_eq!(check_new_password(&"a".repeat(1024)), Ok(()));
        assert_eq!(
            check_new_password(&"a".repeat(1025)),
            Err(PasswordRule::TooLong)
        );
    }
}
