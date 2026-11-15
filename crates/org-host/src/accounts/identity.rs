//! Stable checked identity values; no registry or workflow dependencies.
use super::login::normalize_login;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct InvalidIdentity;

/// NFC/full-default-fold/NFC login, bounded by normalized UTF-8 bytes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LoginKey(String);
impl LoginKey {
    pub(crate) fn new(display: &str) -> Result<Self, InvalidIdentity> {
        let key = normalize_login(display);
        if key.is_empty() || key.len() > 256 {
            return Err(InvalidIdentity);
        }
        Ok(Self(key))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A canonical account ULID, using the shared protocol's exact spelling policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccountId(String);
impl AccountId {
    pub(crate) fn parse(value: &str) -> Result<Self, InvalidIdentity> {
        org_proto::ulid_time_ms(value).ok_or(InvalidIdentity)?;
        Ok(Self(value.to_owned()))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_bounds_apply_after_normalization_to_utf8_bytes() {
        for good in ["a".into(), "a".repeat(256), "é".repeat(128)] {
            assert_eq!(LoginKey::new(&good).unwrap().as_str(), good);
        }
        for bad in [String::new(), "a".repeat(257), "é".repeat(129)] {
            assert_eq!(LoginKey::new(&bad), Err(InvalidIdentity));
        }
        // U+0130 expands to i + COMBINING DOT ABOVE (three UTF-8 bytes).
        assert_eq!(LoginKey::new(&"İ".repeat(86)), Err(InvalidIdentity));
        // Decomposed input exceeds 256 bytes, but its normalized key fits.
        assert_eq!(
            LoginKey::new(&"e\u{301}".repeat(128)).unwrap().as_str(),
            "é".repeat(128)
        );
    }

    #[test]
    fn account_ids_use_the_shared_canonical_ulid_policy() {
        for good in [
            "00000000000000000000000000",
            "7ZZZZZZZZZZZZZZZZZZZZZZZZZ",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        ] {
            assert_eq!(AccountId::parse(good).unwrap().as_str(), good);
        }
        for bad in [
            "",
            "01ARZ3NDEKTSV4RRFFQ69G5FA",
            "01ARZ3NDEKTSV4RRFFQ69G5FAVV",
            "81ARZ3NDEKTSV4RRFFQ69G5FAV",
            "01arz3ndektsv4rrffq69g5fav",
            "01ARZ3NDEKTSV4RRFFQ69G5FAI",
            "01ARZ3NDEKTSV4RRFFQ69G5FAL",
            "01ARZ3NDEKTSV4RRFFQ69G5FAO",
            "01ARZ3NDEKTSV4RRFFQ69G5FAU",
            "01ARZ3NDEKTSV4RRFFQ69G5FÀ",
        ] {
            assert_eq!(AccountId::parse(bad), Err(InvalidIdentity), "{bad}");
        }
    }

    #[test]
    fn checked_login_keys_follow_every_pinned_normalization_vector() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../crates/org-host-control/test-vectors/journal.json"
        ))
        .unwrap();
        for case in vectors["login_normalization"].as_array().unwrap() {
            let key = LoginKey::new(case["input"].as_str().unwrap()).unwrap();
            assert_eq!(key.as_str(), case["normalized"].as_str().unwrap());
            assert_eq!(LoginKey::new(key.as_str()).unwrap(), key);
        }
    }
}
