//! The unit file: what the grammar cannot carry, its rendering with every value quoted, and
//! the unit read back from an exact rendering.

use std::ffi::{OsStr, OsString};

use crate::service::{ServiceError, Unit};

/// What a definition this adapter wrote says: the unit it is a rendering of. `None` when
/// the bytes are not exactly the rendering of a unit under `name`: a file someone wrote or
/// changed, whose meaning the adapter does not claim to know. The parse only has to find
/// the values; the comparison of the bytes with a new rendering is what makes it exact.
pub(super) fn definition(name: &str, bytes: &[u8]) -> Option<Unit> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut exec = None;
    let mut env = Vec::new();
    for line in text.lines() {
        if let Some(words) = line.strip_prefix("ExecStart=") {
            exec = Some(unquoted_words(words, true)?);
        } else if let Some(word) = line.strip_prefix("Environment=") {
            let mut words = unquoted_words(word, false)?.into_iter();
            let assignment = words.next().filter(|_| words.next().is_none())?;
            let (key, value) = assignment.split_once('=')?;
            env.push((key.into(), value.into()));
        }
    }
    let mut exec = exec?.into_iter();
    let unit = Unit {
        instance_id: String::new(),
        program: exec.next()?.into(),
        args: exec.map(OsString::from).collect(),
        env,
    };
    (render(name, &unit) == bytes).then_some(unit)
}

/// The words of a line written by [`quote`], with its escapes undone: `%%` is `%` and, in
/// `ExecStart=` (`exec`), `$$` is `$`. `None` for anything [`quote`] does not write.
fn unquoted_words(line: &str, exec: bool) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut chars = line.chars();
    loop {
        if chars.next()? != '"' {
            return None;
        }
        let mut word = String::new();
        loop {
            match chars.next()? {
                '"' => break,
                '\\' => word.push(match chars.next()? {
                    '\\' => '\\',
                    '"' => '"',
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    _ => return None,
                }),
                '%' => (chars.next()? == '%').then(|| word.push('%'))?,
                '$' if exec => (chars.next()? == '$').then(|| word.push('$'))?,
                c => word.push(c),
            }
        }
        words.push(word);
        match chars.next() {
            None => return Some(words),
            Some(' ') => {}
            Some(_) => return None,
        }
    }
}

/// The location variables (`crate::dirs::location_variables`) that `unit` does not set:
/// its definition unsets them, so the user manager's own values never reach the service.
pub(super) fn unset_variables(unit: &Unit) -> Vec<&'static str> {
    crate::dirs::location_variables()
        .unwrap_or(&[])
        .iter()
        .copied()
        .filter(|name| !unit.env.iter().any(|(key, _)| key == OsStr::new(name)))
        .collect()
}

/// What the unit-file grammar cannot carry safely, or what `systemctl show` cannot report
/// back for the comparison of what systemd would run, beyond the contract's `validate`:
///
/// - an environment name that is not an identifier (`[A-Za-z_][A-Za-z0-9_]*`): systemd
///   drops such an assignment with a warning, and the service would start without it;
/// - a control character (tab, line feed and carriage return included) in the program,
///   an argument or an environment value: systemd prints an argument raw, which breaks its
///   report into lines, and some systemd versions drop an assignment that holds one;
/// - a `$` in the program path: systemd expands variables in arguments, where `$$` writes a
///   literal `$`, but not in the path it executes, so no one spelling is right for both.
pub(super) fn unit_file_safe(unit: &Unit) -> Result<(), ServiceError> {
    if unit.program.to_string_lossy().contains('$') {
        return Err(ServiceError::Invalid(
            "a systemd unit cannot name a program whose path contains a dollar sign",
        ));
    }
    let control = |text: &OsStr| text.to_string_lossy().chars().any(char::is_control);
    if control(unit.program.as_os_str()) || unit.args.iter().any(|arg| control(arg)) {
        return Err(ServiceError::Invalid(
            "a systemd unit cannot run a program or argument with a control character",
        ));
    }
    for (key, value) in &unit.env {
        let key = key.to_string_lossy();
        let identifier = key
            .bytes()
            .enumerate()
            .all(|(at, b)| b == b'_' || b.is_ascii_alphabetic() || (at > 0 && b.is_ascii_digit()));
        if key.is_empty() || !identifier {
            return Err(ServiceError::Invalid(
                "a systemd unit cannot set an environment variable whose name is not an \
                 identifier",
            ));
        }
        if control(value) {
            return Err(ServiceError::Invalid(
                "a systemd unit cannot set an environment value with a control character",
            ));
        }
    }
    Ok(())
}

/// One double-quoted word of a unit file. Inside the quotes systemd undoes C escapes, so
/// backslash, quote and the three line and field characters are written as escapes and no
/// line ever ends in a backslash; `%` begins a specifier, so it is doubled; and in
/// `ExecStart=` (`exec`) `$` begins a variable, so it is doubled too. In `Environment=` a
/// `$` is literal. A quoted `;` is an argument, not a command separator.
pub(super) fn quote(text: &str, exec: bool) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '%' => out.push_str("%%"),
            '$' if exec => out.push_str("$$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The unit file. Inputs were validated as UTF-8 by `validate` and by [`unit_file_safe`].
pub(super) fn render(name: &str, unit: &Unit) -> Vec<u8> {
    let text = |s: &OsStr, exec: bool| quote(&s.to_string_lossy(), exec);
    let mut u = String::from(
        "# Written by agentic-org setup. A changed file is refused, never overwritten.\n[Unit]\n",
    );
    u.push_str(&format!(
        "Description=Agentic Org instance ({})\n\n",
        name.replace('%', "%%")
    ));
    u.push_str("[Service]\nType=exec\nExecStart=");
    u.push_str(&text(unit.program.as_os_str(), true));
    for arg in &unit.args {
        u.push(' ');
        u.push_str(&text(arg, true));
    }
    u.push('\n');
    for (key, value) in &unit.env {
        let assignment = format!("{}={}", key.to_string_lossy(), value.to_string_lossy());
        u.push_str(&format!("Environment={}\n", quote(&assignment, false)));
    }
    let unset = unset_variables(unit);
    if !unset.is_empty() {
        u.push_str(&format!("UnsetEnvironment={}\n", unset.join(" ")));
    }
    u.push_str(concat!(
        "KillMode=mixed\n",
        "TimeoutStopSec=15\n",
        "Restart=on-failure\n",
        "UMask=0077\n",
        "NoNewPrivileges=yes\n",
        "\n[Install]\nWantedBy=default.target\n",
    ));
    u.into_bytes()
}
