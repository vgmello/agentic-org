//! The params hash of the change-id table (§2.3): SHA-256 over the method name, the bound
//! member id and the decoded, re-encoded typed params as canonical JSON (object keys sorted, no
//! insignificant whitespace). The three parts are joined with a NUL byte, which none of them
//! can contain unescaped.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The params hash of one call.
pub fn params_hash(method: &str, member_id: &str, params: &Value) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(method.as_bytes());
    hasher.update([0]);
    hasher.update(member_id.as_bytes());
    hasher.update([0]);
    hasher.update(canonical_json(params).as_bytes());
    hasher.finalize().into()
}

/// `value` as canonical JSON: object keys sorted by their bytes, no whitespace. Independent of
/// how `serde_json`'s map is ordered in this build (`preserve_order` may be on elsewhere).
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push('{');
            for (i, (k, v)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                write(v, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(v, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_keys_at_every_depth() {
        let v = json!({"b": 1, "a": {"z": [1, {"y": "x\n", "c": null}], "a": true}});
        assert_eq!(
            canonical_json(&v),
            r#"{"a":{"a":true,"z":[1,{"c":null,"y":"x\n"}]},"b":1}"#
        );
    }

    #[test]
    fn hash_covers_method_member_and_params() {
        let p = json!({"a": 1});
        let h = params_hash("task.create", "m1", &p);
        assert_eq!(h, params_hash("task.create", "m1", &json!({"a": 1})));
        assert_ne!(h, params_hash("task.update", "m1", &p));
        assert_ne!(h, params_hash("task.create", "m2", &p));
        assert_ne!(h, params_hash("task.create", "m1", &json!({"a": 2})));
        // The separator keeps the parts apart.
        assert_ne!(params_hash("ab", "c", &p), params_hash("a", "bc", &p),);
    }

    #[test]
    fn sha256_matches_a_known_vector() {
        let digest: [u8; 32] = Sha256::digest(b"abc").into();
        assert_eq!(
            digest[..4],
            [0xba, 0x78, 0x16, 0xbf],
            "FIPS 180-2 test vector for \"abc\""
        );
    }
}
