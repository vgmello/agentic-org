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
        if display.chars().any(char::is_control) {
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
        for good in ["a", "Ålice", "ada lovelace", &"é".repeat(64)] {
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
