//! Validate raw bounded business JSON without constructing a value that loses keys.
//! Schema and authorization remain with their owners; credentials use their own ingress.

use std::collections::HashSet;
use std::fmt;

use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;

const MAX_DEPTH: u8 = 64;

pub(crate) fn validate(bytes: &[u8]) -> Result<(), ()> {
    // Bodyless reads are valid; each route owns its required-body/schema rules.
    if bytes.is_empty() {
        return Ok(());
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    Check { depth: 0 }
        .deserialize(&mut decoder)
        .map_err(|_| ())?;
    decoder.end().map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_json_is_consumed_without_confusing_values_or_sibling_keys() {
        for valid in [
            "",
            "{}",
            "null",
            "true",
            "false",
            "-1",
            "0",
            "18446744073709551615",
            "1.5",
            r#""a\"a""#,
            r#"{"a":"a","child":{"a":1},"siblings":[{"a":2},{"a":3}]}"#,
            r#"{"é":1,"e\u0301":2}"#,
        ] {
            assert!(validate(valid.as_bytes()).is_ok(), "{valid}");
        }
    }

    #[test]
    fn malformed_duplicate_unescaped_and_trailing_json_is_rejected() {
        for invalid in [
            " ",
            "{broken}",
            "{}{}",
            "{} true",
            "[1,]",
            "NaN",
            "1e999",
            r#"{"a":1,"a":1}"#,
            r#"{"a":1,"\u0061":2}"#,
            r#"[{"deep":[{"a":1,"a":2}]}]"#,
        ] {
            assert!(validate(invalid.as_bytes()).is_err(), "{invalid}");
        }
    }

    #[test]
    fn both_object_and_array_nesting_count_containers_up_to_64() {
        for (open, close) in [("[", "]"), ("{\"a\":", "}")] {
            for depth in [1, 63, 64, 65, 100] {
                let bytes = format!("{}0{}", open.repeat(depth), close.repeat(depth));
                assert_eq!(validate(bytes.as_bytes()).is_ok(), depth <= 64, "{depth}");
            }
        }
        let mixed = format!("{{\"a\":{}0{}}}", "[".repeat(64), "]".repeat(64));
        assert!(
            validate(mixed.as_bytes()).is_err(),
            "root object also counts"
        );
    }
}

struct Check {
    depth: u8,
}

impl<'de> DeserializeSeed<'de> for Check {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        decoder.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Check {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("valid JSON without duplicate keys")
    }
    fn visit_bool<E: Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        if self.depth >= MAX_DEPTH {
            return Err(M::Error::custom("JSON nesting limit exceeded"));
        }
        let mut names = HashSet::new();
        while let Some(name) = map.next_key::<String>()? {
            // Serde unescapes keys before comparison. Keep sets per object,
            // allowing the same key in independent siblings.
            if !names.insert(name) {
                return Err(M::Error::custom("duplicate JSON key"));
            }
            map.next_value_seed(Check {
                depth: self.depth + 1,
            })?;
        }
        Ok(())
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<(), S::Error> {
        if self.depth >= MAX_DEPTH {
            return Err(S::Error::custom("JSON nesting limit exceeded"));
        }
        while let Some(()) = sequence.next_element_seed(Check {
            depth: self.depth + 1,
        })? {}
        Ok(())
    }
}
