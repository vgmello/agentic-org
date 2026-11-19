//! What the host can observe of another process's confinement: the
//! no-new-privileges and seccomp lines of `/proc/<pid>/status`
//! (`fs/proc/array.c`, `task_seccomp`).

use std::fmt;

/// The value of one `Name:\tvalue` line of a `/proc/<pid>/status` text.
pub(super) fn field<'a>(status: &'a str, name: &str) -> Option<&'a str> {
    status.lines().find_map(|line| {
        let value = line.strip_prefix(name)?.strip_prefix(':')?;
        Some(value.trim())
    })
}

/// The seccomp mode of a process whose filters decide its system calls
/// (`SECCOMP_MODE_FILTER`).
const MODE_FILTER: u32 = 2;

/// What the status text does not show of a confined web process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Unconfined {
    /// The line is missing or is not a number.
    Unreadable(&'static str),
    /// `NoNewPrivs` is not 1.
    Privileges(u32),
    /// `Seccomp` is not 2: no filter decides the process's system calls.
    Mode(u32),
    /// The process has no more filters than the process that checks it, so it
    /// installed none of its own.
    Filters { child: u32, own: u32 },
}

impl fmt::Display for Unconfined {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(line) => {
                write!(f, "the web process's status has no readable {line} line")
            }
            Self::Privileges(value) => write!(
                f,
                "the web process has not set no-new-privileges (NoNewPrivs: {value})"
            ),
            Self::Mode(value) => write!(
                f,
                "the web process runs without a seccomp filter (Seccomp: {value})"
            ),
            Self::Filters { child, own } => write!(
                f,
                "the web process installed no seccomp filter of its own (it has {child}, \
                 the host has {own})"
            ),
        }
    }
}

fn number(status: &str, name: &'static str) -> Result<u32, Unconfined> {
    field(status, name)
        .and_then(|value| value.parse().ok())
        .ok_or(Unconfined::Unreadable(name))
}

/// Whether `child`, the status text of the web process, shows no-new-privileges and a
/// seccomp filter the process installed itself. A child inherits its parent's
/// filters, as under a container's own profile, so it must have at least one more
/// than `own_filters`, the count of the process that checks it. Pure.
pub(super) fn confined(child: &str, own_filters: u32) -> Result<(), Unconfined> {
    match number(child, "NoNewPrivs")? {
        1 => (),
        value => return Err(Unconfined::Privileges(value)),
    }
    match number(child, "Seccomp")? {
        MODE_FILTER => (),
        value => return Err(Unconfined::Mode(value)),
    }
    let filters = number(child, "Seccomp_filters")?;
    if filters == 0 || filters <= own_filters {
        return Err(Unconfined::Filters {
            child: filters,
            own: own_filters,
        });
    }
    Ok(())
}

/// The `Seccomp_filters` count of a status text.
pub(super) fn filters(status: &str) -> Result<u32, Unconfined> {
    number(status, "Seccomp_filters")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(privileges: &str, mode: &str, filters: &str) -> String {
        format!(
            "Name:\tagentic-org-web\nUmask:\t0077\nState:\tS (sleeping)\nThreads:\t5\n\
             NoNewPrivs:\t{privileges}\nSeccomp:\t{mode}\nSeccomp_filters:\t{filters}\n\
             Speculation_Store_Bypass:\tthread force mitigated\n"
        )
    }

    #[test]
    fn a_line_is_found_by_its_whole_name() {
        let text = status("1", "2", "1");
        assert_eq!(field(&text, "Seccomp"), Some("2"));
        assert_eq!(field(&text, "Seccomp_filters"), Some("1"));
        assert_eq!(field(&text, "NoNewPrivs"), Some("1"));
        assert_eq!(field(&text, "Name"), Some("agentic-org-web"));
        assert_eq!(field(&text, "Secc"), None);
        assert_eq!(field(&text, "Missing"), None);
    }

    #[test]
    fn a_process_with_no_new_privileges_and_its_own_filter_is_confined() {
        assert_eq!(confined(&status("1", "2", "1"), 0), Ok(()));
        // Under a container profile the host has one filter and the child two.
        assert_eq!(confined(&status("1", "2", "2"), 1), Ok(()));
        assert_eq!(confined(&status("1", "2", "5"), 1), Ok(()));
    }

    #[test]
    fn an_unconfined_process_is_refused_for_the_first_thing_it_lacks() {
        assert_eq!(
            confined(&status("0", "0", "0"), 0),
            Err(Unconfined::Privileges(0))
        );
        assert_eq!(
            confined(&status("1", "0", "0"), 0),
            Err(Unconfined::Mode(0))
        );
        // Strict mode is not a filter.
        assert_eq!(
            confined(&status("1", "1", "0"), 0),
            Err(Unconfined::Mode(1))
        );
        assert_eq!(
            confined(&status("1", "2", "0"), 0),
            Err(Unconfined::Filters { child: 0, own: 0 })
        );
    }

    #[test]
    fn a_filter_inherited_from_the_host_is_not_the_web_processs_own() {
        assert_eq!(
            confined(&status("1", "2", "1"), 1),
            Err(Unconfined::Filters { child: 1, own: 1 })
        );
        assert_eq!(
            confined(&status("1", "2", "2"), 3),
            Err(Unconfined::Filters { child: 2, own: 3 })
        );
    }

    #[test]
    fn a_missing_or_malformed_line_is_refused() {
        let text = status("1", "2", "1");
        for (line, name) in [
            ("NoNewPrivs:\t1\n", "NoNewPrivs"),
            ("Seccomp:\t2\n", "Seccomp"),
            ("Seccomp_filters:\t1\n", "Seccomp_filters"),
        ] {
            assert_eq!(
                confined(&text.replace(line, ""), 0),
                Err(Unconfined::Unreadable(name))
            );
        }
        assert_eq!(
            confined(&status("yes", "2", "1"), 0),
            Err(Unconfined::Unreadable("NoNewPrivs"))
        );
        assert_eq!(
            confined(&status("1", "-2", "1"), 0),
            Err(Unconfined::Unreadable("Seccomp"))
        );
        assert_eq!(confined("", 0), Err(Unconfined::Unreadable("NoNewPrivs")));
    }

    #[test]
    fn each_refusal_says_what_the_process_lacks() {
        let text = |status: &str, own| confined(status, own).unwrap_err().to_string();
        assert!(text(&status("0", "0", "0"), 0).contains("NoNewPrivs: 0"));
        assert!(text(&status("1", "0", "0"), 0).contains("Seccomp: 0"));
        assert!(text(&status("1", "2", "1"), 1).contains("no seccomp filter of its own"));
    }
}
