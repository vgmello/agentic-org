//! Validate every JSON object before Request's value-based params can discard duplicate keys.
use std::collections::HashSet;
use std::fmt;

use org_proto::Request;
use serde::de::{self, DeserializeOwned, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

pub(super) fn decode(line: &[u8]) -> serde_json::Result<Request> {
    serde_json::from_slice::<de::IgnoredAny>(line)?;
    unique_keys(line)?;
    object::<Request>(line)
}

pub(super) fn salvage_id(line: &[u8]) -> Option<u64> {
    #[derive(Deserialize)]
    struct Id {
        id: u64,
    }
    // Derived struct decoding refuses a second id, including an escaped spelling.
    object::<Id>(line).ok().map(|request| request.id)
}

// Derived structs also accept sequences; framing and ID recovery require a whole object.
fn object<'de, T: Deserialize<'de>>(line: &'de [u8]) -> serde_json::Result<T> {
    struct Object<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Object<T> {
        type Value = T;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a JSON request object")
        }
        fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<T, M::Error> {
            T::deserialize(de::value::MapAccessDeserializer::new(map))
        }
    }
    let mut deserializer = serde_json::Deserializer::from_slice(line);
    let value = deserializer.deserialize_map(Object(std::marker::PhantomData))?;
    deserializer.end()?;
    Ok(value)
}

enum Container {
    Object {
        keys: HashSet<String>,
        wants_key: bool,
    },
    Array,
}

// IgnoredAny has validated the whole grammar without converting numbers. Walk structure
// iteratively so ignored values retain their numeric range and nesting compatibility.
fn unique_keys(bytes: &[u8]) -> serde_json::Result<()> {
    let mut containers = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        match bytes[offset] {
            b'{' => containers.push(Container::Object {
                keys: HashSet::new(),
                wants_key: true,
            }),
            b'[' => containers.push(Container::Array),
            b'}' | b']' => {
                containers.pop();
            }
            b',' => {
                if let Some(Container::Object { wants_key, .. }) = containers.last_mut() {
                    *wants_key = true;
                }
            }
            b'"' => {
                match containers.last_mut() {
                    Some(Container::Object { keys, wants_key }) if *wants_key => {
                        let (key, consumed) = string_token::<String>(&bytes[offset..])?;
                        if !keys.insert(key) {
                            return Err(de::Error::custom("duplicate object key"));
                        }
                        *wants_key = false;
                        offset += consumed;
                    }
                    _ => {
                        let (_, consumed) = string_token::<de::IgnoredAny>(&bytes[offset..])?;
                        offset += consumed;
                    }
                }
                continue;
            }
            _ => {}
        }
        offset += 1;
    }
    Ok(())
}

// Let serde_json decode/skip a self-delimited string, including all escaping rules.
fn string_token<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<(T, usize)> {
    let mut stream = serde_json::Deserializer::from_slice(bytes).into_iter::<T>();
    let value = stream
        .next()
        .ok_or_else(|| <serde_json::Error as de::Error>::custom("missing JSON string"))??;
    Ok((value, stream.byte_offset()))
}

#[cfg(test)]
mod tests {
    use super::{decode, salvage_id};

    #[test]
    fn ignored_envelope_numbers_keep_the_existing_request_contract() {
        for value in [
            "1e1000",
            "-1e1000",
            "1e-1000",
            "1234567890123456789012345678901234567890",
            "[1e1000]",
            r#"{"x":1e1000}"#,
        ] {
            let line = format!(r#"{{"id":7,"method":"hello","ignored":{value}}}"#);
            assert!(serde_json::from_str::<org_proto::Request>(&line).is_ok());
            assert!(decode(line.as_bytes()).is_ok(), "{line}");
        }
    }

    #[test]
    fn ignored_values_keep_deep_nesting_and_string_contents() {
        let nested = format!("{}0{}", r#"{"x":"#.repeat(512), "}".repeat(512));
        let line = format!(r#"{{"id":1,"method":"hello","ignored":{nested}}}"#);
        assert!(serde_json::from_str::<org_proto::Request>(&line).is_ok());
        assert!(decode(line.as_bytes()).is_ok());
        let duplicate = format!(
            "{}{}{}",
            r#"{"x":"#.repeat(512),
            r#"{"a":1,"a":2}"#,
            "}".repeat(512)
        );
        let line = format!(r#"{{"id":1,"method":"hello","ignored":{duplicate}}}"#);
        assert!(decode(line.as_bytes()).is_err());
        let line = serde_json::json!({"id":1,"method":"hello","ignored":{"x":"x", "items":["items", "x", "\"\\,[{}]:"]}}).to_string();
        assert!(decode(line.as_bytes()).is_ok());
        assert!(decode(br#"{"id":1,"method":"hello","params":{"future":1e1000}}"#).is_err());
    }

    #[test]
    fn key_scopes_survive_arrays_and_large_ignored_numbers() {
        for value in [
            r#"{"a":[],"b":[],"x":1,"x":2}"#,
            r#"{"large":1e1000,"x":1,"\u0078":2}"#,
            r#"{"":1,"":2}"#,
            r#"[{"x":1,"x":2}]"#,
        ] {
            let line = format!(r#"{{"id":1,"method":"hello","ignored":{value}}}"#);
            assert!(decode(line.as_bytes()).is_err(), "{line}");
        }
    }

    #[test]
    fn depth_bounds_match_the_existing_decoder_for_each_field_category() {
        for (field, depth, accepted) in [
            ("ignored", 127, true),
            ("ignored", 129, true),
            ("params", 127, true),
            ("params", 129, false),
        ] {
            let value = format!("{}null{}", "[".repeat(depth - 1), "]".repeat(depth - 1));
            let line = format!(r#"{{"id":1,"method":"hello","{field}":{value}}}"#);
            assert_eq!(
                serde_json::from_str::<org_proto::Request>(&line).is_ok(),
                accepted,
                "base {field}/{depth}"
            );
            assert_eq!(
                decode(line.as_bytes()).is_ok(),
                accepted,
                "host {field}/{depth}"
            );
        }
    }

    #[test]
    fn nonobject_frames_have_no_trustworthy_request_id() {
        for line in [
            r#"[7]"#,
            r#"[7,"hello"]"#,
            r#"[7,"hello",null,{}]"#,
            "null",
            "7",
            "true",
            r#""hello""#,
            r#"{"id":7,"method":"hello"} {"id":8}"#,
        ] {
            assert!(decode(line.as_bytes()).is_err(), "{line}");
            assert_eq!(salvage_id(line.as_bytes()), None, "{line}");
        }
    }

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
