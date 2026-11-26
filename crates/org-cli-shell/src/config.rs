//! The non-secret selection of the instance and the office, in five tiers: the flag, the
//! environment variable (empty is unset), the nearest ancestor `agentic-org.toml`, the user's
//! saved context, the built-in default.
//!
//! The manifest selects and nothing more: it never names sockets, release pins or custody,
//! and it accepts no other key (`deny_unknown_fields`). It is read only when the flags and the
//! environment leave a value open, and then a malformed one is an error, never a silent fall
//! through. Its errors name the file, line and column only: never the parser's message or
//! any of its text, which may hold something secret pasted by mistake.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ShellError;
use crate::escape::is_unsafe_char;
use crate::files::{read_regular, ReadRefusal};

/// Selects the instance (tier 2).
pub const INSTANCE_ENV: &str = "AGENTIC_ORG_INSTANCE";
/// Selects the office (tier 2).
pub const OFFICE_ENV: &str = "AGENTIC_ORG_OFFICE";
/// The manifest, looked up from the working directory upwards (tier 3).
pub const MANIFEST_FILE: &str = "agentic-org.toml";
/// The instance flag (tier 1) and its alias.
pub const INSTANCE_FLAG: &str = "--name";
pub const INSTANCE_FLAG_ALIAS: &str = "--instance";
/// The office flag (tier 1).
pub const OFFICE_FLAG: &str = "--office";
/// The most of a manifest that is read.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// The longest instance or office value, in bytes.
pub const MAX_VALUE_BYTES: usize = 128;

const MANIFEST_HELP: &str = "agentic-org.toml may set only `instance` and `office`, each a \
     string of 1 to 128 bytes without control characters.";

/// `agentic-org.toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub instance: Option<String>,
    pub office: Option<String>,
}

/// The nearest `agentic-org.toml` that is not a directory, in `start` or an ancestor. A
/// symbolic link, a FIFO or a device is found, never followed or opened here:
/// [`load_manifest`] refuses it, so it can neither redirect the selection nor be skipped for a
/// manifest further up.
pub fn find_manifest(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(MANIFEST_FILE))
        .find(|path| fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.is_dir()))
}

/// Reads and checks a manifest (at most [`MAX_MANIFEST_BYTES`]): opened without following a
/// symbolic link or blocking on a FIFO, a regular file on the opened handle, read bounded
/// ([`crate::files::read_regular`]). A parse error is [`ShellError::InvalidConfig`]
/// `"<path>: not a valid agentic-org.toml at line L, column C"`.
pub fn load_manifest(path: &Path) -> Result<Manifest, ShellError> {
    let shown = path.display().to_string();
    let bytes = match read_regular(path, MAX_MANIFEST_BYTES) {
        Ok(bytes) => bytes,
        Err(ReadRefusal::NotRegular) => {
            return Err(invalid_manifest(format!(
                "{shown}: not a regular file (a symbolic link is refused)"
            )))
        }
        Err(ReadRefusal::TooLarge { .. }) => {
            return Err(invalid_manifest(format!("{shown}: larger than 64 KiB")))
        }
        Err(ReadRefusal::Io(e)) => return Err(ShellError::io(format!("reading {shown}"), e)),
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            let prefix = std::str::from_utf8(&e.as_bytes()[..valid]).unwrap_or_default();
            return Err(parse_error(&shown, prefix, valid));
        }
    };
    let manifest: Manifest = toml::from_str(&text).map_err(|e| {
        let offset = e.span().map_or(0, |span| span.start);
        parse_error(&shown, &text, offset)
    })?;
    for (key, value) in [
        ("instance", &manifest.instance),
        ("office", &manifest.office),
    ] {
        if value.as_deref().is_some_and(|value| !valid_value(value)) {
            return Err(invalid_manifest(format!("{shown}: `{key}` {VALUE_RULE}")));
        }
    }
    Ok(manifest)
}

const VALUE_RULE: &str = "must be 1 to 128 bytes of text without control characters";

fn invalid_manifest(message: String) -> ShellError {
    ShellError::InvalidConfig {
        message,
        help: Some(MANIFEST_HELP.to_owned()),
    }
}

/// The error at byte `offset` of `text`: its line and column only.
fn parse_error(shown: &str, text: &str, offset: usize) -> ShellError {
    let (line, column) = line_column(text, offset);
    invalid_manifest(format!(
        "{shown}: not a valid {MANIFEST_FILE} at line {line}, column {column}"
    ))
}

/// One-based line and column (in characters) of byte `offset`.
fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let (mut line, mut column) = (1, 1);
    for (index, c) in text.char_indices() {
        if index >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn valid_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_VALUE_BYTES && !value.chars().any(is_unsafe_char)
}

/// Which tier decided a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Flag,
    Env,
    Manifest,
    Saved,
    Default,
}

impl Tier {
    /// `flag`, `env`, `manifest`, `saved` or `default`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Env => "env",
            Self::Manifest => "manifest",
            Self::Saved => "saved",
            Self::Default => "default",
        }
    }
}

/// One selected value and where it came from. `value` is `None` only at [`Tier::Default`]
/// without a default (the office: the consumer picks the sole or HQ office).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
    pub value: Option<String>,
    pub tier: Tier,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub instance: Selected,
    pub office: Selected,
    /// The manifest that was read, if one was needed and found.
    pub manifest: Option<PathBuf>,
}

/// What the caller knows: the flags, the saved context, the default instance, and where to
/// start looking for a manifest (`None`: no manifest).
#[derive(Clone, Copy, Debug, Default)]
pub struct SelectionInput<'a> {
    pub flag_instance: Option<&'a str>,
    pub flag_office: Option<&'a str>,
    pub saved_instance: Option<&'a str>,
    pub saved_office: Option<&'a str>,
    pub default_instance: Option<&'a str>,
    pub start_dir: Option<&'a Path>,
}

/// Selects the instance and the office (see the module documentation). Flag values are
/// checked like manifest values ([`ShellError::InvalidArguments`]); environment values and
/// saved values too ([`ShellError::InvalidConfig`]: a saved context is a file anyone with the
/// account's access may have edited, so a control, line-breaking or bidirectional character
/// in it fails closed); no error repeats the value. The default is the caller's and is taken
/// as it is. An empty value is unset.
pub fn resolve_selection(
    input: &SelectionInput<'_>,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Selection, ShellError> {
    let flag_instance = flag_value(input.flag_instance, INSTANCE_FLAG)?;
    let flag_office = flag_value(input.flag_office, OFFICE_FLAG)?;
    let env_instance = env_value(env, INSTANCE_ENV)?;
    let env_office = env_value(env, OFFICE_ENV)?;
    let open = (flag_instance.is_none() && env_instance.is_none())
        || (flag_office.is_none() && env_office.is_none());
    let (manifest, manifest_path) = match input.start_dir.filter(|_| open).and_then(find_manifest) {
        Some(path) => (load_manifest(&path)?, Some(path)),
        None => (Manifest::default(), None),
    };
    let saved_instance = saved_value(input.saved_instance, "instance")?;
    let saved_office = saved_value(input.saved_office, "office")?;
    let default = |value: Option<&str>| value.filter(|value| !value.is_empty()).map(str::to_owned);
    Ok(Selection {
        instance: pick(
            [
                (flag_instance, Tier::Flag),
                (env_instance, Tier::Env),
                (manifest.instance, Tier::Manifest),
                (saved_instance, Tier::Saved),
            ],
            default(input.default_instance),
        ),
        office: pick(
            [
                (flag_office, Tier::Flag),
                (env_office, Tier::Env),
                (manifest.office, Tier::Manifest),
                (saved_office, Tier::Saved),
            ],
            None,
        ),
        manifest: manifest_path,
    })
}

fn pick(tiers: [(Option<String>, Tier); 4], default: Option<String>) -> Selected {
    tiers
        .into_iter()
        .find_map(|(value, tier)| {
            value.map(|value| Selected {
                value: Some(value),
                tier,
            })
        })
        .unwrap_or(Selected {
            value: default,
            tier: Tier::Default,
        })
}

fn flag_value(value: Option<&str>, flag: &str) -> Result<Option<String>, ShellError> {
    match value {
        Some(value) if !valid_value(value) => Err(ShellError::InvalidArguments {
            message: format!("the {flag} value {VALUE_RULE}"),
        }),
        other => Ok(other.map(str::to_owned)),
    }
}

/// A value of the saved context: empty is unset; one that is not a usable value is
/// [`ShellError::InvalidConfig`], never selected or shown.
fn saved_value(value: Option<&str>, what: &str) -> Result<Option<String>, ShellError> {
    match value.filter(|value| !value.is_empty()) {
        Some(value) if !valid_value(value) => Err(ShellError::InvalidConfig {
            message: format!("the saved context's {what} {VALUE_RULE}"),
            help: Some(
                "Clear the saved context (`agentic-org context clear`) and save it again."
                    .to_owned(),
            ),
        }),
        other => Ok(other.map(str::to_owned)),
    }
}

fn env_value(
    env: &dyn Fn(&str) -> Option<OsString>,
    name: &str,
) -> Result<Option<String>, ShellError> {
    let Some(raw) = env(name).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    match raw.into_string() {
        Ok(value) if valid_value(&value) => Ok(Some(value)),
        _ => Err(ShellError::InvalidConfig {
            message: format!("{name} {VALUE_RULE}"),
            help: Some(format!("Fix or unset {name}.")),
        }),
    }
}

#[cfg(test)]
mod tests;
