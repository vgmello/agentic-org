//! Strict shell command decoding and command-prefix matching for allow and deny rules.

pub(super) fn is_direct_non_shell_argv(argv: &[String]) -> bool {
    let Some(first) = argv.first() else {
        return false;
    };
    !first.is_empty()
        && !first.contains('=')
        && !is_env_assignment_word(first)
        && !is_shell_binary(first)
}

pub(super) fn shell_command_is_unambiguously_decoded(input: &serde_json::Value) -> bool {
    let Some(cmd_val) = extract_shell_command(input) else {
        return false;
    };
    match cmd_val {
        serde_json::Value::String(s) => {
            if let Some(outer) = parse_strict_single_command(s, false) {
                if is_shell_binary(&outer[0]) {
                    return outer.len() == 3
                        && is_shell_cmd_flag(&outer[1])
                        && parse_strict_single_command(&outer[2], true)
                            .is_some_and(|inner| is_direct_non_shell_argv(&inner));
                }
            }
            parse_strict_single_command(s, true).is_some_and(|argv| is_direct_non_shell_argv(&argv))
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return false;
            }
            let Some(argv): Option<Vec<String>> = items
                .iter()
                .map(|v| v.as_str().map(ToOwned::to_owned))
                .collect()
            else {
                return false;
            };
            if is_shell_binary(&argv[0]) {
                return argv.len() == 3
                    && is_shell_cmd_flag(&argv[1])
                    && parse_strict_single_command(&argv[2], true)
                        .is_some_and(|inner| is_direct_non_shell_argv(&inner));
            }
            is_direct_non_shell_argv(&argv) && !argv.iter().any(|arg| has_unsafe_shell_chars(arg))
        }
        _ => false,
    }
}

pub(super) fn extract_shell_command(input: &serde_json::Value) -> Option<&serde_json::Value> {
    let obj = input.as_object()?;
    let mut found: Option<&serde_json::Value> = None;
    for key in ["command", "cmd", "CommandLine"] {
        if let Some(val) = obj.get(key) {
            if !val.is_null() {
                if found.is_some() {
                    // Fail closed if multiple command keys are present in the same payload.
                    return None;
                }
                found = Some(val);
            }
        }
    }
    found
}

pub(super) fn is_shell_binary(bin: &str) -> bool {
    matches!(
        bin,
        "sh" | "/bin/sh" | "bash" | "/bin/bash" | "zsh" | "/bin/zsh"
    )
}

pub(super) fn is_shell_cmd_flag(flag: &str) -> bool {
    matches!(flag, "-c" | "-lc" | "-cl")
}

pub(super) fn has_unsafe_shell_chars(s: &str) -> bool {
    s.contains("$(")
        || s.chars().any(|c| {
            matches!(
                c,
                ';' | '&' | '|' | '`' | '>' | '<' | '\u{2028}' | '\u{2029}'
            ) || (c.is_ascii_control() && c != '\t')
        })
}

pub(super) fn is_safe_unquoted_shell_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '-' | '_' | '.' | '/' | ':' | '@' | ',' | '=' | '+' | '%')
}

pub(super) fn consume_strict_single_quote(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    current: &mut String,
    enforce_no_metachars: bool,
) -> bool {
    for qc in chars.by_ref() {
        if qc == '\'' {
            return true;
        }
        if enforce_no_metachars && matches!(qc, ';' | '&' | '|' | '`' | '>' | '<') {
            return false;
        }
        current.push(qc);
    }
    false
}

pub(super) fn consume_strict_double_quote(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    current: &mut String,
    enforce_no_metachars: bool,
) -> bool {
    for qc in chars.by_ref() {
        if qc == '"' {
            return true;
        }
        // Reject every expansion/escape/history character inside double quotes.
        if matches!(qc, '$' | '`' | '\\' | '!') {
            return false;
        }
        if enforce_no_metachars && matches!(qc, ';' | '&' | '|' | '>' | '<') {
            return false;
        }
        current.push(qc);
    }
    false
}

pub(super) fn has_disallowed_shell_control_char(raw: &str) -> bool {
    raw.chars()
        .any(|c| matches!(c, '\u{2028}' | '\u{2029}') || (c.is_ascii_control() && c != '\t'))
}

pub(super) fn step_strict_command_char(
    c: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    words: &mut Vec<String>,
    current: &mut String,
    in_word: &mut bool,
    enforce_no_metachars: bool,
) -> bool {
    match c {
        ' ' | '\t' => {
            if *in_word {
                words.push(std::mem::take(current));
                *in_word = false;
            }
            true
        }
        '\'' => {
            *in_word = true;
            consume_strict_single_quote(chars, current, enforce_no_metachars)
        }
        '"' => {
            *in_word = true;
            consume_strict_double_quote(chars, current, enforce_no_metachars)
        }
        other if is_safe_unquoted_shell_char(other) => {
            *in_word = true;
            current.push(other);
            true
        }
        _ => false,
    }
}

/// Parses one shell command string strictly into literal argv words, rejecting any shell
/// operator, redirection, command/process/parameter/arithmetic substitution (including zsh
/// `=(...)` temporary-file substitution and `*(e:...:)` glob qualifiers), backslash escape,
/// variable expansion, or unterminated quote.
pub(super) fn parse_strict_single_command(
    raw: &str,
    enforce_no_metachars: bool,
) -> Option<Vec<String>> {
    if has_disallowed_shell_control_char(raw)
        || (enforce_no_metachars && has_unsafe_shell_chars(raw))
    {
        return None;
    }

    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        if !step_strict_command_char(
            c,
            &mut chars,
            &mut words,
            &mut current,
            &mut in_word,
            enforce_no_metachars,
        ) {
            return None;
        }
    }

    if in_word {
        words.push(current);
    }
    (!words.is_empty()).then_some(words)
}

pub(super) fn argv_matches_prefix(argv: &[String], prefix: &str) -> bool {
    let prefix_words: Vec<&str> = prefix
        .split([' ', '\t'])
        .filter(|s| !s.is_empty())
        .collect();
    if prefix_words.is_empty() {
        return false;
    }
    let requires_extra_arg = prefix.ends_with([' ', '\t']);
    if requires_extra_arg {
        if argv.len() <= prefix_words.len() {
            return false;
        }
    } else if argv.len() < prefix_words.len() {
        return false;
    }
    argv.iter()
        .zip(prefix_words.iter())
        .all(|(arg, expected)| arg == expected)
}

pub(super) fn inner_command_str_matches_safe_prefix(raw: &str, prefix: &str) -> bool {
    let Some(argv) = parse_strict_single_command(raw, true) else {
        return false;
    };
    if is_shell_binary(&argv[0]) {
        return false;
    }
    argv_matches_prefix(&argv, prefix)
}

pub(super) fn command_str_matches_safe_prefix(raw: &str, prefix: &str) -> bool {
    // Allow at most one native shell-wrapper layer (e.g. Codex 0.169.0+ string-serialized
    // `"/bin/zsh -lc 'agent-os inbox'"`), then enforce strict literal-only parsing on the inner
    // invocation.
    if let Some(outer) = parse_strict_single_command(raw, false) {
        if is_shell_binary(&outer[0]) {
            return outer.len() == 3
                && is_shell_cmd_flag(&outer[1])
                && inner_command_str_matches_safe_prefix(&outer[2], prefix);
        }
    }
    inner_command_str_matches_safe_prefix(raw, prefix)
}

pub(super) fn command_value_matches_safe_prefix(value: &serde_json::Value, prefix: &str) -> bool {
    match value {
        serde_json::Value::String(s) => command_str_matches_safe_prefix(s, prefix),
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return false;
            }
            let Some(argv): Option<Vec<String>> = items
                .iter()
                .map(|v| v.as_str().map(ToOwned::to_owned))
                .collect()
            else {
                return false;
            };
            if is_shell_binary(&argv[0]) {
                return argv.len() == 3
                    && is_shell_cmd_flag(&argv[1])
                    && inner_command_str_matches_safe_prefix(&argv[2], prefix);
            }
            if argv[0].is_empty() || argv.iter().any(|arg| has_unsafe_shell_chars(arg)) {
                return false;
            }
            argv_matches_prefix(&argv, prefix)
        }
        _ => false,
    }
}

pub(super) fn is_valid_env_assignment_lhs(lhs: &str) -> bool {
    let name = lhs.strip_suffix('+').unwrap_or(lhs);
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(super) fn is_env_assignment_word(word: &str) -> bool {
    word.split_once('=')
        .is_some_and(|(lhs, _)| is_valid_env_assignment_lhs(lhs))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellQuoteState {
    Unquoted,
    Single,
    Double,
}

pub(super) fn push_quoted_segment_char(
    state: &mut ShellQuoteState,
    ch: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    current: &mut String,
) -> Option<()> {
    if *state == ShellQuoteState::Single {
        if ch == '\'' {
            *state = ShellQuoteState::Unquoted;
        }
        current.push(ch);
        return Some(());
    }
    if ch == '\\' {
        let next = chars.next()?;
        if next == '\n' {
            // POSIX/Bash line continuation inside double quotes removes both `\` and LF.
            return Some(());
        }
        current.push(ch);
        current.push(next);
        return Some(());
    }
    if ch == '"' {
        *state = ShellQuoteState::Unquoted;
    }
    current.push(ch);
    Some(())
}

pub(super) fn skip_comment_until_lf(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    // POSIX/Bash comment runs until LF (`\n`); a bare CR (`\r`) stays inside the comment.
    chars.by_ref().any(|cc| cc == '\n')
}

pub(super) fn split_unquoted_shell_segments(raw: &str) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut quote = ShellQuoteState::Unquoted;
    let mut at_word_start = true;
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        if quote != ShellQuoteState::Unquoted {
            push_quoted_segment_char(&mut quote, ch, &mut chars, &mut current)?;
            at_word_start = false;
            continue;
        }
        match ch {
            '#' if at_word_start => {
                if skip_comment_until_lf(&mut chars) {
                    if !current.is_empty() {
                        segments.push(std::mem::take(&mut current));
                    }
                    at_word_start = true;
                }
            }
            '\\' => {
                let next = chars.next()?;
                if next != '\n' {
                    current.push(ch);
                    current.push(next);
                    at_word_start = false;
                }
            }
            '\'' => {
                quote = ShellQuoteState::Single;
                current.push(ch);
                at_word_start = false;
            }
            '"' => {
                quote = ShellQuoteState::Double;
                current.push(ch);
                at_word_start = false;
            }
            ' ' | '\t' => {
                current.push(ch);
                at_word_start = true;
            }
            ';' | '&' | '|' | '\n' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                at_word_start = true;
            }
            _ => {
                current.push(ch);
                at_word_start = false;
            }
        }
    }
    if quote != ShellQuoteState::Unquoted {
        return None;
    }
    if !current.is_empty() {
        segments.push(current);
    }
    Some(segments)
}

pub(super) fn push_quoted_word_char(
    state: &mut ShellQuoteState,
    ch: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    current: &mut String,
) -> Option<()> {
    if *state == ShellQuoteState::Single {
        if ch == '\'' {
            *state = ShellQuoteState::Unquoted;
        } else {
            current.push(ch);
        }
        return Some(());
    }
    if ch == '\\' {
        let next = chars.next()?;
        if next == '\n' {
            return Some(());
        }
        if !matches!(next, '"' | '\\' | '$' | '`') {
            current.push('\\');
        }
        current.push(next);
        return Some(());
    }
    if ch == '"' {
        *state = ShellQuoteState::Unquoted;
    } else {
        current.push(ch);
    }
    Some(())
}

pub(super) fn parse_shell_segment_words(segment: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote = ShellQuoteState::Unquoted;
    let mut chars = segment.chars().peekable();

    while let Some(ch) = chars.next() {
        if quote != ShellQuoteState::Unquoted {
            push_quoted_word_char(&mut quote, ch, &mut chars, &mut current)?;
            continue;
        }
        match ch {
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            '\\' => {
                let next = chars.next()?;
                if next != '\n' {
                    in_word = true;
                    current.push(next);
                }
            }
            '\'' => {
                in_word = true;
                quote = ShellQuoteState::Single;
            }
            '"' => {
                in_word = true;
                quote = ShellQuoteState::Double;
            }
            _ => {
                in_word = true;
                current.push(ch);
            }
        }
    }
    if quote != ShellQuoteState::Unquoted {
        return None;
    }
    if in_word {
        words.push(current);
    }
    (!words.is_empty()).then_some(words)
}

pub(super) fn decoded_argv_matches_deny_prefix(argv: &[String], prefix: &str) -> bool {
    let start = argv
        .iter()
        .position(|w| !is_env_assignment_word(w))
        .unwrap_or(argv.len());
    let cmd = &argv[start..];
    if cmd.is_empty() {
        return false;
    }
    if cmd.join(" ").trim_start().starts_with(prefix) {
        return true;
    }
    argv_matches_prefix(cmd, prefix)
}

pub(super) fn advance_quoted_raw_char(
    state: &mut ShellQuoteState,
    i: usize,
    ch: char,
    iter: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Option<usize> {
    if *state == ShellQuoteState::Single {
        if ch == '\'' {
            *state = ShellQuoteState::Unquoted;
        }
        return Some(i + ch.len_utf8());
    }
    if ch == '\\' {
        let (ni, nc) = iter.next()?;
        return Some(ni + nc.len_utf8());
    }
    if ch == '"' {
        *state = ShellQuoteState::Unquoted;
    }
    Some(i + ch.len_utf8())
}

pub(super) fn scan_leading_raw_assignment(rest: &str) -> Option<usize> {
    let mut idx = 0;
    let mut eq_pos = None;
    let mut quote = ShellQuoteState::Unquoted;
    let mut iter = rest.char_indices().peekable();
    while let Some((i, ch)) = iter.next() {
        if quote != ShellQuoteState::Unquoted {
            idx = advance_quoted_raw_char(&mut quote, i, ch, &mut iter)?;
            continue;
        }
        match ch {
            ' ' | '\t' => break,
            '\\' => {
                let (ni, nc) = iter.next()?;
                idx = ni + nc.len_utf8();
            }
            '\'' => {
                quote = ShellQuoteState::Single;
                idx = i + 1;
            }
            '"' => {
                quote = ShellQuoteState::Double;
                idx = i + 1;
            }
            '=' if eq_pos.is_none() => {
                eq_pos = Some(i);
                idx = i + 1;
            }
            _ => idx = i + ch.len_utf8(),
        }
    }
    if quote != ShellQuoteState::Unquoted {
        return None;
    }
    let eq = eq_pos?;
    is_valid_env_assignment_lhs(&rest[..eq]).then_some(idx)
}

pub(super) fn strip_leading_env_assignments_raw(mut rest: &str) -> &str {
    loop {
        rest = rest.trim_start_matches([' ', '\t']);
        let Some(end_idx) = scan_leading_raw_assignment(rest) else {
            return rest;
        };
        rest = &rest[end_idx..];
    }
}

pub(super) fn decoded_str_matches_deny_prefix(raw: &str, prefix: &str) -> bool {
    let Some(segments) = split_unquoted_shell_segments(raw) else {
        // Undecodable/unbalanced command syntax: do not fabricate false segments;
        // the fail-closed `has_command_prefix_deny` guard escalates it to the owner.
        return false;
    };
    for seg in segments {
        // Check both the raw segment and the raw span after leading `NAME=VALUE` env assignments
        // before quote stripping so literal quoted CommandPrefix rules (e.g. `'rm' -rf` or
        // `"agent-os"`) match verbatim even when preceded by environment assignments.
        let raw_trimmed = seg.trim_start_matches([' ', '\t']);
        if raw_trimmed.starts_with(prefix)
            || strip_leading_env_assignments_raw(raw_trimmed).starts_with(prefix)
        {
            return true;
        }
        let Some(words) = parse_shell_segment_words(&seg) else {
            continue;
        };
        let start = words
            .iter()
            .position(|w| !is_env_assignment_word(w))
            .unwrap_or(words.len());
        let cmd = &words[start..];
        if cmd.len() >= 3
            && is_shell_binary(&cmd[0])
            && is_shell_cmd_flag(&cmd[1])
            && decoded_str_matches_deny_prefix(&cmd[2], prefix)
        {
            return true;
        }
        if decoded_argv_matches_deny_prefix(cmd, prefix) {
            return true;
        }
    }
    false
}
