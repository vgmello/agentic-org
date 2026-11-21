//! The byte form of the organization family's files: one `key: <JSON value>` line per
//! field, keys in byte order. A JSON value is a YAML flow value, so the files are YAML and
//! are read back through `org-docs`' strict parser. A profile is that mapping between
//! `---` fences, followed by its Markdown body.

use std::fmt;

use serde_json::{Map, Value};

/// The fields of one mapping. [`encode`] writes them in byte order of key.
pub type Fields = Map<String, Value>;

/// Why bytes are not the record they should be: the field, as a path, and the reason.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RecordError {
    pub field: String,
    pub reason: String,
}

impl RecordError {
    pub fn new(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            return f.write_str(&self.reason);
        }
        write!(f, "{}: {}", self.field, self.reason)
    }
}

impl std::error::Error for RecordError {}

/// `value` as compact JSON with the keys of every object in byte order, whatever order the
/// map itself keeps: a file's bytes do not depend on how `serde_json` was built.
fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Object(fields) => {
            out.push('{');
            for (i, (key, value)) in sorted(fields).into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push(':');
                write_value(out, value);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        // A scalar always serializes: its numbers are finite.
        scalar => out.push_str(&scalar.to_string()),
    }
}

fn sorted(fields: &Fields) -> Vec<(&String, &Value)> {
    let mut entries: Vec<_> = fields.iter().collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    entries
}

pub fn encode(fields: &Fields) -> Vec<u8> {
    let mut out = String::new();
    for (key, value) in sorted(fields) {
        out.push_str(key);
        out.push_str(": ");
        write_value(&mut out, value);
        out.push('\n');
    }
    out.into_bytes()
}

pub fn encode_document(fields: &Fields, body: &str) -> Vec<u8> {
    let mut out = b"---\n".to_vec();
    out.extend(encode(fields));
    out.extend(b"---\n");
    out.extend(body.as_bytes());
    out
}

fn text(bytes: &[u8]) -> Result<&str, RecordError> {
    std::str::from_utf8(bytes).map_err(|_| RecordError::new("", "not UTF-8"))
}

fn refused(error: org_docs::FrontMatterError) -> RecordError {
    RecordError::new(error.field, format!("{:?}", error.reason))
}

pub fn decode(bytes: &[u8]) -> Result<Reader, RecordError> {
    let fields = org_docs::parse_mapping(text(bytes)?).map_err(refused)?;
    Ok(Reader::new(fields, String::new()))
}

/// The front matter and the body of a profile.
pub fn decode_document(bytes: &[u8]) -> Result<(Reader, String), RecordError> {
    let document = org_docs::parse(text(bytes)?).map_err(refused)?;
    let fields = document
        .front_matter
        .ok_or_else(|| RecordError::new("", "no front matter"))?;
    Ok((Reader::new(fields, String::new()), document.body.to_owned()))
}

/// Takes the fields of one mapping, each exactly once. `finish` refuses what is left, so
/// a record has no unknown field and no missing one.
pub struct Reader {
    fields: Fields,
    at: String,
}

impl Reader {
    fn new(fields: Fields, at: String) -> Self {
        Self { fields, at }
    }

    /// The path of `key` from the root of the file.
    pub fn path(&self, key: &str) -> String {
        format!("{}{key}", self.at)
    }

    pub fn error(&self, key: &str, reason: impl Into<String>) -> RecordError {
        RecordError::new(self.path(key), reason)
    }

    fn take(&mut self, key: &str) -> Result<Value, RecordError> {
        let value = self.fields.remove(key);
        value.ok_or_else(|| self.error(key, "missing"))
    }

    pub fn text(&mut self, key: &str) -> Result<String, RecordError> {
        match self.take(key)? {
            Value::String(text) => Ok(text),
            _ => Err(self.error(key, "not a string")),
        }
    }

    pub fn optional_text(&mut self, key: &str) -> Result<Option<String>, RecordError> {
        match self.take(key)? {
            Value::Null => Ok(None),
            Value::String(text) => Ok(Some(text)),
            _ => Err(self.error(key, "not a string or null")),
        }
    }

    pub fn number(&mut self, key: &str) -> Result<u64, RecordError> {
        let value = self.take(key)?;
        let number = value.as_u64().filter(|n| *n <= i64::MAX as u64);
        number.ok_or_else(|| self.error(key, "not a whole number"))
    }

    pub fn flag(&mut self, key: &str) -> Result<bool, RecordError> {
        let value = self.take(key)?;
        value
            .as_bool()
            .ok_or_else(|| self.error(key, "not true or false"))
    }

    /// A list of mappings, each read like a file of its own.
    pub fn list(&mut self, key: &str) -> Result<Vec<Reader>, RecordError> {
        let Value::Array(items) = self.take(key)? else {
            return Err(self.error(key, "not a list"));
        };
        let readers = items.into_iter().enumerate().map(|(i, item)| match item {
            Value::Object(fields) => Ok(Reader::new(fields, format!("{}[{i}].", self.path(key)))),
            _ => Err(RecordError::new(
                format!("{}[{i}]", self.path(key)),
                "not a mapping",
            )),
        });
        readers.collect()
    }

    /// A list of strings.
    pub fn strings(&mut self, key: &str) -> Result<Vec<String>, RecordError> {
        let Value::Array(items) = self.take(key)? else {
            return Err(self.error(key, "not a list"));
        };
        items
            .into_iter()
            .enumerate()
            .map(|(i, item)| match item {
                Value::String(text) => Ok(text),
                _ => Err(RecordError::new(
                    format!("{}[{i}]", self.path(key)),
                    "not a string",
                )),
            })
            .collect()
    }

    /// Whether the file has `key` and nothing took it yet: for a field a file may omit.
    pub fn has(&self, key: &str) -> bool {
        self.fields.contains_key(key)
    }

    pub fn finish(self) -> Result<(), RecordError> {
        match self.fields.keys().min() {
            Some(key) => Err(self.error(key, "unknown field")),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(value: Value) -> Fields {
        value.as_object().expect("an object").clone()
    }

    #[test]
    fn a_mapping_is_one_line_per_field_in_byte_order_of_key() {
        let bytes = encode(&fields(
            json!({"name":"A","id":"x","revision":1,"lead":null,"on":true}),
        ));
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "id: \"x\"\nlead: null\nname: \"A\"\non: true\nrevision: 1\n"
        );
    }

    #[test]
    fn the_order_fields_were_inserted_in_never_reaches_the_bytes() {
        // Inserted in reverse byte order at both levels; holds with `preserve_order` on too.
        let mut inner = Fields::new();
        for key in ["z", "m", "a"] {
            inner.insert(key.into(), json!(key));
        }
        let mut outer = Fields::new();
        outer.insert("list".into(), json!([Value::Object(inner.clone())]));
        outer.insert("b".into(), Value::Object(inner));
        outer.insert("a".into(), json!("quote \" here"));
        assert_eq!(
            String::from_utf8(encode(&outer)).unwrap(),
            concat!(
                "a: \"quote \\\" here\"\n",
                "b: {\"a\":\"a\",\"m\":\"m\",\"z\":\"z\"}\n",
                "list: [{\"a\":\"a\",\"m\":\"m\",\"z\":\"z\"}]\n",
            )
        );
        // The same bytes `serde_json` writes for a sorted map.
        let sorted: std::collections::BTreeMap<_, _> = outer.iter().collect();
        let mut plain = String::new();
        write_value(&mut plain, &Value::Object(outer.clone()));
        assert_eq!(plain, serde_json::to_string(&sorted).unwrap());
    }

    #[test]
    fn every_text_reads_back_as_it_was_written() {
        for text in [
            "plain",
            "",
            " edge spaces ",
            "a: b # not a comment",
            "quote \" and \\ backslash",
            "line\nbreak\tand tab",
            "---",
            "null",
            "true",
            "007",
            "Zoë 組織 🙂",
            "{not: a map}",
            "- not a list",
            "'single'",
            "\u{1}\u{7f}\u{85}\u{2028}",
        ] {
            let written = fields(json!({"name": text, "list": [{"inner": text}]}));
            let mut reader = decode(&encode(&written)).unwrap();
            assert_eq!(reader.text("name").unwrap(), text, "{text:?}");
            let mut items = reader.list("list").unwrap();
            assert_eq!(items[0].text("inner").unwrap(), text, "{text:?}");
            items.remove(0).finish().unwrap();
            reader.finish().unwrap();

            let (mut reader, body) = decode_document(&encode_document(&written, text)).unwrap();
            assert_eq!(reader.text("name").unwrap(), text, "{text:?}");
            assert_eq!(body, text, "{text:?}");
        }
    }

    #[test]
    fn a_missing_an_unknown_and_a_mistyped_field_are_refused_by_path() {
        let written = fields(json!({"id":"x","n":-1,"list":[{"a":1,"b":2}, 3]}));
        let mut reader = decode(&encode(&written)).unwrap();
        assert_eq!(
            reader.text("name"),
            Err(RecordError::new("name", "missing"))
        );
        assert_eq!(
            reader.number("id"),
            Err(RecordError::new("id", "not a whole number"))
        );
        assert_eq!(
            reader.number("n"),
            Err(RecordError::new("n", "not a whole number"))
        );
        assert_eq!(
            reader.list("list").err(),
            Some(RecordError::new("list[1]", "not a mapping"))
        );

        let written = fields(json!({"id":"x","list":[{"a":1,"b":2}]}));
        let mut reader = decode(&encode(&written)).unwrap();
        let mut item = reader.list("list").unwrap().remove(0);
        assert_eq!(item.number("a"), Ok(1));
        assert_eq!(
            item.finish(),
            Err(RecordError::new("list[0].b", "unknown field"))
        );
        assert_eq!(
            reader.finish(),
            Err(RecordError::new("id", "unknown field"))
        );
    }

    #[test]
    fn bytes_that_are_not_one_strict_mapping_are_refused() {
        for bad in [
            &b"\xff"[..],
            b"- a\n- b\n",
            b"a: 1\na: 2\n",
            b"a: &x 1\n",
            b"a: !!str 1\n",
        ] {
            assert!(decode(bad).is_err(), "{bad:?}");
        }
        assert!(decode_document(b"name: \"x\"\n").is_err());
        assert!(decode_document(b"---\nname: \"x\"\n").is_err());
    }
}
