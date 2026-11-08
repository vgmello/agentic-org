//! A feature: a registry entry with a versioned prompt template and its schemas.

use std::fmt;

use serde_json::{json, Value};

use crate::contract::FeatureInfo;
use crate::schema::{Schema, SchemaError};

const SCHEMA_HOLE: &str = "{{schema}}";
const DATA_HOLE: &str = "{{data}}";

/// Notes: at most this many, each at most this many characters.
pub const MAX_NOTES: u64 = 5;
pub const MAX_NOTE_CHARS: u64 = 200;

/// Why a feature definition is not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureError {
    /// The id must be lowercase words joined by `.` or `_`, e.g. `text.improve`.
    BadId,
    /// The template needs `{{schema}}` once, then `{{data}}` once, and nothing else of that form.
    BadTemplate,
    /// A schema is not valid (which one is in the message).
    Schema(&'static str, SchemaError),
}

impl fmt::Display for FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeatureError::BadId => {
                f.write_str("feature id must be lowercase words joined by '.' or '_'")
            }
            FeatureError::BadTemplate => {
                f.write_str("template needs one {{schema}} hole, then one {{data}} hole")
            }
            FeatureError::Schema(which, e) => write!(f, "{which} schema: {e}"),
        }
    }
}

impl std::error::Error for FeatureError {}

/// The template, split at its two holes.
#[derive(Debug, Clone)]
pub(crate) struct Template {
    pub head: String,
    pub middle: String,
    pub tail: String,
}

/// A smart feature. Build one with [`Feature::new`]; the built-in ones are in [`crate::builtin`].
#[derive(Debug, Clone)]
pub struct Feature {
    id: String,
    version: u32,
    title: String,
    template: Template,
    context_kinds: Vec<String>,
    /// Draft fields that name something from the context: (draft field, context kind).
    context_refs: Vec<(String, String)>,
    input: Schema,
    output: Schema,
}

impl Feature {
    /// `draft_schema` describes the `draft`; the answer the runtime must give is
    /// `{ "draft": <draft>, "notes": [string, ...] }`, built here.
    pub fn new(
        id: &str,
        version: u32,
        title: &str,
        template: &str,
        context_kinds: &[&str],
        input_schema: Value,
        draft_schema: Value,
    ) -> Result<Feature, FeatureError> {
        let id_ok = !id.is_empty()
            && !id.starts_with(['.', '_'])
            && !id.ends_with(['.', '_'])
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || matches!(c, '.' | '_'));
        if !id_ok {
            return Err(FeatureError::BadId);
        }
        let template = split_template(template)?;
        let input = Schema::compile(input_schema).map_err(|e| FeatureError::Schema("input", e))?;
        let envelope = json!({
            "type": "object",
            "properties": {
                "draft": draft_schema,
                "notes": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": MAX_NOTE_CHARS },
                    "maxItems": MAX_NOTES
                }
            },
            "required": ["draft"],
            "additionalProperties": false
        });
        let output = Schema::compile(envelope).map_err(|e| FeatureError::Schema("draft", e))?;
        Ok(Feature {
            id: id.to_owned(),
            version,
            title: title.to_owned(),
            template,
            context_kinds: context_kinds.iter().map(|k| (*k).to_owned()).collect(),
            context_refs: Vec::new(),
            input,
            output,
        })
    }

    /// Say that these top-level fields of the draft name something the office gave as context:
    /// `("area_id", "area")` means `draft.area_id` must be the `text` of a context item of kind
    /// `area`. The copilot drops a field whose value isn't one it was given, so a model can't
    /// invent (or be talked into) an id.
    pub fn with_context_refs(mut self, refs: &[(&str, &str)]) -> Self {
        self.context_refs = refs
            .iter()
            .map(|(f, k)| ((*f).to_owned(), (*k).to_owned()))
            .collect();
        self
    }

    pub(crate) fn context_refs(&self) -> &[(String, String)] {
        &self.context_refs
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// The schema `copilot.draft`'s `input` must match; it carries the per-field caps.
    pub fn input_schema(&self) -> &Schema {
        &self.input
    }

    /// The schema of the runtime's whole answer (`draft` and `notes`).
    pub fn output_schema(&self) -> &Schema {
        &self.output
    }

    /// True when the office may attach context of this kind.
    pub fn accepts_context(&self, kind: &str) -> bool {
        self.context_kinds.iter().any(|k| k == kind)
    }

    pub fn info(&self) -> FeatureInfo {
        FeatureInfo {
            id: self.id.clone(),
            version: self.version,
            title: self.title.clone(),
        }
    }

    pub(crate) fn template(&self) -> &Template {
        &self.template
    }
}

fn split_template(template: &str) -> Result<Template, FeatureError> {
    let bad = FeatureError::BadTemplate;
    if template.matches(SCHEMA_HOLE).count() != 1 || template.matches(DATA_HOLE).count() != 1 {
        return Err(bad);
    }
    let (head, rest) = template
        .split_once(SCHEMA_HOLE)
        .ok_or(FeatureError::BadTemplate)?;
    let (middle, tail) = rest
        .split_once(DATA_HOLE)
        .ok_or(FeatureError::BadTemplate)?;
    // The data goes after the schema, so `{{data}}` before `{{schema}}` is refused.
    if head.contains(DATA_HOLE) {
        return Err(FeatureError::BadTemplate);
    }
    Ok(Template {
        head: head.to_owned(),
        middle: middle.to_owned(),
        tail: tail.to_owned(),
    })
}

/// The features this build offers, by id.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    features: Vec<Feature>,
}

impl Registry {
    /// `None` when two features share an id.
    pub fn new(features: Vec<Feature>) -> Option<Registry> {
        let mut seen = std::collections::HashSet::new();
        features
            .iter()
            .all(|f| seen.insert(f.id.clone()))
            .then_some(Registry { features })
    }

    pub fn get(&self, id: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Feature> {
        self.features.iter()
    }
}
