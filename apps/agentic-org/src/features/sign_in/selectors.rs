//! The words of an office command as `agentic-org` takes them: the instance and office
//! selectors (`--name`/`--instance`, `--office`) and the global flags written after the
//! resource (`-q`, `--no-input`, `--color`, `--no-color`) are taken out; everything else goes
//! to the command table as written. A flag's value is never taken for a flag: the scan knows
//! which flags take one from the table's row.

/// What the scan took out of the words.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Selectors {
    pub(super) instance: Option<String>,
    pub(super) office: Option<String>,
    pub(super) forwarded: Vec<String>,
    pub(super) quiet: bool,
    pub(super) no_input: bool,
    pub(super) no_color: bool,
    pub(super) color: Option<String>,
}

fn set_selector(slot: &mut Option<String>, flag: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{flag} needs a value"));
    }
    if slot.replace(value.to_owned()).is_some() {
        return Err(format!("{flag} is given more than once"));
    }
    Ok(())
}

/// `<resource> <verb>` of the forwarded words (`task note <verb>` is `task-note <verb>`).
pub(super) fn resource_and_verb(forwarded: &[String]) -> Option<(&str, &str)> {
    match forwarded {
        [a, b, v, ..] if a == "task" && b == "note" => Some(("task-note", v.as_str())),
        [a, ..] if a == "who" => Some(("directory", "list")),
        [r, v, ..] => Some((r.as_str(), v.as_str())),
        _ => None,
    }
}

const COMMON_VALUE_OPTIONS: [&str; 9] = [
    "format",
    "fields",
    "limit",
    "cursor",
    "json",
    "input-file",
    "input-format",
    "idempotency-key",
    "if-match",
];

fn flag_takes_value(forwarded: &[String], flag_name: &str) -> bool {
    if COMMON_VALUE_OPTIONS.contains(&flag_name) {
        return true;
    }
    let Some((resource, verb)) = resource_and_verb(forwarded) else {
        return false;
    };
    org_agent_cli::table::runtime_table()
        .find(resource, verb)
        .is_some_and(|cmd| {
            cmd.input
                .iter()
                .any(|f| f.is_flag() && f.flag() == flag_name && f.ty != org_command::Type::Boolean)
        })
}

/// Whether one of `names` (`--dry-run`, `-h`, ...) is written as a flag in `forwarded`, as it
/// is or as `--name=value`. The words are read as [`split`] and the table read them: the value
/// of a flag that takes one is text, never a flag (`--body --dry-run` sends the text
/// `--dry-run`), and nothing after `--` is a flag.
pub(super) fn has_flag(forwarded: &[String], names: &[&str]) -> bool {
    let mut words = forwarded.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            return false;
        }
        let named = names.iter().any(|name| {
            word == name
                || word
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with('='))
        });
        if named {
            return true;
        }
        if let Some(flag_name) = word.strip_prefix("--") {
            if !flag_name.contains('=') && flag_takes_value(forwarded, flag_name) {
                words.next();
            }
        }
    }
    false
}

fn command_accepts_flag(forwarded: &[String], flag_name: &str) -> bool {
    let Some((resource, verb)) = resource_and_verb(forwarded) else {
        return false;
    };
    org_agent_cli::table::runtime_table()
        .find(resource, verb)
        .is_some_and(|cmd| {
            cmd.input
                .iter()
                .any(|f| f.is_flag() && f.flag() == flag_name)
        })
}

fn try_consume_selector<'a, I: Iterator<Item = &'a String>>(
    arg: &str,
    iter: &mut I,
    scanned: &mut Selectors,
) -> Result<bool, String> {
    let name_is_command_flag = command_accepts_flag(&scanned.forwarded, "name");
    if let Some(val) = arg.strip_prefix("--instance=") {
        set_selector(&mut scanned.instance, "--instance", val)?;
        return Ok(true);
    }
    if !name_is_command_flag {
        if let Some(val) = arg.strip_prefix("--name=") {
            set_selector(&mut scanned.instance, "--name", val)?;
            return Ok(true);
        }
    }
    if let Some(val) = arg.strip_prefix("--office=") {
        set_selector(&mut scanned.office, "--office", val)?;
        return Ok(true);
    }
    if let Some(val) = arg.strip_prefix("--color=") {
        scanned.color = Some(val.to_owned());
        return Ok(true);
    }
    match arg {
        "-q" | "--quiet" => scanned.quiet = true,
        "--no-input" => scanned.no_input = true,
        "--no-color" => scanned.no_color = true,
        "--color" => {
            let value = iter
                .next()
                .ok_or_else(|| "--color needs a value".to_owned())?;
            scanned.color = Some(value.clone());
        }
        "--instance" | "--office" => take_value(arg, iter, scanned)?,
        "--name" if !name_is_command_flag => take_value(arg, iter, scanned)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn take_value<'a, I: Iterator<Item = &'a String>>(
    arg: &str,
    iter: &mut I,
    scanned: &mut Selectors,
) -> Result<(), String> {
    let value = iter
        .next()
        .filter(|v| !v.starts_with("--"))
        .ok_or_else(|| format!("{arg} needs a value"))?;
    let slot = if arg == "--office" {
        &mut scanned.office
    } else {
        &mut scanned.instance
    };
    set_selector(slot, arg, value)
}

/// Splits the words of an office command. A flag that takes a value but is the last word is
/// refused here, naming the flag: nothing is sent, and no word added later (`--dry-run`,
/// `--format`) can become its value.
pub(super) fn split(args: &[String]) -> Result<Selectors, String> {
    let mut scanned = Selectors::default();
    let mut iter = args.iter();
    let mut awaiting: Option<&str> = None;
    let mut after_terminator = false;
    while let Some(arg) = iter.next() {
        if awaiting.take().is_some() || after_terminator {
            scanned.forwarded.push(arg.clone());
            continue;
        }
        if arg == "--" {
            after_terminator = true;
            scanned.forwarded.push(arg.clone());
            continue;
        }
        if try_consume_selector(arg, &mut iter, &mut scanned)? {
            continue;
        }
        if let Some(flag_name) = arg.strip_prefix("--") {
            if !flag_name.contains('=') && flag_takes_value(&scanned.forwarded, flag_name) {
                awaiting = Some(arg);
            }
        }
        scanned.forwarded.push(arg.clone());
    }
    match awaiting {
        // The command's own `--json` takes the input as JSON; the global `--json` (JSON
        // output) is written before the resource.
        Some("--json") => Err(
            "--json needs a value: after the command it is the input as JSON; for JSON output \
             write --json before the command (`agentic-org --json task list`)"
                .to_owned(),
        ),
        Some(flag) => Err(format!("{flag} needs a value")),
        None => Ok(scanned),
    }
}

/// How many leading words name a command: `who` is one, `task note <verb>` three,
/// `<resource> <verb>` two.
fn command_path_words(forwarded: &[String]) -> usize {
    match forwarded {
        [first, ..] if first == "who" => 1,
        [first, second, ..] if first == "task" && second == "note" => 3,
        _ => 2,
    }
}

/// How many leading words of `forwarded` name the command ([`command_path_words`], fewer when
/// the line is shorter). Words the run adds go right after them, before every flag and value
/// the caller wrote.
pub(super) fn command_path_len(forwarded: &[String]) -> usize {
    command_path_words(forwarded).min(forwarded.len())
}

/// Whether `forwarded` names a whole command: `task note` (no verb yet), `task` and an empty
/// line do not.
pub(super) fn names_a_command(forwarded: &[String]) -> bool {
    forwarded.len() >= command_path_words(forwarded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn selectors_and_global_flags_leave_and_values_stay() {
        let scanned = split(&words(&[
            "msg", "send", "--to", "#general", "--body", "-q", "--office", "O1", "-q", "--name",
            "lab", "--color", "never",
        ]))
        .unwrap();
        assert_eq!(
            scanned.forwarded,
            words(&["msg", "send", "--to", "#general", "--body", "-q"])
        );
        assert_eq!(scanned.office.as_deref(), Some("O1"));
        assert_eq!(scanned.instance.as_deref(), Some("lab"));
        assert!(scanned.quiet);
        assert_eq!(scanned.color.as_deref(), Some("never"));
    }

    #[test]
    fn a_command_flag_named_name_stays_with_the_command() {
        let scanned = split(&words(&[
            "team",
            "create",
            "--name",
            "Ops",
            "--instance",
            "lab",
        ]))
        .unwrap();
        assert_eq!(
            scanned.forwarded,
            words(&["team", "create", "--name", "Ops"])
        );
        assert_eq!(scanned.instance.as_deref(), Some("lab"));
        assert!(split(&words(&["team", "list", "--office", "a", "--office", "b"])).is_err());
        assert!(split(&words(&["team", "list", "--office"])).is_err());
    }

    /// P0-1: a value-taking flag at the end of the line is refused and named, so a word the
    /// run adds (`--dry-run`, `--format json`) can never become its value.
    #[test]
    fn a_trailing_flag_without_its_value_is_refused_by_name() {
        for (list, flag) in [
            (&["msg", "send", "--to", "ada", "--body"][..], "--body"),
            (&["task", "create", "--title"], "--title"),
            (&["task", "list", "--format"], "--format"),
            (&["task", "list", "--json"], "before the command"),
            (&["task", "create", "-q", "--title"], "--title"),
        ] {
            let error = split(&words(list)).unwrap_err();
            assert!(error.contains(flag), "{list:?}: {error}");
        }
        // A value that reads like a flag is still a value; `--name=value` needs nothing more.
        for list in [
            &["msg", "send", "--to", "ada", "--body", "--body"][..],
            &["task", "create", "--title=x"],
            &["msg", "send", "--to", "ada", "--", "--body"],
        ] {
            assert!(split(&words(list)).is_ok(), "{list:?}");
        }
    }

    #[test]
    fn the_command_path_is_one_two_or_three_words() {
        for (list, len) in [
            (&["who", "--format", "json"][..], 1),
            (&["task", "note", "add", "--task", "T"], 3),
            (&["task", "note"], 2),
            (&["msg", "send", "--to", "ada"], 2),
            (&["task"], 1),
            (&[], 0),
        ] {
            assert_eq!(command_path_len(&words(list)), len, "{list:?}");
        }
    }

    /// `task note` without its verb is not a command yet, so nothing is added after its path.
    #[test]
    fn a_command_path_without_its_verb_is_not_a_command() {
        for (list, whole) in [
            (&["who"][..], true),
            (&["msg", "send"], true),
            (&["task", "note", "add"], true),
            (&["task", "note"], false),
            (&["task"], false),
            (&[], false),
        ] {
            assert_eq!(names_a_command(&words(list)), whole, "{list:?}");
        }
    }

    /// B-02: a flag's value and the words after `--` are never taken for a flag.
    #[test]
    fn a_flag_value_or_a_word_after_the_terminator_is_not_a_flag() {
        let dry = ["--dry-run"];
        let help = ["--format", "--help", "-h", "--schema", "--version"];
        for (list, names, found) in [
            (
                &["msg", "send", "--to", "ada", "--body", "--dry-run"][..],
                &dry[..],
                false,
            ),
            (&["msg", "send", "--body", "x", "--dry-run"], &dry, true),
            (&["msg", "send", "--body=--dry-run"], &dry, false),
            (&["msg", "send", "--dry-run=x"], &dry, true),
            (
                &["msg", "send", "--to", "ada", "--body", "-h"],
                &help,
                false,
            ),
            (&["task", "create", "--title", "--help"], &help, false),
            (&["task", "create", "--title", "t", "--help"], &help, true),
            (&["task", "list", "--format", "yaml"], &help, true),
            (&["task", "list", "-h"], &help, true),
            (&["msg", "send", "--", "--dry-run"], &dry, false),
            (&["msg", "send", "--", "-h"], &help, false),
        ] {
            assert_eq!(has_flag(&words(list), names), found, "{list:?}");
        }
    }
}
