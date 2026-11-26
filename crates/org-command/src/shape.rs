//! What the declared fields of a row are used for: checking a value against them, the JSON
//! Schema `--schema` prints, the order `data` is printed in, and `--fields`. One declaration
//! drives all four, so the schema and the parser cannot drift.

use serde_json::{Map, Number, Value};

use crate::node::Node;
use crate::row::{Command, Field, Kind, Type};

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
        (Type::Integer, Value::Number(n)) if whole(n).is_some() => Ok(()),
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

/// The largest whole number a JSON number written with a fraction or an exponent holds exactly.
const MAX_EXACT_FLOAT: f64 = 9_007_199_254_740_992.0;

/// `n` as a whole number, when it is one. JSON Schema's `integer` is any number with no
/// fractional part, so `1.0` is one, and it is the integer 1 from here on: the schema and the
/// parser agree, and the office never receives `1.0` for an integer field.
pub(crate) fn whole(n: &Number) -> Option<Number> {
    if n.is_i64() || n.is_u64() {
        return Some(n.clone());
    }
    let f = n.as_f64()?;
    if f.fract() != 0.0 || f.abs() > MAX_EXACT_FLOAT {
        return None;
    }
    Some(if f < 0.0 {
        Number::from(f as i64)
    } else {
        Number::from(f as u64)
    })
}

/// Rewrites every declared integer that is written like `1.0` as the integer it is. With
/// `declared_only`, a field the fields do not declare is also dropped, at every depth.
fn fit(fields: &[Field], object: &mut Map<String, Value>, declared_only: bool) {
    if declared_only {
        object.retain(|key, _| fields.iter().any(|field| field.name == key));
    }
    for field in fields {
        if let Some(value) = object.get_mut(field.name) {
            fit_type(&field.ty, value, declared_only);
        }
    }
}

fn fit_type(ty: &Type, value: &mut Value, declared_only: bool) {
    match (ty, value) {
        (Type::Integer, Value::Number(n)) => {
            if let Some(integer) = whole(n) {
                *n = integer;
            }
        }
        (Type::List(inner), Value::Array(items)) => items
            .iter_mut()
            .for_each(|item| fit_type(inner, item, declared_only)),
        (Type::Object(fields), Value::Object(object)) => fit(fields, object, declared_only),
        _ => {}
    }
}

/// An input object with its whole numbers as integers, before it is compared and checked.
pub(crate) fn normalize(fields: &[Field], object: &mut Map<String, Value>) {
    fit(fields, object, false);
}

/// The office's `data` as the row declares it, in the row's order: a field the row does not
/// declare is dropped, at every depth (a `Type::Json` field is kept whole). `Err` says what is
/// missing or of the wrong type; the answer is then not printed.
pub(crate) fn conform(command: &Command, data: &Value) -> Result<Node, String> {
    let mut data = data.clone();
    match (command.kind, &mut data) {
        (Kind::List, Value::Array(items)) => {
            for item in items {
                if let Value::Object(object) = item {
                    fit(command.output, object, true);
                }
            }
        }
        (Kind::Read | Kind::Write { .. }, Value::Object(object)) => {
            fit(command.output, object, true);
        }
        _ => {}
    }
    validate_data(command, &data)?;
    Ok(order_data(command, &data))
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

/// A command's `data` in the order its row declares: the declared fields and no other.
fn order_data(command: &Command, data: &Value) -> Node {
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
    let entries: Vec<(String, Node)> = fields
        .iter()
        .filter_map(|field| {
            Some((
                field.name.to_owned(),
                order_type(&field.ty, object.get(field.name)?),
            ))
        })
        .collect();
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
