//! A small JSON Schema checker, for the features' input and output.
//!
//! It supports only what the registry needs: `type`, `enum`, `description` (ignored), and per type
//! `properties` / `required` / `additionalProperties`, `items` / `minItems` / `maxItems`,
//! `minLength` / `maxLength`, `minimum` / `maximum`. One twist: a `minLength` of 1 or more also
//! refuses a string that is only whitespace, because a draft that looks empty is empty. A schema that uses anything else, or that
//! leaves an object open (no `additionalProperties: false`), is refused when it is compiled.
//! A violation names the path and the rule, never the value: values can be a person's text.

use std::fmt;

use serde_json::{Map, Value};

/// A compiled (checked) schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Schema(Value);

/// Why a schema document is not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    /// Where in the schema document, e.g. `/properties/title`.
    pub path: String,
    pub reason: &'static str,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "schema at '{}': {}", self.path, self.reason)
    }
}

impl std::error::Error for SchemaError {}

/// One way a value breaks a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Where in the value, e.g. `/steps/2`. Empty is the value itself.
    pub path: String,
    /// The rule broken: `type`, `required`, `maxLength`, `additionalProperties`, ...
    pub rule: &'static str,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' breaks {}", self.path, self.rule)
    }
}

const MAX_SCHEMA_DEPTH: usize = 16;

impl Schema {
    /// Check a schema document and keep it.
    pub fn compile(document: Value) -> Result<Schema, SchemaError> {
        check_node(&document, "", 0)?;
        Ok(Schema(document))
    }

    /// The schema document, to put in a prompt or hand to a runtime.
    pub fn document(&self) -> &Value {
        &self.0
    }

    /// Every violation found in `value` (empty when it conforms).
    pub fn violations(&self, value: &Value) -> Vec<Violation> {
        let mut found = Vec::new();
        visit(&self.0, value, &mut String::new(), &mut found);
        found
    }

    /// `Ok` when `value` conforms.
    pub fn validate(&self, value: &Value) -> Result<(), Vec<Violation>> {
        let found = self.violations(value);
        if found.is_empty() {
            Ok(())
        } else {
            Err(found)
        }
    }
}

fn bad(path: &str, reason: &'static str) -> SchemaError {
    SchemaError {
        path: path.to_owned(),
        reason,
    }
}

fn child(path: &str, key: &str) -> String {
    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn allowed_keywords(ty: &str) -> &'static [&'static str] {
    match ty {
        "object" => &[
            "type",
            "enum",
            "description",
            "properties",
            "required",
            "additionalProperties",
        ],
        "array" => &[
            "type",
            "enum",
            "description",
            "items",
            "minItems",
            "maxItems",
        ],
        "string" => &["type", "enum", "description", "minLength", "maxLength"],
        "number" | "integer" => &["type", "enum", "description", "minimum", "maximum"],
        _ => &["type", "enum", "description"],
    }
}

fn check_node(node: &Value, path: &str, depth: usize) -> Result<(), SchemaError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(bad(path, "nested too deep"));
    }
    let Some(map) = node.as_object() else {
        return Err(bad(path, "a schema must be an object"));
    };
    let ty = match map.get("type") {
        Some(Value::String(t))
            if matches!(
                t.as_str(),
                "object" | "array" | "string" | "number" | "integer" | "boolean" | "null"
            ) =>
        {
            t.as_str()
        }
        _ => return Err(bad(path, "'type' must be one known type name")),
    };
    let allowed = allowed_keywords(ty);
    if let Some(key) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(bad(&child(path, key), "keyword not supported here"));
    }
    if let Some(e) = map.get("enum") {
        if !e.as_array().is_some_and(|a| !a.is_empty()) {
            return Err(bad(&child(path, "enum"), "must be a non-empty array"));
        }
    }
    for key in ["minLength", "maxLength", "minItems", "maxItems"] {
        if map.get(key).is_some_and(|v| v.as_u64().is_none()) {
            return Err(bad(&child(path, key), "must be a non-negative integer"));
        }
    }
    for key in ["minimum", "maximum"] {
        if map.get(key).is_some_and(|v| !v.is_number()) {
            return Err(bad(&child(path, key), "must be a number"));
        }
    }
    match ty {
        "object" => check_object(map, path, depth),
        "array" => match map.get("items") {
            Some(items) => check_node(items, &child(path, "items"), depth + 1),
            None => Err(bad(path, "an array schema needs 'items'")),
        },
        _ => Ok(()),
    }
}

fn check_object(map: &Map<String, Value>, path: &str, depth: usize) -> Result<(), SchemaError> {
    if map.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Err(bad(
            path,
            "an object schema needs 'additionalProperties': false",
        ));
    }
    let empty = Map::new();
    let properties = match map.get("properties") {
        None => &empty,
        Some(Value::Object(p)) => p,
        Some(_) => return Err(bad(&child(path, "properties"), "must be an object")),
    };
    for (name, schema) in properties {
        check_node(schema, &child(&child(path, "properties"), name), depth + 1)?;
    }
    if let Some(required) = map.get("required") {
        let Some(list) = required.as_array() else {
            return Err(bad(&child(path, "required"), "must be an array of names"));
        };
        for name in list {
            if !name.as_str().is_some_and(|n| properties.contains_key(n)) {
                return Err(bad(
                    &child(path, "required"),
                    "names a property that isn't declared",
                ));
            }
        }
    }
    Ok(())
}

fn add(found: &mut Vec<Violation>, path: &str, rule: &'static str) {
    found.push(Violation {
        path: path.to_owned(),
        rule,
    });
}

fn visit(schema: &Value, value: &Value, path: &mut String, found: &mut Vec<Violation>) {
    // The schema was checked by `compile`, so the lookups below can't meet an unexpected shape.
    let ty = schema["type"].as_str().unwrap_or_default();
    let type_ok = match ty {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    if !type_ok {
        return add(found, path, "type");
    }
    if let Some(options) = schema["enum"].as_array() {
        if !options.contains(value) {
            add(found, path, "enum");
        }
    }
    match value {
        Value::String(s) => {
            let chars = s.chars().count() as u64;
            let blank = s.trim().is_empty();
            if schema["minLength"]
                .as_u64()
                .is_some_and(|m| chars < m || (m > 0 && blank))
            {
                add(found, path, "minLength");
            }
            if schema["maxLength"].as_u64().is_some_and(|m| chars > m) {
                add(found, path, "maxLength");
            }
        }
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if schema["minimum"].as_f64().is_some_and(|m| x < m) {
                add(found, path, "minimum");
            }
            if schema["maximum"].as_f64().is_some_and(|m| x > m) {
                add(found, path, "maximum");
            }
        }
        Value::Array(items) => {
            let count = items.len() as u64;
            if schema["minItems"].as_u64().is_some_and(|m| count < m) {
                add(found, path, "minItems");
            }
            if schema["maxItems"].as_u64().is_some_and(|m| count > m) {
                add(found, path, "maxItems");
            }
            for (i, item) in items.iter().enumerate() {
                let keep = path.len();
                path.push_str(&format!("/{i}"));
                visit(&schema["items"], item, path, found);
                path.truncate(keep);
            }
        }
        Value::Object(map) => {
            let properties = schema["properties"].as_object();
            if let Some(required) = schema["required"].as_array() {
                for name in required.iter().filter_map(Value::as_str) {
                    if !map.contains_key(name) {
                        add(found, &child(path, name), "required");
                    }
                }
            }
            for (name, item) in map {
                let keep = path.len();
                *path = child(path, name);
                match properties.and_then(|p| p.get(name)) {
                    Some(sub) => visit(sub, item, path, found),
                    None => add(found, path, "additionalProperties"),
                }
                path.truncate(keep);
            }
        }
        _ => {}
    }
}
