//! AX-7: every command's `--help` ends with its row's example, a command line a shell reads
//! back word for word and the parser reads as the example input.

use serde_json::{Map, Value};

use super::{run_recorded, OfficeFactory, StubSources, PROGRAM};
use crate::table::{Command, CommandTable};

/// `stdout`, the `--help` of `row`, ends with the row's example as one printable line that
/// [`shell_words`] reads back as the same words.
pub(super) fn assert_help_ends_with_example(row: &Command, name: &str, stdout: &str) {
    let example: Vec<String> = crate::help::example_args(row)
        .iter()
        .map(|word| crate::help::shell_word(word))
        .collect();
    let line = format!("{PROGRAM} {}", example.join(" "));
    assert!(
        stdout.ends_with(&format!("\nExample:\n  {line}\n")),
        "{name}: --help ends with its example: {stdout}"
    );
    assert_eq!(
        shell_words(&line[PROGRAM.len() + 1..]),
        crate::help::example_args(row),
        "{name}: the example reads back as the same words"
    );
    assert!(
        !line.chars().any(char::is_control),
        "{name}: the example is one printable line"
    );
}

/// The example `--help` shows is a command line the parser reads as the example `input`.
pub(super) fn assert_example_parses(
    table: &CommandTable,
    row: &Command,
    name: &str,
    input: &Map<String, Value>,
    make: OfficeFactory,
) {
    let (_, shown) = run_recorded(
        table,
        &crate::help::example_args(row),
        &mut StubSources::default(),
        make,
    );
    assert_eq!(
        shown.calls.first().map(|call| &call.input),
        Some(input),
        "{name}: the example in --help gives the example input"
    );
}

/// The words a POSIX shell reads from `line`, for the quoting `--help` writes: bare words and
/// single-quoted text, `'\''` being a quote inside.
pub fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' if started => {
                words.push(std::mem::take(&mut word));
                started = false;
            }
            ' ' => {}
            '\'' => {
                started = true;
                for inner in chars.by_ref() {
                    if inner == '\'' {
                        break;
                    }
                    word.push(inner);
                }
            }
            '\\' => {
                started = true;
                word.extend(chars.next());
            }
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}
