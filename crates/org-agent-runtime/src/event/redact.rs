//! The redaction walkers [`crate::SessionEvent::redact`] and its parts share.

use serde_json::Value;

pub(crate) fn redact_opt(s: &mut Option<String>, f: &mut dyn FnMut(&mut String)) {
    if let Some(s) = s {
        f(s);
    }
}

/// Redacts every string in a JSON value, object keys included.
pub(super) fn redact_value(value: &mut Value, f: &mut dyn FnMut(&mut String)) {
    match value {
        Value::String(s) => f(s),
        Value::Array(items) => items.iter_mut().for_each(|v| redact_value(v, f)),
        Value::Object(map) => {
            for (mut key, mut item) in std::mem::take(map) {
                f(&mut key);
                redact_value(&mut item, f);
                map.insert(key, item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
