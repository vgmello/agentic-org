//! The file-size ratchet (TASK-178, `.agents/rules/rust-security-and-modularity.md`).
//!
//! A production Rust file is split to 800 lines or fewer and never passes 1,000. This
//! test reads every `.rs` file of the repository, counts the lines of each production
//! file, and compares them with `.github/large-files.allow`, which lists, each with its
//! line count, the production files that were over the cap when the check was written.
//! The test fails when:
//!
//! - a production file that is not listed has more than 1,000 lines;
//! - a listed file has more lines than its entry (a listed file never grows);
//! - a listed file has fewer lines than its entry but still more than 1,000 (the entry
//!   is lowered in the same change, so every line won stays won);
//! - a listed file has 1,000 lines or fewer, or is gone (its line is deleted in the
//!   same change, so the list always states what is left);
//! - the list breaks its format: it opens with a `#` comment, and each entry is
//!   `path lines`, written exactly so, sorted by path, listed once, with a production
//!   `.rs` path and a count over 1,000;
//! - git tracks a `.rs` file that exists and the walk did not read (a hidden directory,
//!   `node_modules`, a directory holding `CACHEDIR.TAG`, a nested checkout), or the
//!   checkout has `.git` and git cannot list what it tracks.
//!
//! Production code is every `.rs` file but dedicated test code: a file or directory
//! named `tests`, `testing`, `testkit` or `test_support`, or ending in `_tests` or
//! `-tests` (Cargo `tests/` targets, `src/**/tests.rs`, `wake_tests.rs`,
//! `web_process_tests/`, `setup-tests/`, the `#[cfg(test)]` and feature-gated test
//! kits). Everything else counts: `build.rs`, examples, benchmarks, and test modules
//! under other names. A file's lines are every line of it, as `wc -l` counts them plus
//! a last line without a newline, with no parsing: an inline `#[cfg(test)] mod tests`
//! counts, and moves to its own file (`foo/tests.rs`, or `foo_tests.rs` through a
//! `#[path]` attribute) when `foo.rs` is too long.
//!
//! CI also checks that a change never raises a count, and adds a path only for a listed
//! file renamed (at no more than its old count). The ignored test at the end compares
//! the list with a base commit:
//! `FILE_SIZE_RATCHET_BASE=$(git merge-base HEAD origin/v1) cargo test -p org-platform --test file_size_ratchet -- --include-ignored`
//!
//! To see what the check finds, in the list's format:
//! `FILE_SIZE_RATCHET_PRINT=1 cargo test -p org-platform --test file_size_ratchet -- --nocapture`

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ALLOWLIST: &str = ".github/large-files.allow";
const RULES: &str = ".agents/rules/rust-security-and-modularity.md";
/// The most lines a production file that is not listed may have.
const CAP: usize = 1_000;
/// What a production file over the cap is split down to.
const TARGET: usize = 800;
/// File and directory names, without `.rs`, that hold only test code.
const TEST_NAMES: [&str; 4] = ["tests", "testing", "testkit", "test_support"];
/// Endings of file and directory names, without `.rs`, that hold only test code.
const TEST_SUFFIXES: [&str; 2] = ["_tests", "-tests"];
/// A build output directory, wherever it is and whatever it is called, has this file.
const BUILD_OUTPUT_MARKER: &str = "CACHEDIR.TAG";
/// The commit the ignored test compares the list with.
const BASE_VARIABLE: &str = "FILE_SIZE_RATCHET_BASE";
/// The variables that point git at a repository, its index or its objects: what
/// `git rev-parse --local-env-vars` lists, less the configuration ones (a sandbox may
/// pass `safe.directory` through them).
const REPOSITORY_VARIABLES: [&str; 12] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
];

/// Whether `path` (relative, `/` separators) is dedicated test code, which the cap does
/// not count: one of its directories, or the file itself without `.rs`, has a test name.
fn is_test_code(path: &str) -> bool {
    path.strip_suffix(".rs")
        .unwrap_or(path)
        .split('/')
        .any(|name| {
            TEST_NAMES.contains(&name) || TEST_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
        })
}

/// The lines `wc -l` counts, plus a last line that has no newline.
fn lines(bytes: &[u8]) -> usize {
    let newlines = bytes.iter().filter(|&&byte| byte == b'\n').count();
    newlines + usize::from(bytes.last().is_some_and(|&byte| byte != b'\n'))
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/org-platform is two levels below the repository root")
        .to_path_buf()
}

/// `git -C root`, without the calling environment's repository variables. A test run
/// from a git hook inherits `GIT_DIR` and `GIT_INDEX_FILE`, which would point every
/// command, those on a temporary repository included, at the calling repository and its
/// index.
fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(root);
    for name in REPOSITORY_VARIABLES {
        command.env_remove(name);
    }
    command
}

/// Every `.rs` file under `root`, as a path relative to the root with `/` separators,
/// sorted. Skipped: hidden directories, `node_modules`, build output (a directory
/// holding `CACHEDIR.TAG`, as every cargo target directory does), so a source directory
/// that happens to be named `target` is read, and a checkout nested in this one (a
/// directory holding `.git`), which git does not track here.
fn sources(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        for entry in entries {
            let entry = entry.expect("a directory entry");
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type().expect("a file type").is_dir() {
                let skipped = name.starts_with('.')
                    || name == "node_modules"
                    || path.join(BUILD_OUTPUT_MARKER).exists()
                    || path.join(".git").exists();
                if !skipped {
                    pending.push(path);
                }
            } else if name.ends_with(".rs") && path.is_file() {
                // `is_file` follows a symbolic link, so a linked source file is read.
                let relative = path.strip_prefix(root).expect("under the root");
                found.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
}

/// The `.rs` files git tracks under `root` that exist; `None` where there is no git
/// checkout to ask (a source archive, without `.git`). A checkout git cannot list is an
/// error, never a skipped rule.
fn tracked(root: &Path) -> Result<Option<Vec<String>>, String> {
    if !root.join(".git").exists() {
        return Ok(None);
    }
    let listed = git(root)
        .args(["ls-files", "-z", "--", "*.rs"])
        .output()
        .map_err(|error| format!("git ls-files: {error}"))?;
    if !listed.status.success() {
        return Err(format!(
            "git ls-files: {}",
            String::from_utf8_lossy(&listed.stderr).trim()
        ));
    }
    Ok(Some(
        String::from_utf8_lossy(&listed.stdout)
            .split('\0')
            // A file renamed or deleted without staging is still listed.
            .filter(|path| !path.is_empty() && root.join(path).is_file())
            .map(str::to_owned)
            .collect(),
    ))
}

/// Tracked sources the walk did not read, so the check did not measure them.
fn unread(tracked: &[String], read: &[String]) -> Vec<String> {
    tracked
        .iter()
        .filter(|path| read.binary_search(path).is_err())
        .map(|path| {
            format!(
                "{path}: tracked, but in a directory the check does not read (hidden, \
                 node_modules, build output or a nested checkout). Move it, or delete the \
                 stray `.git`."
            )
        })
        .collect()
}

/// Each production file among `read`, with its lines.
fn measure(root: &Path, read: &[String]) -> BTreeMap<String, usize> {
    read.iter()
        .filter(|path| !is_test_code(path))
        .map(|path| {
            let bytes = fs::read(root.join(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
            (path.clone(), lines(&bytes))
        })
        .collect()
}

/// The list's entries. It opens with a `#` comment; blank lines and lines starting
/// with `#` are ignored; every other line is an entry, sorted by path, listed once.
fn allowlist(text: &str) -> Result<BTreeMap<String, usize>, String> {
    if !text.starts_with('#') {
        return Err(format!(
            "{ALLOWLIST}:1: the list opens with a `#` comment that says what it holds and \
             that it only shrinks"
        ));
    }
    let mut listed = BTreeMap::new();
    let mut previous: Option<&str> = None;
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = format!("{ALLOWLIST}:{}", index + 1);
        let (path, count) = entry(line).map_err(|problem| format!("{at}: {problem}"))?;
        if previous.is_some_and(|previous| previous >= path) {
            return Err(format!("{at}: entries are sorted by path and listed once"));
        }
        previous = Some(path);
        listed.insert(path.to_owned(), count);
    }
    Ok(listed)
}

/// One entry: a production `.rs` path relative to the repository root, one space, and
/// its line count over the cap, in decimal without a sign or leading zero.
fn entry(line: &str) -> Result<(&str, usize), String> {
    let malformed = || {
        format!(
            "expected `path lines`: a relative .rs path, one space, and a line count over \
             {CAP}, with nothing else on the line"
        )
    };
    let (path, count_text) = line.split_once(' ').ok_or_else(malformed)?;
    let count: usize = count_text.parse().map_err(|_| malformed())?;
    if count.to_string() != count_text {
        return Err(malformed());
    }
    let plain = path.ends_with(".rs")
        && !path.contains(|character: char| character.is_whitespace() || character == '\\')
        && path
            .split('/')
            .all(|name| !name.is_empty() && name != "." && name != "..");
    if !plain {
        return Err(format!(
            "{path}: not a plain relative .rs path (no leading `/`, `./`, `..`, empty \
             part or backslash)"
        ));
    }
    if is_test_code(path) {
        return Err(format!(
            "{path}: test code, which the cap does not count. Delete the line."
        ));
    }
    if count <= CAP {
        return Err(format!(
            "{path}: a count of {count} is within the {CAP}-line cap. Delete the line."
        ));
    }
    Ok((path, count))
}

/// What is wrong, one line each; empty when the production files and the list agree.
fn compare(counted: &BTreeMap<String, usize>, listed: &BTreeMap<String, usize>) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, &lines) in counted {
        match listed.get(path) {
            None if lines > CAP => problems.push(format!(
                "{path}: {lines} lines, over the {CAP}-line cap for production code. Split it \
                 by responsibility to {TARGET} lines or fewer ({RULES}); the list only \
                 shrinks, so a file is not added to it."
            )),
            Some(&ceiling) if lines > ceiling => problems.push(format!(
                "{path}: {lines} lines, and its entry allows {ceiling}. A listed file never \
                 grows: move code out of it first ({RULES})."
            )),
            Some(_) if lines <= CAP => problems.push(format!(
                "{path}: {lines} lines, within the {CAP}-line cap. Delete its line from \
                 {ALLOWLIST}."
            )),
            Some(&ceiling) if lines < ceiling => problems.push(format!(
                "{path}: {lines} lines, and its entry says {ceiling}. Lower the entry in \
                 {ALLOWLIST} to {lines}, so the file never grows back."
            )),
            _ => {}
        }
    }
    for path in listed.keys() {
        if !counted.contains_key(path) {
            problems.push(format!(
                "{path}: no such production source. Delete its line from {ALLOWLIST} (a \
                 renamed file is listed under its new path; split files stay at or under {CAP} \
                 lines)."
            ));
        }
    }
    problems
}

/// The production files over the cap, in the list's format.
fn over_the_cap(counted: &BTreeMap<String, usize>) -> String {
    let mut text = String::new();
    for (path, lines) in counted.iter().filter(|&(_, &lines)| lines > CAP) {
        writeln!(text, "{path} {lines}").expect("a string");
    }
    text
}

/// What a change did to the list since `before`, one line per problem. No count rises,
/// and a path is added only as a listed file renamed: each added path takes the place of
/// a distinct removed entry whose file is gone (`exists` says no) and whose count was at
/// least its own. Lines won in a file that is still there never pay for a new entry, so
/// a piece split from a listed file is not listed: each piece is within the cap. What
/// the check cannot tell from a rename (the old path gone, most of it moved to one new
/// path) is left to review; a swap of two listed paths is done in two changes.
fn shrinks(
    before: &BTreeMap<String, usize>,
    after: &BTreeMap<String, usize>,
    exists: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut problems = Vec::new();
    let mut added = Vec::new();
    for (path, &count) in after {
        match before.get(path) {
            None => added.push((count, path)),
            Some(&was) if count > was => {
                problems.push(format!("{path}: count raised from {was} to {count}"));
            }
            Some(_) => {}
        }
    }
    let mut gone: Vec<usize> = before
        .iter()
        .filter(|(path, _)| !after.contains_key(*path) && !exists(path))
        .map(|(_, &count)| count)
        .collect();
    // Largest first, each added path takes the longest gone entry left when that one is
    // at least as long: no other pairing renames more of them.
    added.sort_unstable_by(|left, right| right.cmp(left));
    gone.sort_unstable_by(|left, right| right.cmp(left));
    let mut next = gone.iter().peekable();
    for (count, path) in added {
        if next.next_if(|&&was| was >= count).is_none() {
            problems.push(format!(
                "{path}: added to the list ({count}), and no entry of {count} lines or more \
                 whose file is gone makes it a rename"
            ));
        }
    }
    problems
}

/// The entries of the list at the base, read leniently (`path count` lines; blank lines
/// and `#` comments ignored), so a later change to the list's format or to what counts
/// as test code does not fail every branch.
fn base_entries(text: &str) -> Result<BTreeMap<String, usize>, String> {
    let mut entries = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = line
            .rsplit_once(' ')
            .and_then(|(path, count)| Some((path.trim(), count.parse::<usize>().ok()?)));
        let Some((path, count)) = entry else {
            return Err(format!("{ALLOWLIST}:{}: not `path count`", index + 1));
        };
        entries.insert(path.to_owned(), count);
    }
    Ok(entries)
}

/// The list as it was at `base`; `None` when the commit exists without it. Any other
/// failure to read it is an error, never a pass.
fn list_at(root: &Path, base: &str) -> Result<Option<String>, String> {
    // A hexadecimal id only, so the value can never reach git as an option.
    let id = (7..=64).contains(&base.len())
        && base
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !id {
        return Err(format!(
            "{BASE_VARIABLE} is a commit id in lowercase hexadecimal, not {base:?}"
        ));
    }
    let run = |arguments: &[&str]| {
        let output = git(root)
            .args(arguments)
            .output()
            .map_err(|error| format!("git: {error}"))?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    };
    run(&["cat-file", "-e", &format!("{base}^{{commit}}")])
        .map_err(|error| format!("base commit {base} is not available: {error}"))?;
    // Paths relative to the repository root, even where git's top level is above it.
    let present = run(&["ls-tree", "--name-only", base, "--", ALLOWLIST])
        .map_err(|error| format!("git ls-tree {base}: {error}"))?;
    if present.is_empty() {
        return Ok(None);
    }
    let shown = run(&["show", &format!("{base}:./{ALLOWLIST}")])
        .map_err(|error| format!("{ALLOWLIST} at {base}: {error}"))?;
    String::from_utf8(shown)
        .map(Some)
        .map_err(|_| format!("{ALLOWLIST} at {base} is not UTF-8"))
}

#[test]
fn production_files_stay_under_the_cap_and_listed_files_only_shrink() {
    let root = repository();
    let read = sources(&root);
    let counted = measure(&root, &read);
    if std::env::var_os("FILE_SIZE_RATCHET_PRINT").is_some() {
        println!("{}", over_the_cap(&counted));
    }
    let text = fs::read_to_string(root.join(ALLOWLIST))
        .unwrap_or_else(|error| panic!("{ALLOWLIST}: {error}"));
    let listed = allowlist(&text).unwrap_or_else(|error| panic!("{error}"));
    let mut problems = compare(&counted, &listed);
    match tracked(&root) {
        Ok(Some(tracked)) => problems.extend(unread(&tracked, &read)),
        Ok(None) => {}
        Err(error) => problems.push(error),
    }
    assert!(
        problems.is_empty(),
        "file-size ratchet ({RULES}):\n{}",
        problems.join("\n")
    );
}

#[test]
#[ignore = "compares the list with a base commit: set FILE_SIZE_RATCHET_BASE and pass \
            --include-ignored (CI job large-files-list)"]
fn the_list_only_shrinks_since_its_base() {
    let base = std::env::var(BASE_VARIABLE)
        .unwrap_or_else(|_| panic!("{BASE_VARIABLE} names the commit to compare the list with"));
    let root = repository();
    let Some(before) = list_at(&root, &base).unwrap_or_else(|error| panic!("{error}")) else {
        println!("{ALLOWLIST} does not exist at {base}; nothing to compare with");
        return;
    };
    let before = base_entries(&before).unwrap_or_else(|error| panic!("at {base}: {error}"));
    let text = fs::read_to_string(root.join(ALLOWLIST))
        .unwrap_or_else(|error| panic!("{ALLOWLIST}: {error}"));
    let after = allowlist(&text).unwrap_or_else(|error| panic!("{error}"));
    let problems = shrinks(&before, &after, |path| root.join(path).exists());
    assert!(
        problems.is_empty(),
        "{ALLOWLIST} only shrinks. Compared with {base}:\n{}\nSplit the file instead, to \
         pieces within the cap ({RULES}).",
        problems.join("\n")
    );
    let total = after.values().sum::<usize>();
    println!(
        "{ALLOWLIST}: {} files, {total} lines; nothing added since {base}",
        after.len()
    );
}

mod classify {
    use super::is_test_code;

    #[test]
    fn dedicated_test_files_and_directories_are_not_counted() {
        for path in [
            "crates/a/tests/cli.rs",
            "crates/a/tests/support/mod.rs",
            "crates/a/src/tests.rs",
            "crates/a/src/store/open/tests/messaging.rs",
            "crates/a/src/wake_tests.rs",
            "crates/a/src/web_process_tests.rs",
            "crates/a/src/web_process_tests/organizations.rs",
            "apps/b/setup-tests/setup.rs",
            "apps/b/testing/fake_office_server.rs",
            "crates/a/src/testing.rs",
            "crates/a/src/fs/testing.rs",
            "crates/a/src/testkit.rs",
            "crates/a/src/testkit/suite/structure.rs",
            "crates/a/src/test_support.rs",
            "crates/a/src/test_support/custody.rs",
        ] {
            assert!(is_test_code(path), "{path} is test code");
        }
    }

    #[test]
    fn everything_else_is_production_code() {
        for path in [
            "crates/a/src/lib.rs",
            "crates/a/src/manager.rs",
            "crates/a/build.rs",
            "crates/a/examples/demo.rs",
            "crates/a/benches/latency.rs",
            "apps/b/src/main.rs",
            // A `#[cfg(test)]` module under another name counts.
            "crates/a/src/test_teardown.rs",
            "crates/a/src/test_programs.rs",
            // Names that only contain a test name are production names.
            "crates/a/src/contests.rs",
            "crates/a/src/tests_helper.rs",
            "crates/a/src/testsuite.rs",
            "crates/a/src/attests/lib.rs",
            "crates/a/src/latest_tests_helper.rs",
            "crates/tests.rs.d/src/lib.rs",
        ] {
            assert!(!is_test_code(path), "{path} is production code");
        }
    }
}

mod count {
    use super::lines;

    #[test]
    fn lines_are_what_wc_counts_plus_an_unterminated_last_line() {
        assert_eq!(lines(b""), 0);
        assert_eq!(lines(b"\n"), 1);
        assert_eq!(lines(b"\n\n"), 2);
        assert_eq!(lines(b"fn a() {}\n"), 1);
        assert_eq!(lines(b"fn a() {}\nfn b() {}\n"), 2);
        assert_eq!(lines(b"fn a() {}\nfn b() {}"), 2);
        assert_eq!(lines(b"fn a() {}"), 1);
    }

    #[test]
    fn line_endings_multibyte_text_and_invalid_utf8_do_not_change_the_count() {
        assert_eq!(lines(b"a\r\nb\r\n"), 2);
        assert_eq!(lines("é\n日本\n🦀\n".as_bytes()), 3);
        assert_eq!(lines(b"\xff\xfe\n\x80"), 2);
        // A lone carriage return is not a line ending.
        assert_eq!(lines(b"a\rb\n"), 1);
    }
}

mod walk {
    use std::fs;

    use super::{measure, sources};

    #[test]
    fn the_walk_reads_sources_and_skips_only_build_output_and_tool_state() {
        let root = tempfile::tempdir().unwrap();
        let write = |path: &str| {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        };
        write("crates/a/src/lib.rs");
        write("crates/a/src/target/mod.rs");
        write("crates/a/tests/cli.rs");
        write("apps/b/build.rs");
        write("apps/b/notes.md");
        write("crates/org-platform/src/lib.rs");
        write("target/debug/build/generated.rs");
        write("target/CACHEDIR.TAG");
        write("other-output/CACHEDIR.TAG");
        write("other-output/generated.rs");
        write("apps/ui/node_modules/dep/index.rs");
        write(".git/hooks/sample.rs");
        write(".claude/skills/x.rs");
        // Checkouts nested in this one: a clone (`.git` directory) and a worktree (file).
        write("nested-clone/.git/HEAD");
        write("nested-clone/crates/a/src/lib.rs");
        write("crates/nested-worktree/.git");
        write("crates/nested-worktree/src/lib.rs");
        std::os::unix::fs::symlink(
            root.path().join("crates/a/src/lib.rs"),
            root.path().join("crates/a/src/linked.rs"),
        )
        .unwrap();
        let read = sources(root.path());
        assert_eq!(
            read,
            [
                "apps/b/build.rs",
                "crates/a/src/lib.rs",
                "crates/a/src/linked.rs",
                "crates/a/src/target/mod.rs",
                "crates/a/tests/cli.rs",
                "crates/org-platform/src/lib.rs",
            ]
        );
        let measured: Vec<_> = measure(root.path(), &read).into_keys().collect();
        assert_eq!(
            measured,
            [
                "apps/b/build.rs",
                "crates/a/src/lib.rs",
                "crates/a/src/linked.rs",
                "crates/a/src/target/mod.rs",
                "crates/org-platform/src/lib.rs",
            ],
            "every source but test code is measured, org-platform's own included"
        );
    }
}

mod coverage {
    use super::{git, repository, sources, tracked, unread};

    #[test]
    fn a_tracked_source_the_walk_skips_is_reported() {
        let read = vec!["crates/a/src/lib.rs".to_owned()];
        let tracked = vec![
            "crates/a/.os/imp.rs".to_owned(),
            "crates/a/src/lib.rs".to_owned(),
        ];
        let problems = unread(&tracked, &read);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("crates/a/.os/imp.rs: tracked, but in a directory"));
    }

    #[test]
    fn a_tracked_source_that_is_gone_from_disk_is_not_reported() {
        let root = tempfile::tempdir().unwrap();
        let git = |arguments: &[&str]| {
            git(root.path())
                .args(arguments)
                .output()
                .is_ok_and(|output| output.status.success())
        };
        std::fs::write(root.path().join("kept.rs"), "").unwrap();
        std::fs::write(root.path().join("renamed.rs"), "").unwrap();
        if !(git(&["init", "-q"]) && git(&["add", "kept.rs", "renamed.rs"])) {
            return; // no usable git here; the rule is then skipped altogether
        }
        std::fs::rename(root.path().join("renamed.rs"), root.path().join("new.rs")).unwrap();
        assert_eq!(tracked(root.path()), Ok(Some(vec!["kept.rs".to_owned()])));
    }

    #[test]
    fn a_checkout_git_cannot_list_is_an_error_and_a_source_archive_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(tracked(root.path()), Ok(None), "no .git: a source archive");
        std::fs::write(root.path().join(".git"), "gitdir: /nonexistent/ratchet\n").unwrap();
        let problem = tracked(root.path()).expect_err("a broken checkout");
        assert!(problem.starts_with("git ls-files"), "{problem}");
    }

    #[test]
    fn every_tracked_source_of_this_checkout_is_read() {
        let root = repository();
        if let Some(tracked) = tracked(&root).unwrap_or_else(|error| panic!("{error}")) {
            assert!(!tracked.is_empty(), "git lists the workspace's sources");
            assert_eq!(unread(&tracked, &sources(&root)), [] as [String; 0]);
        }
    }
}

mod list {
    use super::allowlist;

    const HEADER: &str = "# Production files over the cap.\n";

    fn error(entries: &str) -> String {
        allowlist(&format!("{HEADER}{entries}")).expect_err(entries)
    }

    #[test]
    fn a_commented_sorted_list_of_counts_over_the_cap_is_read() {
        let list = allowlist(&format!(
            "{HEADER}#\n# More words.\n\napps/b/src/api.rs 1387\ncrates/a/src/lib.rs 1001\n\
             crates/a/src/x/mod.rs 6002\n# A trailing note.\n"
        ))
        .expect("a valid list");
        assert_eq!(list.len(), 3);
        assert_eq!(list["apps/b/src/api.rs"], 1387);
        assert_eq!(list["crates/a/src/lib.rs"], 1001);
        assert_eq!(list["crates/a/src/x/mod.rs"], 6002);
        assert!(allowlist(HEADER).expect("an empty list").is_empty());
    }

    #[test]
    fn a_list_without_its_opening_comment_fails() {
        for text in ["", "\n# late\n", "crates/a.rs 1001\n", " # indented\n"] {
            let problem = allowlist(text).expect_err(text);
            assert!(
                problem.contains(":1: the list opens with a `#` comment"),
                "{problem}"
            );
        }
    }

    #[test]
    fn an_unsorted_or_repeated_entry_fails_at_its_line() {
        for entries in [
            "crates/b.rs 1001\ncrates/a.rs 1001\n",
            "crates/a.rs 1001\ncrates/a.rs 1001\n",
            "crates/a.rs 1001\ncrates/a.rs 2002\n",
            // `-` sorts before `/`, as git and the walk sort.
            "crates/org-host/src/a.rs 1001\ncrates/org-host-control/src/b.rs 1001\n",
        ] {
            let problem = error(entries);
            assert!(
                problem.ends_with(":3: entries are sorted by path and listed once"),
                "{entries:?}: {problem}"
            );
        }
    }

    #[test]
    fn an_entry_not_written_exactly_so_fails() {
        for entries in [
            "crates/a.rs\n",
            "crates/a.rs \n",
            "crates/a.rs two\n",
            "crates/a.rs 1,001\n",
            "crates/a.rs +1001\n",
            "crates/a.rs -1001\n",
            "crates/a.rs 01001\n",
            "crates/a.rs  1001\n",
            "crates/a.rs 1001 \n",
            "crates/a.rs\t1001\n",
            " crates/a.rs 1001\n",
            "crates/a.rs 1001 # a note\n",
            "crates/a.rs 99999999999999999999999\n",
        ] {
            let problem = error(entries);
            assert!(
                problem.contains(":2: expected `path lines`"),
                "{entries:?}: {problem}"
            );
        }
    }

    #[test]
    fn a_path_that_is_not_plain_relative_rust_fails() {
        for path in [
            "/crates/a.rs",
            "./crates/a.rs",
            "crates/../a.rs",
            "crates/./a.rs",
            "crates//a.rs",
            "crates\\a.rs",
            "crates/a.txt",
            "crates/a.rs/",
            "crates/a\u{a0}b.rs",
        ] {
            let problem = error(&format!("{path} 1001\n"));
            assert!(
                problem.contains(":2: ") && problem.contains("not a plain relative .rs path"),
                "{path:?}: {problem}"
            );
        }
    }

    #[test]
    fn listing_test_code_or_a_count_within_the_cap_fails() {
        let problem = error("crates/a/src/tests.rs 6338\n");
        assert!(problem.contains("test code, which the cap does not count"));
        let problem = error("crates/a/tests/cli.rs 1500\n");
        assert!(problem.contains("test code, which the cap does not count"));
        for count in ["0", "800", "1000"] {
            let problem = error(&format!("crates/a.rs {count}\n"));
            assert!(
                problem.contains(&format!("a count of {count} is within the 1000-line cap")),
                "{problem}"
            );
        }
    }

    #[test]
    fn this_checkouts_list_is_valid_and_says_what_it_is() {
        let root = super::repository();
        let text = std::fs::read_to_string(root.join(super::ALLOWLIST)).unwrap();
        allowlist(&text).unwrap_or_else(|error| panic!("{error}"));
        for words in ["only shrinks", super::RULES, "file_size_ratchet.rs"] {
            assert!(text.contains(words), "the opening comment mentions {words}");
        }
    }
}

mod ratchet {
    use std::collections::BTreeMap;
    use std::fs;

    use super::{allowlist, compare, measure, over_the_cap, sources};

    fn counted(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
        entries
            .iter()
            .map(|&(path, lines)| (path.to_owned(), lines))
            .collect()
    }

    fn listed(entries: &str) -> BTreeMap<String, usize> {
        allowlist(&format!("# The list.\n{entries}")).expect("a valid list")
    }

    #[test]
    fn files_within_the_cap_and_listed_files_at_their_count_pass() {
        let files = counted(&[("a.rs", 1500), ("edge.rs", 1000), ("small.rs", 20)]);
        let problems = compare(&files, &listed("a.rs 1500\n"));
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_new_file_over_the_cap_fails() {
        let files = counted(&[("a.rs", 1500), ("new.rs", 1001)]);
        let problems = compare(&files, &listed("a.rs 1500\n"));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("new.rs: 1001 lines, over the 1000-line cap"));
    }

    #[test]
    fn a_listed_file_that_gains_one_line_fails() {
        let problems = compare(&counted(&[("a.rs", 1501)]), &listed("a.rs 1500\n"));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("a.rs: 1501 lines, and its entry allows 1500."));
    }

    #[test]
    fn a_listed_file_that_shrinks_fails_until_its_entry_is_lowered() {
        let problems = compare(&counted(&[("a.rs", 1499)]), &listed("a.rs 1500\n"));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("Lower the entry in .github/large-files.allow to 1499"));
        assert!(compare(&counted(&[("a.rs", 1499)]), &listed("a.rs 1499\n")).is_empty());
    }

    #[test]
    fn a_stale_entry_fails_so_the_list_states_what_is_left() {
        let files = counted(&[("shrunk.rs", 1000), ("tiny.rs", 3)]);
        let problems = compare(
            &files,
            &listed("gone.rs 1200\nshrunk.rs 4000\ntiny.rs 1001\n"),
        );
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].starts_with("shrunk.rs: 1000 lines, within the 1000-line cap."));
        assert!(problems[1].starts_with("tiny.rs: 3 lines, within the 1000-line cap."));
        assert!(problems[2].starts_with("gone.rs: no such production source."));
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let files = counted(&[("a.rs", 2001), ("b.rs", 1999), ("c.rs", 5000)]);
        let problems = compare(&files, &listed("a.rs 2000\nb.rs 2000\nd.rs 3000\n"));
        let starts: Vec<_> = problems
            .iter()
            .map(|problem| problem.split(':').next().unwrap())
            .collect();
        assert_eq!(starts, ["a.rs", "b.rs", "c.rs", "d.rs"], "{problems:?}");
    }

    #[test]
    fn a_repository_is_measured_end_to_end() {
        let root = tempfile::tempdir().unwrap();
        let write = |path: &str, lines: usize| {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "// a line\n".repeat(lines)).unwrap();
        };
        write("crates/a/src/god.rs", 1201);
        write("crates/a/src/new.rs", 1001);
        write("crates/a/src/edge.rs", 1000);
        write("crates/a/src/tests.rs", 6000);
        write("crates/a/tests/cli.rs", 3000);
        write("apps/b/src/main.rs", 10);
        let counted = measure(root.path(), &sources(root.path()));
        assert_eq!(
            over_the_cap(&counted),
            "crates/a/src/god.rs 1201\ncrates/a/src/new.rs 1001\n"
        );
        let problems = compare(&counted, &listed("crates/a/src/god.rs 1200\n"));
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("crates/a/src/god.rs: 1201 lines, and its entry"));
        assert!(problems[1].starts_with("crates/a/src/new.rs: 1001 lines, over the"));
    }
}

mod base {
    use std::collections::BTreeMap;

    use super::{base_entries, git, list_at, shrinks, ALLOWLIST};

    /// No listed file is still there: every removed entry may pay for a rename.
    fn gone(_: &str) -> bool {
        false
    }

    fn list(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
        entries
            .iter()
            .map(|&(path, lines)| (path.to_owned(), lines))
            .collect()
    }

    #[test]
    fn lowered_and_deleted_entries_pass() {
        let before = list(&[("a.rs", 3000), ("b.rs", 1200)]);
        assert!(shrinks(&before, &before, gone).is_empty());
        assert!(shrinks(&before, &list(&[("a.rs", 2500)]), gone).is_empty());
        assert!(shrinks(&before, &list(&[]), gone).is_empty());
    }

    #[test]
    fn a_raised_count_fails_even_when_the_total_falls() {
        let before = list(&[("a.rs", 3000), ("b.rs", 1200)]);
        let problems = shrinks(&before, &list(&[("a.rs", 3001), ("b.rs", 1100)]), gone);
        assert_eq!(problems, ["a.rs: count raised from 3000 to 3001"]);
    }

    #[test]
    fn a_renamed_or_moved_file_passes_at_no_more_than_its_old_count() {
        let before = list(&[("a.rs", 3000), ("b.rs", 1200)]);
        assert!(shrinks(&before, &list(&[("a.rs", 3000), ("c.rs", 1200)]), gone).is_empty());
        assert!(shrinks(&before, &list(&[("a.rs", 3000), ("c.rs", 1100)]), gone).is_empty());
        // A directory moved: each new path pairs with an old one at least as long.
        assert!(shrinks(&before, &list(&[("y/a.rs", 1200), ("y/b.rs", 3000)]), gone).is_empty());
    }

    #[test]
    fn a_renamed_file_that_grows_fails() {
        let before = list(&[("a.rs", 3000), ("b.rs", 1200)]);
        let problems = shrinks(&before, &list(&[("a.rs", 3000), ("c.rs", 1201)]), gone);
        assert_eq!(
            problems,
            [
                "c.rs: added to the list (1201), and no entry of 1201 lines or more whose file \
              is gone makes it a rename"
            ]
        );
    }

    #[test]
    fn lines_won_elsewhere_never_pay_for_a_new_entry() {
        let before = list(&[("a.rs", 6002)]);
        let problems = shrinks(&before, &list(&[("a.rs", 4502), ("new.rs", 1400)]), gone);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("new.rs: added to the list (1400)"));
        // A split lists none of its pieces: each one is within the cap.
        let problems = shrinks(&before, &list(&[("a.rs", 3002), ("a2.rs", 3000)]), gone);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("a2.rs: added to the list (3000)"));
        // One removed entry renames one path, not two.
        let problems = shrinks(
            &list(&[("a.rs", 3000)]),
            &list(&[("b.rs", 1500), ("c.rs", 1400)]),
            gone,
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("c.rs: added to the list (1400)"));
    }

    #[test]
    fn a_piece_split_from_a_file_that_is_still_there_is_never_listed() {
        // `big.rs` fell within the cap, its entry went, and the piece moved out is listed.
        let before = list(&[("b.rs", 1500), ("big.rs", 3000)]);
        let after = list(&[("b.rs", 1500), ("big/engine.rs", 2800)]);
        let problems = shrinks(&before, &after, |path| path == "big.rs");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("big/engine.rs: added to the list (2800)"));
        // An unrelated file that shrank within the cap pays for nothing either.
        let after = list(&[("big.rs", 3000), ("new.rs", 1400)]);
        let problems = shrinks(&before, &after, |path| path == "b.rs");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("new.rs: added to the list (1400)"));
        // The same change with `big.rs` gone reads as a rename.
        let after = list(&[("b.rs", 1500), ("big/engine.rs", 2800)]);
        assert!(shrinks(&before, &after, gone).is_empty());
    }

    #[test]
    fn only_the_paths_no_pairing_renames_are_reported() {
        let before = list(&[("a.rs", 1450), ("b.rs", 1300)]);
        let after = list(&[("c.rs", 1500), ("d.rs", 1400)]);
        let problems = shrinks(&before, &after, gone);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("c.rs: added to the list (1500)"));
        // A swap of two listed paths is done in two changes.
        let before = list(&[("a.rs", 3000), ("b.rs", 1200)]);
        let swapped = list(&[("a.rs", 1200), ("c.rs", 3000)]);
        assert_eq!(shrinks(&before, &swapped, gone).len(), 1);
    }

    #[test]
    fn the_base_list_is_read_leniently() {
        let entries = base_entries(
            "# An older header.\n  crates/z.rs   1200  \n\ncrates/a.rs 900\ncrates/a/tests.rs 2000\n",
        )
        .expect("a lenient read");
        assert_eq!(
            entries,
            list(&[
                ("crates/a.rs", 900),
                ("crates/a/tests.rs", 2000),
                ("crates/z.rs", 1200)
            ])
        );
        let problem = base_entries("# A header.\ncrates/a.rs\n").expect_err("no count");
        assert!(problem.ends_with(":2: not `path count`"), "{problem}");
    }

    #[test]
    fn the_base_is_a_hexadecimal_commit_id_never_an_option() {
        let root = super::repository();
        let long = "a".repeat(65);
        for base in [
            "",
            "HEAD",
            "v1",
            "abc12",
            "ABCDEF1",
            "--output=/tmp/x",
            "-p",
            "abcdef1 ",
            "abcdef1:x",
            long.as_str(),
        ] {
            let problem = list_at(&root, base).expect_err(base);
            assert!(
                problem.contains("lowercase hexadecimal"),
                "{base:?}: {problem}"
            );
        }
    }

    /// The workspace sits one directory below git's top level, so the list's path is read
    /// relative to the workspace, not to the top level.
    #[test]
    fn the_list_is_read_at_its_base_and_absent_or_unknown_commits_are_told_apart() {
        let top = tempfile::tempdir().unwrap();
        let root = top.path().join("workspace");
        let git = |arguments: &[&str]| {
            git(&root)
                .args([
                    "-c",
                    "user.name=ratchet",
                    "-c",
                    "user.email=ratchet@invalid",
                ])
                .args([
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(arguments)
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        };
        let commit = |message: &str| {
            git(&["add", "-A"])?;
            git(&["commit", "-q", "--allow-empty", "-m", message])?;
            git(&["rev-parse", "HEAD"])
        };
        let path = root.join(ALLOWLIST);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(root.join(".github/other.allow"), "# Not the list.\n").unwrap();
        let initialized = super::git(top.path())
            .args(["init", "-q"])
            .status()
            .is_ok_and(|status| status.success());
        let Some(without) = initialized.then(|| commit("without the list")).flatten() else {
            return; // no usable git here
        };
        std::fs::write(&path, "# The list.\na.rs 1001\n").unwrap();
        let with = commit("with the list").expect("a commit");
        assert_eq!(list_at(&root, &without), Ok(None));
        assert_eq!(
            list_at(&root, &with),
            Ok(Some("# The list.\na.rs 1001\n".to_owned()))
        );
        let unknown = "0123456789abcdef0123456789abcdef01234567";
        let problem = list_at(&root, unknown).expect_err("an unknown commit");
        assert!(problem.starts_with(&format!("base commit {unknown} is not available")));
    }
}

mod hook {
    use std::fs;
    use std::process::Command;

    use super::{git, REPOSITORY_VARIABLES};

    /// Set in the child run, so the test that starts it never runs again inside it.
    const CHILD: &str = "FILE_SIZE_RATCHET_HOOK_CHILD";

    #[test]
    fn git_runs_without_the_callers_repository_variables_but_keeps_its_configuration() {
        let command = git(std::path::Path::new("."));
        let mut removed: Vec<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        removed.sort();
        assert_eq!(removed, REPOSITORY_VARIABLES);
        for kept in [
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_GLOBAL",
        ] {
            assert!(!removed.iter().any(|name| name == kept), "{kept} is kept");
        }
    }

    /// A pre-commit hook that runs this test binary exports the repository being committed
    /// as `GIT_DIR`, `GIT_INDEX_FILE` and `GIT_WORK_TREE`. The tests that drive git on
    /// temporary repositories leave that repository, its index and its history alone.
    #[test]
    fn tests_run_from_a_git_hook_leave_the_calling_repository_alone() {
        if std::env::var_os(CHILD).is_some() {
            return;
        }
        let caller = tempfile::tempdir().unwrap();
        let run = |arguments: &[&str]| {
            git(caller.path())
                .args([
                    "-c",
                    "user.name=ratchet",
                    "-c",
                    "user.email=ratchet@invalid",
                ])
                .args([
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(arguments)
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        };
        fs::write(caller.path().join("kept.txt"), "kept\n").unwrap();
        let ready = run(&["init", "-q"]).is_some()
            && run(&["add", "kept.txt"]).is_some()
            && run(&["commit", "-q", "-m", "the caller"]).is_some();
        if !ready {
            return; // no usable git here
        }
        let dot_git = caller.path().join(".git");
        let index = fs::read(dot_git.join("index")).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "--test-threads=1"])
            .arg("coverage::a_tracked_source_that_is_gone_from_disk_is_not_reported")
            .arg("base::the_list_is_read_at_its_base_and_absent_or_unknown_commits_are_told_apart")
            .env(CHILD, "1")
            .env("GIT_DIR", &dot_git)
            .env("GIT_INDEX_FILE", dot_git.join("index"))
            .env("GIT_WORK_TREE", caller.path())
            .output()
            .unwrap();
        let report = String::from_utf8_lossy(&child.stdout);
        assert!(
            child.status.success() && report.contains("test result: ok. 2 passed;"),
            "{report}{}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(
            fs::read(dot_git.join("index")).unwrap(),
            index,
            "index unchanged"
        );
        assert_eq!(run(&["status", "--porcelain"]).as_deref(), Some(""));
        assert_eq!(
            run(&["rev-list", "--count", "HEAD"]).as_deref(),
            Some("1\n")
        );
    }
}
