//! Validate every JSON object before Request's value-based params can discard duplicate keys.
use std::collections::HashSet;
use std::fmt;

use org_proto::Request;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

pub(super) fn decode(line: &[u8]) -> serde_json::Result<Request> {
    serde_json::from_slice::<UniqueKeys>(line)?;
    serde_json::from_slice(line)
}

pub(super) fn salvage_id(line: &[u8]) -> Option<u64> {
    #[derive(Deserialize)]
    struct Id {
        id: u64,
    }
    // Derived struct decoding refuses a second id, including an escaped spelling.
    serde_json::from_slice::<Id>(line)
        .ok()
        .map(|request| request.id)
}

struct UniqueKeys;

impl<'de> Deserialize<'de> for UniqueKeys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Self)
    }
}

impl<'de> Visitor<'de> for UniqueKeys {
    type Value = Self;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self, M::Error> {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            // String decoding has already resolved JSON escapes. Never echo a key or value.
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate object key"));
            }
            map.next_value::<Self>()?;
        }
        Ok(Self)
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self, S::Error> {
        while seq.next_element::<Self>()?.is_some() {}
        Ok(Self)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self, E> {
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, salvage_id};

    #[test]
    fn only_one_unambiguous_top_level_id_can_be_salvaged() {
        for line in [
            r#"{"id":1,"id":2,"method":"hello"}"#,
            r#"{"id":1,"\u0069d":1,"method":"hello"}"#,
            r#"{"id":null,"id":2,"method":"hello"}"#,
        ] {
            assert_eq!(salvage_id(line.as_bytes()), None, "{line}");
        }
        for line in [
            r#"{"id":7,"method":5}"#,
            r#"{"id":7,"method":"hello","params":{"id":1,"id":2}}"#,
            r#"{"id":7,"method":"hello","ignored":1,"ignored":2}"#,
        ] {
            assert_eq!(salvage_id(line.as_bytes()), Some(7), "{line}");
        }
    }

    #[test]
    fn duplicate_keys_are_checked_even_in_ignored_fields_and_array_elements() {
        for line in [
            r#"{"id":1,"id":2,"method":"hello"}"#,
            r#"{"id":1,"method":"hello","method":"hello"}"#,
            r#"{"id":1,"method":"hello","\u0069d":1}"#,
            r#"{"id":1,"method":"hello","ignored":{"x":1,"x":2}}"#,
            r#"{"id":1,"method":"hello","params":{"nested":[{"x":1,"\u0078":2}]}}"#,
        ] {
            assert!(decode(line.as_bytes()).is_err(), "{line}");
        }
    }

    #[test]
    fn each_json_value_and_distinct_names_survive_validation() {
        for value in [
            "null",
            "true",
            "false",
            "0",
            "-1",
            "18446744073709551615",
            "1.5",
            "1e3",
            r#""hello""#,
            "[]",
            "{}",
            r#"[{"x":1},{"x":2}]"#,
            r#"{"x":1,"X":2,"\u00e9":3,"e\u0301":4}"#,
        ] {
            let line = format!(r#"{{"id":1,"method":"future.method","params":{value}}}"#);
            assert!(decode(line.as_bytes()).is_ok(), "{line}");
        }
    }
}
