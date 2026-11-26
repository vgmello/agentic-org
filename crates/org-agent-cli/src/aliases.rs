//! The shorthands `agent-os` accepts, declared once on the command table (`org_command::Alias`):
//! `who` is `directory list`, `task note <verb>` is `task-note <verb>`, and `boot` and
//! `instructions overview` are `session boot`. The table rewrites them before it reads any
//! line, so a shorthand is the same command for a call, its `--help` and `--schema`, every line
//! of `session attach`, and the drift guard of the built-in text.

use std::borrow::Cow;

use org_command::Alias;

/// Every shorthand, in the order the catalogue lists them.
pub const ALIASES: &[Alias] = &[
    Alias {
        words: &["boot"],
        command: &["session", "boot"],
    },
    Alias {
        words: &["instructions", "overview"],
        command: &["session", "boot"],
    },
    Alias {
        words: &["task", "note"],
        command: &["task-note"],
    },
    Alias {
        words: &["who"],
        command: &["directory", "list"],
    },
];

/// `args` with a leading shorthand replaced by the command it stands for; anything else as it is.
pub fn normalize_args(args: &[String]) -> Cow<'_, [String]> {
    crate::table::runtime_table().expand(args)
}

#[cfg(test)]
mod tests {
    use super::normalize_args;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn each_shorthand_becomes_its_command_and_keeps_the_rest_of_the_line() {
        for (line, expected) in [
            ("who --team Core", "directory list --team Core"),
            ("boot --format yaml", "session boot --format yaml"),
            ("instructions overview --offline", "session boot --offline"),
            ("task note add --task T-A1", "task-note add --task T-A1"),
            ("who --help", "directory list --help"),
            ("boot --schema", "session boot --schema"),
        ] {
            assert_eq!(
                normalize_args(&words(line)).as_ref(),
                words(expected),
                "{line}"
            );
        }
    }

    #[test]
    fn anything_else_is_left_as_it_is() {
        for line in [
            "task list",
            "session boot",
            "instructions",
            "task notes",
            "msg send --to who --body boot",
            "",
        ] {
            let args = words(line);
            assert_eq!(normalize_args(&args).as_ref(), args.as_slice(), "{line}");
        }
    }
}
