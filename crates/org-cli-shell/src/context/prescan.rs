//! The global output flags read from the raw arguments before the real parser runs, so that
//! even a usage error is reported in the requested form (`--json`) and color.

use std::ffi::OsString;

use super::{ColorMode, OutputFormat};

/// What [`prescan`] found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Prescan {
    pub json: bool,
    pub quiet: bool,
    /// The last valid `--format` value.
    pub format: Option<OutputFormat>,
    /// The last valid `--color` value.
    pub color: Option<ColorMode>,
    pub no_color: bool,
}

/// Scans `args` (`args[0]` is the program and is skipped) for `--json`, `-q`/`--quiet`,
/// `--format X`/`--format=X`, `--color X`/`--color=X` and `--no-color`, stopping at `--`.
/// Combined short flags (`-qv`) and abbreviations are not recognised; the real parser still
/// decides everything else.
pub fn prescan(args: &[OsString]) -> Prescan {
    let mut found = Prescan::default();
    let mut words = args.iter().skip(1).map(|arg| arg.to_str()).peekable();
    while let Some(word) = words.next() {
        match word {
            Some("--") => break,
            Some("--json") => found.json = true,
            Some("-q" | "--quiet") => found.quiet = true,
            Some("--no-color") => found.no_color = true,
            Some("--format") => {
                if let Some(Some(value)) = words.next_if(|next| !next.is_some_and(is_flag)) {
                    if let Some(fmt) = OutputFormat::parse(value) {
                        found.format = Some(fmt);
                    }
                }
            }
            Some("--color") => {
                // Like the parser, a following flag is not taken as the value.
                if let Some(Some(value)) = words.next_if(|next| !next.is_some_and(is_flag)) {
                    if let Some(mode) = ColorMode::from_flag(value) {
                        found.color = Some(mode);
                    }
                }
            }
            Some(other) => {
                if let Some(fmt) = other
                    .strip_prefix("--format=")
                    .and_then(OutputFormat::parse)
                {
                    found.format = Some(fmt);
                } else if let Some(mode) = other
                    .strip_prefix("--color=")
                    .and_then(ColorMode::from_flag)
                {
                    found.color = Some(mode);
                }
            }
            None => {}
        }
    }
    found
}

fn is_flag(word: &str) -> bool {
    word.starts_with('-') && word != "-"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(args: &[&str]) -> Prescan {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        prescan(&args)
    }

    #[test]
    fn finds_the_output_flags() {
        assert_eq!(scan(&["agentic-org"]), Prescan::default());
        assert_eq!(
            scan(&["agentic-org", "org", "ls", "--json", "-q", "--no-color"]),
            Prescan {
                json: true,
                quiet: true,
                format: None,
                color: None,
                no_color: true,
            }
        );
        assert_eq!(
            scan(&["agentic-org", "org", "ls", "--format", "yaml"]).format,
            Some(OutputFormat::Yaml)
        );
        assert_eq!(
            scan(&["agentic-org", "org", "ls", "--format=compact"]).format,
            Some(OutputFormat::Compact)
        );
        assert!(scan(&["agentic-org", "--quiet"]).quiet);
        assert_eq!(
            scan(&["agentic-org", "--color", "always"]).color,
            Some(ColorMode::Always)
        );
        assert_eq!(
            scan(&["agentic-org", "--color=never"]).color,
            Some(ColorMode::Never)
        );
        assert_eq!(
            scan(&["agentic-org", "--color=never", "--color", "auto"]).color,
            Some(ColorMode::Auto)
        );
    }

    #[test]
    fn skips_the_program_and_stops_at_double_dash() {
        assert!(!scan(&["--json"]).json);
        let after = scan(&["agentic-org", "run", "--", "--json", "-q", "--color=always"]);
        assert_eq!(after, Prescan::default());
    }

    #[test]
    fn ignores_bad_color_values() {
        assert_eq!(scan(&["agentic-org", "--color", "sometimes"]).color, None);
        assert_eq!(scan(&["agentic-org", "--color=", "--json"]).color, None);
        assert_eq!(scan(&["agentic-org", "--color"]).color, None);
        // A following flag is not a value.
        let flag = scan(&["agentic-org", "--color", "--json"]);
        assert!(flag.json && flag.color.is_none());
    }
}
