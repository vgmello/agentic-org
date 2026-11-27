//! How setup reports a failed step and writes a line of its output.

use std::io::Write;

pub(super) fn fail(context: &str, why: impl std::fmt::Display) -> String {
    format!("{context}: {why}")
}

pub(super) fn say(out: &mut dyn Write, line: impl std::fmt::Display) -> Result<(), String> {
    writeln!(out, "{line}").map_err(|e| fail("writing output", e))
}
