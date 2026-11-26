//! What `systemctl show` reports of a unit, and the comparison of what systemd would run
//! with what the definition says.

use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

use crate::service::{ServiceError, Unit};

use super::unit_file::unset_variables;
use super::{Outcome, EFFECTIVE, MINIMUM_VERSION};

/// What `systemctl show` says of a unit.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Shown {
    pub(super) load: String,
    pub(super) active: String,
    pub(super) pid: u32,
    pub(super) fragment: String,
    pub(super) file_state: String,
    pub(super) needs_reload: bool,
    /// The drop-in files systemd applies to the unit, as it lists them.
    pub(super) drop_ins: String,
    /// The [`EFFECTIVE`] properties systemd printed, by name, as it printed them.
    pub(super) effective: Vec<(&'static str, String)>,
}

impl Shown {
    /// The main process, while the unit has one that is not just scheduled to start.
    pub(super) fn running(&self) -> Option<u32> {
        let live = matches!(
            self.active.as_str(),
            "active" | "reloading" | "activating" | "deactivating"
        );
        (live && self.pid != 0).then_some(self.pid)
    }

    /// What register established and nothing has changed since: systemd read our file as
    /// it is now, and the unit is enabled.
    pub(super) fn settled(&self) -> bool {
        self.load == "loaded" && self.file_state == "enabled" && !self.needs_reload
    }

    /// The value systemd printed for the [`EFFECTIVE`] property `name`; a property it did
    /// not print is empty (`systemctl show` leaves out an empty list).
    fn effective(&self, name: &str) -> &str {
        self.effective
            .iter()
            .find(|(key, _)| *key == name)
            .map_or("", |(_, value)| value.as_str())
    }
}

/// `path` with the links of its deepest existing ancestor resolved, for comparing a
/// directory that may not exist yet with one systemd names.
pub(super) fn resolved(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(found) = fs::canonicalize(ancestor) {
            return match path.strip_prefix(ancestor) {
                Ok(rest) if !rest.as_os_str().is_empty() => found.join(rest),
                _ => found,
            };
        }
    }
    path.to_owned()
}

/// `systemctl`'s own words when no user manager answers it.
pub(super) fn no_manager(stderr: &str) -> bool {
    stderr.contains("Failed to connect to bus")
        || stderr.contains("Failed to connect to user scope bus")
        || stderr.contains("Failed to get D-Bus connection")
}

/// The first line a program wrote about its failure, for an error text: escaped, and
/// shortened so that a program's output never floods a terminal.
pub(super) fn said(out: &Outcome) -> String {
    let line = out
        .stderr
        .lines()
        .chain(out.stdout.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("it printed nothing");
    line.chars()
        .take(200)
        .flat_map(char::escape_default)
        .collect()
}

/// The value of `key` in `Key=Value` lines.
pub(super) fn property<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    output.lines().find_map(|line| {
        line.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
    })
}

/// Refuses a user manager older than [`MINIMUM_VERSION`], from what `systemctl show
/// --property=Version` printed: `255`, `252 (252.22-1~deb12u1)` or `249.11-0ubuntu3.12`,
/// whose leading number is the version. One that does not start with a number is not
/// refused here: what such a manager would run is still compared with the definition.
pub(super) fn recent_enough(out: &Outcome) -> Result<(), ServiceError> {
    let Some(version) = property(&out.stdout, "Version") else {
        return Ok(());
    };
    let digits: String = version
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    match digits.parse::<u32>() {
        Ok(major) if major < MINIMUM_VERSION => Err(ServiceError::Unavailable(format!(
            "systemd's user manager is version {major}, and the service's unit needs \
             {MINIMUM_VERSION} or later (Type=exec)"
        ))),
        _ => Ok(()),
    }
}

/// `systemctl show`'s `Key=Value` lines for [`PROPERTIES`]. A unit systemd does not know
/// is answered too (`LoadState=not-found`); output without the load and active state is
/// not an answer.
///
/// [`PROPERTIES`]: super::PROPERTIES
pub(super) fn parse_show(output: &str) -> Option<Shown> {
    Some(Shown {
        load: property(output, "LoadState")?.to_owned(),
        active: property(output, "ActiveState")?.to_owned(),
        pid: property(output, "MainPID")
            .and_then(|pid| pid.parse().ok())
            .unwrap_or(0),
        fragment: property(output, "FragmentPath").unwrap_or("").to_owned(),
        file_state: property(output, "UnitFileState").unwrap_or("").to_owned(),
        needs_reload: property(output, "NeedDaemonReload") == Some("yes"),
        drop_ins: property(output, "DropInPaths").unwrap_or("").to_owned(),
        effective: EFFECTIVE
            .iter()
            .filter_map(|name| Some((*name, property(output, name)?.to_owned())))
            .collect(),
    })
}

/// The first of the [`EFFECTIVE`] properties for which what systemd reports differs from
/// what the unit `defined` says, or `None`. systemd 257 prints `ExecStart` as
/// `{ path=<program> ; argv[]=<words> ; ignore_errors=no ; … }`, where the words are the
/// program and its arguments joined by spaces, unquoted, each `$` still written `$$`; and
/// `Environment=` and `UnsetEnvironment=` as words separated by spaces, double-quoted where
/// they hold a space or a character a shell treats specially ([`shell_words`]). So
/// `ExecStart` compares the joined text: a change that only regroups words around a space
/// is not seen. Units hold no control character ([`unit_file_safe`]), which systemd would
/// print raw in an argument and break the line. A systemd that quotes differently makes a
/// value with a space or such a character differ, and the unit is refused: never accepted
/// on doubt. The command must also stop at our words: an extra argument after an argument
/// `;` shows as more text before `ignore_errors=`.
///
/// [`unit_file_safe`]: super::unit_file::unit_file_safe
pub(super) fn effective_differs(defined: &Unit, shown: &Shown) -> Option<&'static str> {
    let exec = std::iter::once(defined.program.as_os_str())
        .chain(defined.args.iter().map(OsString::as_os_str))
        .map(|word| word.to_string_lossy().replace('$', "$$"))
        .collect::<Vec<_>>()
        .join(" ");
    let start = shown.effective("ExecStart");
    // One command, our words and nothing after them: the field that follows the arguments
    // comes next, and appears once (an argument `;` would otherwise hide more words).
    let one_command = start
        .strip_prefix(&format!(
            "{{ path={} ; argv[]={exec} ; ignore_errors=no ; ",
            defined.program.display()
        ))
        .is_some_and(|rest| rest.ends_with(" }") && !rest.contains("{ path="))
        && start.matches("ignore_errors=").count() == 1;
    if !one_command {
        return Some("ExecStart");
    }
    let sorted = |mut words: Vec<String>| {
        words.sort();
        words
    };
    let environment: Vec<String> = defined
        .env
        .iter()
        .map(|(key, value)| format!("{}={}", key.to_string_lossy(), value.to_string_lossy()))
        .collect();
    if shell_words(shown.effective("Environment")).map(sorted) != Some(sorted(environment)) {
        return Some("Environment");
    }
    let unset: Vec<String> = unset_variables(defined)
        .into_iter()
        .map(str::to_owned)
        .collect();
    if shell_words(shown.effective("UnsetEnvironment")).map(sorted) != Some(sorted(unset)) {
        return Some("UnsetEnvironment");
    }
    let expected = [
        ("ExecStartPre", ""),
        ("ExecStartPost", ""),
        ("ExecCondition", ""),
        ("ExecStop", ""),
        ("ExecStopPost", ""),
        ("EnvironmentFiles", ""),
        ("PassEnvironment", ""),
        ("Type", "exec"),
        ("KillMode", "mixed"),
        ("KillSignal", "15"),
        ("SendSIGKILL", "yes"),
        ("TimeoutStopUSec", "15s"),
        ("Restart", "on-failure"),
        ("UMask", "0077"),
        ("NoNewPrivileges", "yes"),
    ];
    expected
        .into_iter()
        .find(|(name, value)| shown.effective(name) != *value)
        .map(|(name, _)| name)
}

/// The words of a list `systemctl show` prints, such as `Environment=`: separated by
/// spaces; a word, or part of one, in double quotes may hold spaces, and inside the quotes
/// a backslash makes the next `\`, `"`, `$` or `` ` `` literal. `None` for anything else
/// (an unfinished quote, another escape): that is not compared, it differs.
pub(super) fn shell_words(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quoted = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' if !quoted => words.extend(word.take()),
            '"' => {
                quoted = !quoted;
                word.get_or_insert_with(String::new);
            }
            '\\' if quoted => match chars.next()? {
                escaped @ ('\\' | '"' | '$' | '`') => {
                    word.get_or_insert_with(String::new).push(escaped)
                }
                _ => return None,
            },
            '\\' => return None,
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    if quoted {
        return None;
    }
    words.extend(word);
    Some(words)
}

/// Whether the drop-in file `path` is one systemd applies to the unit `name` by its name:
/// it is in `<name>.d/`, or in the folder of a prefix of the name that ends at a dash
/// (`agentic-org-.service.d/` for `agentic-org-<id>.service`, and every longer one up to
/// the last dash of the instance ID), as systemd.unit(5) has it. Such a drop-in reaches
/// only units named like ours, and can replace `ExecStart` while the unit's file is still
/// our definition. The folder for every unit of the type (`service.d/`) is not one: it
/// applies to every service of the user, and distributions ship drop-ins there.
pub(super) fn applies_by_name(name: &str, path: &str) -> bool {
    let Some(folder) = Path::new(path)
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
    else {
        return false;
    };
    let Some(stem) = name.strip_suffix(".service") else {
        return folder == format!("{name}.d");
    };
    folder == format!("{name}.d")
        || stem
            .match_indices('-')
            .any(|(dash, _)| folder == format!("{}.service.d", &stem[..=dash]))
}

pub(super) fn canonical(path: &Path) -> Option<PathBuf> {
    match path.parent() {
        Some(parent) => Some(fs::canonicalize(parent).ok()?.join(path.file_name()?)),
        None => None,
    }
}
