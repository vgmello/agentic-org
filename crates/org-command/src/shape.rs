//! What the declared fields of a row are used for: checking a value against them, the JSON
//! Schema `--schema` prints, the order `data` is printed in, and `--fields`. One declaration
//! drives all four, so the schema and the parser cannot drift.

use serde_json::{Map, Value};

use crate::node::Node;
use crate::table::{Command, Field, Kind, Type};

pub(crate) const SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Checks an object against the fields: no unknown field, every required one, each of its type.
pub(crate) fn validate(fields: &[Field], object: &Map<String, Value>) -> Result<(), String> {
    validate_at(fields, object, "")
}

/// Checks a command's `data`: one record, or for a list an array of records.
pub(crate) fn validate_data(command: &Command, data: &Value) -> Result<(), String> {
    match (command.kind, data) {
        (Kind::List, Value::Array(items)) => {
            items
                .iter()
                .enumerate()
                .try_for_each(|(i, item)| match item {
                    Value::Object(object) => {
                        validate_at(command.output, object, &format!("[{i}]."))
                    }
                    _ => Err(format!("item {i} is not an object")),
                })
        }
        (Kind::List, _) => Err("a list answers an array".into()),
        (_, Value::Object(object)) => validate(command.output, object),
        _ => Err("the answer is not an object".into()),
    }
}

fn validate_at(fields: &[Field], object: &Map<String, Value>, at: &str) -> Result<(), String> {
    for key in object.keys() {
        if !fields.iter().any(|field| field.name == key) {
            return Err(format!("`{at}{key}` is not a field"));
        }
    }
    for field in fields {
        match object.get(field.name) {
            Some(value) => validate_type(&field.ty, value, &format!("{at}{}", field.name))?,
            None if field.required => return Err(format!("`{at}{}` is required", field.name)),
            None => {}
        }
    }
    Ok(())
}

fn validate_type(ty: &Type, value: &Value, at: &str) -> Result<(), String> {
    let wrong = |what: &str| Err(format!("`{at}` must be {what}"));
    match (ty, value) {
        (Type::Json, _) | (Type::String, Value::String(_)) | (Type::Boolean, Value::Bool(_)) => {
            Ok(())
        }
        (Type::String, _) => wrong("text"),
        (Type::Boolean, _) => wrong("true or false"),
        (Type::Integer, Value::Number(n)) if n.is_i64() || n.is_u64() => Ok(()),
        (Type::Integer, _) => wrong("a whole number"),
        (Type::Enum(words), Value::String(s)) if words.contains(&s.as_str()) => Ok(()),
        (Type::Enum(words), _) => wrong(&format!("one of {}", words.join(", "))),
        (Type::List(inner), Value::Array(items)) => items
            .iter()
            .enumerate()
            .try_for_each(|(i, item)| validate_type(inner, item, &format!("{at}[{i}]"))),
        (Type::List(_), _) => wrong("a list"),
        (Type::Object(fields), Value::Object(object)) => {
            validate_at(fields, object, &format!("{at}."))
        }
        (Type::Object(_), _) => wrong("an object"),
    }
}

fn schema_of_type(ty: &Type) -> Vec<(String, Node)> {
    let ty_entry = |name: &str| ("type".to_owned(), Node::text(name));
    match ty {
        Type::String => vec![ty_entry("string")],
        Type::Integer => vec![ty_entry("integer")],
        Type::Boolean => vec![ty_entry("boolean")],
        Type::Enum(words) => vec![
            ty_entry("string"),
            (
                "enum".to_owned(),
                Node::List(words.iter().map(|word| Node::text(*word)).collect()),
            ),
        ],
        Type::List(inner) => vec![
            ty_entry("array"),
            ("items".to_owned(), Node::Map(schema_of_type(inner))),
        ],
        Type::Object(fields) => schema_of_fields(fields),
        Type::Json => vec![],
    }
}

fn schema_of_fields(fields: &[Field]) -> Vec<(String, Node)> {
    let properties = fields
        .iter()
        .map(|field| {
            let mut schema = schema_of_type(&field.ty);
            schema.push(("description".to_owned(), Node::text(field.help)));
            (field.name.to_owned(), Node::Map(schema))
        })
        .collect();
    let required = fields
        .iter()
        .filter(|field| field.required)
        .map(|field| Node::text(field.name))
        .collect();
    vec![
        ("type".to_owned(), Node::text("object")),
        ("properties".to_owned(), Node::Map(properties)),
        ("required".to_owned(), Node::List(required)),
        ("additionalProperties".to_owned(), Node::Bool(false)),
    ]
}

fn with_dialect(mut schema: Vec<(String, Node)>) -> Node {
    schema.insert(0, ("$schema".to_owned(), Node::text(SCHEMA_DIALECT)));
    Node::Map(schema)
}

/// The JSON Schema of a command's input object.
pub(crate) fn input_schema(command: &Command) -> Node {
    with_dialect(schema_of_fields(command.input))
}

/// The JSON Schema of a command's `data`.
pub(crate) fn output_schema(command: &Command) -> Node {
    let record = schema_of_fields(command.output);
    with_dialect(match command.kind {
        Kind::List => vec![
            ("type".to_owned(), Node::text("array")),
            ("items".to_owned(), Node::Map(record)),
        ],
        _ => record,
    })
}

/// A command's `data` in the order its row declares. A field the row does not declare follows
/// the declared ones, in name order.
pub(crate) fn order_data(command: &Command, data: &Value) -> Node {
    match (command.kind, data) {
        (Kind::List, Value::Array(items)) => Node::List(
            items
                .iter()
                .map(|item| order_fields(command.output, item))
                .collect(),
        ),
        _ => order_fields(command.output, data),
    }
}

fn order_fields(fields: &[Field], value: &Value) -> Node {
    let Value::Object(object) = value else {
        return Node::from_value(value);
    };
    let mut entries: Vec<(String, Node)> = fields
        .iter()
        .filter_map(|field| {
            Some((
                field.name.to_owned(),
                order_type(&field.ty, object.get(field.name)?),
            ))
        })
        .collect();
    for (key, value) in object {
        if !fields.iter().any(|field| field.name == key) {
            entries.push((key.clone(), Node::from_value(value)));
        }
    }
    Node::Map(entries)
}

fn order_type(ty: &Type, value: &Value) -> Node {
    match (ty, value) {
        (Type::Object(fields), Value::Object(_)) => order_fields(fields, value),
        (Type::List(inner), Value::Array(items)) => {
            Node::List(items.iter().map(|item| order_type(inner, item)).collect())
        }
        _ => Node::from_value(value),
    }
}

/// One `--fields` entry: the names from the record down.
pub(crate) type FieldPath = Vec<String>;

/// Reads `--fields a,b.c` and checks every name against the command's output.
pub(crate) fn field_paths(command: &Command, text: &str) -> Result<Vec<FieldPath>, String> {
    let mut paths = Vec::new();
    for entry in text.split(',') {
        let path: FieldPath = entry.split('.').map(str::to_owned).collect();
        if path.iter().any(String::is_empty) {
            return Err(format!("`{entry}` is not a field name"));
        }
        check_path(command.output, &path)
            .map_err(|name| format!("`{name}` in `{entry}` is not a field of the answer"))?;
        paths.push(path);
    }
    Ok(paths)
}

fn check_path<'p>(fields: &[Field], path: &'p [String]) -> Result<(), &'p str> {
    let Some((head, rest)) = path.split_first() else {
        return Ok(());
    };
    let field = fields
        .iter()
        .find(|field| field.name == head)
        .ok_or(head.as_str())?;
    let mut ty = &field.ty;
    while let Type::List(inner) = ty {
        ty = inner;
    }
    match ty {
        Type::Object(fields) => check_path(fields, rest),
        Type::Json => Ok(()),
        _ => rest.first().map_or(Ok(()), |name| Err(name.as_str())),
    }
}

/// Keeps only the chosen fields, for one record or for each item of a list. The record's own
/// field order is kept.
pub(crate) fn project(node: Node, paths: &[&[String]]) -> Node {
    match node {
        Node::List(items) => {
            Node::List(items.into_iter().map(|item| project(item, paths)).collect())
        }
        Node::Map(entries) => Node::Map(
            entries
                .into_iter()
                .filter_map(|(key, value)| {
                    let tails: Vec<&[String]> = paths
                        .iter()
                        .filter(|path| path.first() == Some(&key))
                        .map(|path| &path[1..])
                        .collect();
                    if tails.is_empty() {
                        None
                    } else if tails.iter().any(|tail| tail.is_empty()) {
                        Some((key, value))
                    } else {
                        Some((key, project(value, &tails)))
                    }
                })
                .collect(),
        ),
        other => other,
    }
}
