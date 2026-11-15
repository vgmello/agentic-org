use caseless::Caseless;
use unicode_normalization::UnicodeNormalization;

/// Unicode default full fold (non-Turkic), with NFC on both sides of the fold.
pub(crate) fn normalize_login(login: &str) -> String {
    login.nfc().default_case_fold().nfc().collect()
}

#[cfg(test)]
mod tests;
