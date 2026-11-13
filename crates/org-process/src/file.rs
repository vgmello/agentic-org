//! The process file's front matter (processes-ux.md §1, processes.md §2.1), decoded by hand so
//! every problem names its field, as the Processes view shows it (PROC-16).

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::place::{is_invisible_or_break, Level, Place, ProcessKey};

/// A valid process file. Status filtering (draft, retired) is resolution's, not this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessFile {
    /// The doc id, `doc-<ULID>`; none until the doc is first saved through the tool.
    pub id: Option<String>,
    /// The stable key that matches the process across levels (`process`).
    pub key: ProcessKey,
    /// The display name.
    pub name: String,
    /// Where it lives, which its `level` and `scope` match.
    pub place: Place,
    /// Bumped by each merged change; 1 or more.
    pub version: u32,
    /// Its owner, as written.
    pub owner: String,
    /// One line.
    pub summary: String,
    /// Who it is for, as written (required: the Processes view shows it for every process).
    pub applies_to: String,
    /// `false` seals it.
    pub overridable: bool,
    /// Its state at this level.
    pub status: Status,
    /// Why it is turned off: required with `status: off`, kept as written (and meaningless)
    /// with any other status.
    pub reason: Option<String>,
    /// For an override: the inherited key and the version it was written against.
    pub overrides: Option<(ProcessKey, u32)>,
    /// Short prompts for guidance moments.
    pub moments: Moments,
    /// Steps added to a task's checklist.
    pub checklist: Vec<Step>,
}

/// A process's state at its level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not in force yet.
    Draft,
    /// In force.
    Active,
    /// Turned off at this level.
    Off,
    /// No longer in force.
    Retired,
}

/// The optional guidance-moment prompts, each one 500 characters at most.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Moments {
    /// Starting a task.
    pub starting_task: Option<String>,
    /// Waking up.
    pub waking_up: Option<String>,
    /// Finishing a task.
    pub finishing_task: Option<String>,
}

/// A checklist step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// What to do, one line.
    pub text: String,
    /// Whether the task can't close without it.
    pub required: bool,
}

/// Why a file isn't valid, and in which field (`checklist[2].text`; empty for the whole file).
/// Only the first problem is reported: an unknown top-level key first (the smallest), then the
/// fields in a fixed order (unknown keys inside `moments` and steps when those are read). What
/// could still be read comes with it, since an invalid file whose key is sealed keeps that key
/// sealed (processes.md §3.2 rule 2); a draft or retired file never seals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The field.
    pub field: String,
    /// The kind of problem.
    pub kind: ProblemKind,
    /// The file's `process` key, if it could be read.
    pub key: Option<ProcessKey>,
    /// Whether it is sealed (`overridable: false`), if that could be read.
    pub sealed: Option<bool>,
}

/// The kinds of problem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    /// The file has no front matter.
    NoFrontMatter,
    /// The front matter isn't acceptable YAML (org-docs' reason).
    FrontMatter(org_docs::Reason),
    /// A key this version doesn't know: refused, never ignored (a typo can't fail open).
    Unrecognized,
    /// A required field is missing.
    Missing,
    /// The value has the wrong type, shape or length.
    Invalid,
    /// `level` or `scope` doesn't match where the file lives.
    WrongPlace,
}

/// The front matter's keys; any other is refused.
const KNOWN: &str = "id process name kind level scope version owner summary applies_to \
                     overridable status reason overrides moments checklist";
const LIMIT_LINE: usize = 200;
const LIMIT_MOMENT: usize = 500;
const LIMIT_STEPS: usize = 50;

fn problem(field: &str, kind: ProblemKind) -> Problem {
    let field = field.to_owned();
    Problem {
        field,
        kind,
        key: None,
        sealed: None,
    }
}

/// Reads the process file at `place` from its text.
pub fn read(place: &Place, text: &str) -> Result<ProcessFile, Problem> {
    let doc =
        org_docs::parse(text).map_err(|e| problem(&e.field, ProblemKind::FrontMatter(e.reason)))?;
    let map = doc
        .front_matter
        .ok_or_else(|| problem("", ProblemKind::NoFrontMatter))?;
    decode(place, &map).map_err(|mut problem| {
        let raw = |key: &str| map.get(key);
        problem.key = raw("process")
            .and_then(Value::as_str)
            .and_then(ProcessKey::new);
        let exempt = matches!(
            raw("status").and_then(Value::as_str),
            Some("draft" | "retired")
        );
        let overridable = raw("overridable").and_then(Value::as_bool);
        problem.sealed = if exempt {
            Some(false)
        } else {
            overridable.map(|o| !o)
        };
        problem
    })
}

fn decode(place: &Place, map: &Map<String, Value>) -> Result<ProcessFile, Problem> {
    let known: Vec<&str> = KNOWN.split(' ').collect();
    unknown(map, &known, "")?;
    let get = |key: &str| map.get(key).filter(|v| !v.is_null());
    let need = |key: &str| get(key).ok_or_else(|| problem(key, ProblemKind::Missing));
    let invalid = |key: &str| problem(key, ProblemKind::Invalid);
    // A doc gets its id on its first save through the tool; one written with git has none.
    let id = get("id").map(|v| line(v, "id", LIMIT_LINE)).transpose()?;
    if !id.as_deref().is_none_or(is_doc_id) {
        return Err(invalid("id"));
    }
    if need("kind")?.as_str() != Some("process") {
        return Err(invalid("kind"));
    }
    let level = need("level")?.as_str().and_then(Level::parse);
    let level = level.ok_or_else(|| invalid("level"))?;
    if level != place.level() {
        return Err(problem("level", ProblemKind::WrongPlace));
    }
    match (get("scope"), place.scope()) {
        (None, None) => {}
        (Some(scope), Some(expected)) if scope.as_str() == Some(expected) => {}
        (Some(scope), _) if !scope.is_string() => return Err(invalid("scope")),
        _ => return Err(problem("scope", ProblemKind::WrongPlace)),
    }
    let key = ProcessKey::new(need("process")?.as_str().unwrap_or(""))
        .ok_or_else(|| invalid("process"))?;
    let version = count(need("version")?)
        .filter(|v| *v >= 1)
        .ok_or_else(|| invalid("version"))?;
    let status = match need("status")?.as_str() {
        Some("draft") => Status::Draft,
        Some("active") => Status::Active,
        Some("off") => Status::Off,
        Some("retired") => Status::Retired,
        _ => return Err(invalid("status")),
    };
    let optional = |key: &str| get(key).map(|v| line(v, key, LIMIT_LINE)).transpose();
    let reason = optional("reason")?;
    if status == Status::Off && reason.is_none() {
        return Err(problem("reason", ProblemKind::Missing));
    }
    // An override names its own key (the inherited process) and the version it was written
    // against: digits only, no sign or leading zero.
    let overrides = match get("overrides") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .and_then(|s| s.split_once('@'))
                .filter(|(of, v)| {
                    *of == key.as_str()
                        && !v.starts_with('0')
                        && v.bytes().all(|b| b.is_ascii_digit())
                })
                .and_then(|(_, v)| Some((key.clone(), v.parse().ok()?)))
                .ok_or_else(|| invalid("overrides"))?,
        ),
    };
    Ok(ProcessFile {
        id,
        key,
        name: line(need("name")?, "name", LIMIT_LINE)?,
        place: place.clone(),
        version,
        owner: line(need("owner")?, "owner", LIMIT_LINE)?,
        summary: line(need("summary")?, "summary", LIMIT_LINE)?,
        applies_to: line(need("applies_to")?, "applies_to", LIMIT_LINE)?,
        overridable: need("overridable")?
            .as_bool()
            .ok_or_else(|| invalid("overridable"))?,
        status,
        reason,
        overrides,
        moments: get("moments").map(moments).transpose()?.unwrap_or_default(),
        checklist: get("checklist")
            .map(checklist)
            .transpose()?
            .unwrap_or_default(),
    })
}

/// Refuses any key of `map` not in `known`, naming the smallest.
fn unknown(map: &Map<String, Value>, known: &[&str], at: &str) -> Result<(), Problem> {
    let known: BTreeSet<&str> = known.iter().copied().collect();
    match map.keys().filter(|key| !known.contains(key.as_str())).min() {
        Some(key) if at.is_empty() => Err(problem(key, ProblemKind::Unrecognized)),
        Some(key) => Err(problem(&format!("{at}.{key}"), ProblemKind::Unrecognized)),
        None => Ok(()),
    }
}

/// One line of 1 to `limit` characters: no control characters, Unicode line or paragraph
/// separators or characters that hide text (`is_invisible_or_break`), and no surrounding spaces.
fn line(value: &Value, field: &str, limit: usize) -> Result<String, Problem> {
    let ok = |s: &&str| {
        let breaks = |c: char| c.is_control() || is_invisible_or_break(c);
        !s.is_empty() && s.trim() == *s && !s.contains(breaks) && s.chars().count() <= limit
    };
    value
        .as_str()
        .filter(ok)
        .map(str::to_owned)
        .ok_or_else(|| problem(field, ProblemKind::Invalid))
}

fn count(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// `doc-` and a lowercase Crockford ULID (tasks-projects-docs.md §4.9), whose first character
/// is 0-7 (a larger one overflows 128 bits).
fn is_doc_id(id: &str) -> bool {
    let s = id.strip_prefix("doc-").unwrap_or("");
    let crockford = |b: u8| b.is_ascii_digit() || (b.is_ascii_lowercase() && !b"ilou".contains(&b));
    s.len() == 26 && s.bytes().all(crockford) && (b'0'..=b'7').contains(&s.as_bytes()[0])
}

fn moments(value: &Value) -> Result<Moments, Problem> {
    let map = value
        .as_object()
        .ok_or_else(|| problem("moments", ProblemKind::Invalid))?;
    unknown(
        map,
        &["starting_task", "waking_up", "finishing_task"],
        "moments",
    )?;
    let moment = |key: &str| {
        let field = format!("moments.{key}");
        map.get(key)
            .filter(|v| !v.is_null())
            .map(|v| line(v, &field, LIMIT_MOMENT))
            .transpose()
    };
    Ok(Moments {
        starting_task: moment("starting_task")?,
        waking_up: moment("waking_up")?,
        finishing_task: moment("finishing_task")?,
    })
}

fn checklist(value: &Value) -> Result<Vec<Step>, Problem> {
    let steps = value.as_array().filter(|s| s.len() <= LIMIT_STEPS);
    let steps = steps.ok_or_else(|| problem("checklist", ProblemKind::Invalid))?;
    let mut out = Vec::with_capacity(steps.len());
    for (n, step) in steps.iter().enumerate() {
        let at = format!("checklist[{n}]");
        let map = step
            .as_object()
            .ok_or_else(|| problem(&at, ProblemKind::Invalid))?;
        unknown(map, &["text", "required"], &at)?;
        let text_at = format!("{at}.text");
        let get = |key: &str| map.get(key).filter(|v| !v.is_null());
        let text = get("text").ok_or_else(|| problem(&text_at, ProblemKind::Missing))?;
        let text = line(text, &text_at, LIMIT_LINE)?;
        let required = match get("required") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| problem(&format!("{at}.required"), ProblemKind::Invalid))?,
        };
        out.push(Step { text, required });
    }
    Ok(out)
}
