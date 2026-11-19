use org_proto::msg::{MessagePriority, Meta};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{ContentHash, MailboxError};

/// ADR-0003 §3.4: version byte and seven u32be length-prefixed UTF-8 fields.
/// Object keys are sorted explicitly at every depth, independent of preserve_order.
pub fn content_hash(
    to_member_id: &str,
    kind: &str,
    priority: MessagePriority,
    subject: Option<&str>,
    reply_to: Option<&str>,
    body: &str,
    meta: &Meta,
) -> Result<ContentHash, MailboxError> {
    let priority = match priority {
        MessagePriority::Normal => "normal",
        MessagePriority::High => "high",
        MessagePriority::Urgent => "urgent",
        _ => return Err(MailboxError::InvalidRequest),
    };
    let mut canonical_meta = Vec::new();
    if !meta.is_empty() {
        canonical_object(
            meta.iter().map(|(key, value)| (key.as_str(), value)),
            &mut canonical_meta,
        )?;
    }
    let mut hasher = Sha256::new();
    hasher.update([1]);
    for field in [
        to_member_id.as_bytes(),
        kind.as_bytes(),
        priority.as_bytes(),
        subject.unwrap_or_default().as_bytes(),
        reply_to.unwrap_or_default().as_bytes(),
        body.as_bytes(),
        &canonical_meta,
    ] {
        let size = u32::try_from(field.len()).map_err(|_| MailboxError::TooLarge)?;
        hasher.update(size.to_be_bytes());
        hasher.update(field);
    }
    Ok(hasher.finalize().into())
}

fn canonical_object<'a>(
    entries: impl Iterator<Item = (&'a str, &'a Value)>,
    output: &mut Vec<u8>,
) -> Result<(), MailboxError> {
    let mut entries: Vec<_> = entries.collect();
    entries.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    output.push(b'{');
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index != 0 {
            output.push(b',');
        }
        serde_json::to_writer(&mut *output, key).map_err(|_| MailboxError::InvalidRequest)?;
        output.push(b':');
        canonical_value(value, output)?;
    }
    output.push(b'}');
    Ok(())
}

fn canonical_value(value: &Value, output: &mut Vec<u8>) -> Result<(), MailboxError> {
    match value {
        Value::Object(entries) => canonical_object(
            entries.iter().map(|(key, value)| (key.as_str(), value)),
            output,
        ),
        Value::Array(entries) => {
            output.push(b'[');
            for (index, entry) in entries.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                canonical_value(entry, output)?;
            }
            output.push(b']');
            Ok(())
        }
        _ => serde_json::to_writer(output, value).map_err(|_| MailboxError::InvalidRequest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn adr_pinned_digest() {
        let meta = serde_json::from_value(json!({"z":1,"a":{"d":true,"b":"x"}})).unwrap();
        let hash = content_hash(
            "01M6WAGENT2",
            "request",
            MessagePriority::Urgent,
            Some("Check"),
            None,
            "ping",
            &meta,
        )
        .unwrap();
        let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            hex,
            "6c981ce347b8a49c9c766af0a1bdf36373abc26d2e77378a04ed5d00feed18ba"
        );
    }

    /// A second vector, computed independently (Python hashlib over the §3.4 byte stream):
    /// an empty subject, a reply_to, non-ASCII and a newline in the body, nested meta.
    #[test]
    fn independent_digest_with_reply_and_nested_meta() {
        let meta = serde_json::from_value(json!({"k": [1, "two", {"b": null, "a": 1.5}]})).unwrap();
        let hash = content_hash(
            "01JCMPAT000000000000000000",
            "msg",
            MessagePriority::Normal,
            None,
            Some("01JCMSG0000000000000000001"),
            "h\u{e9}llo\nworld",
            &meta,
        )
        .unwrap();
        let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            hex,
            "1608e0565043d65161f45e080a189102e1ecb1f970d398b6f91a6e8bfacbb20d"
        );
        // The empty subject and an absent one hash alike (§3.4: subject_or_empty).
        assert_eq!(
            hash,
            content_hash(
                "01JCMPAT000000000000000000",
                "msg",
                MessagePriority::Normal,
                Some(""),
                Some("01JCMSG0000000000000000001"),
                "h\u{e9}llo\nworld",
                &meta,
            )
            .unwrap()
        );
    }

    #[test]
    fn recursively_sorts_arrays_objects_and_escaped_keys() {
        let mut output = Vec::new();
        canonical_value(&json!({"z":[{"é":null,"a\n":2}],"a":false}), &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "{\"a\":false,\"z\":[{\"a\\n\":2,\"é\":null}]}"
        );
    }

    #[test]
    fn field_boundaries_cannot_be_ambiguous() {
        let hash = |to, kind| {
            content_hash(
                to,
                kind,
                MessagePriority::Normal,
                None,
                None,
                "",
                &Meta::new(),
            )
            .unwrap()
        };
        assert_ne!(hash("ab", "c"), hash("a", "bc"));
        assert_ne!(
            hash("a", "msg"),
            content_hash(
                "a",
                "msg",
                MessagePriority::High,
                None,
                None,
                "",
                &Meta::new()
            )
            .unwrap()
        );
    }
}
