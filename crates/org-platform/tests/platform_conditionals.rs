//! The platform-conditional check (ADR-0017 LNX-5).
//!
//! Every operating-system difference lives in `crates/org-platform`. This test parses
//! every other `.rs` file of the repository with `syn` and counts the `cfg`,
//! `cfg_attr` and `cfg!` predicates that name `target_os`, `target_vendor`,
//! `target_env` or `target_family`. The files that had such predicates when the crate
//! was created are listed, each with its count, in `.github/platform-conditionals.allow`.
//! The test fails when:
//!
//! - a file that is not listed has a predicate;
//! - a listed file has more predicates than its entry (a listed file gains none);
//! - a listed file has fewer than its entry, or is gone (the entry is lowered or
//!   deleted in the same change, so the list always states what is left).
//!
//! The list therefore only shrinks, and is empty when Track X is complete. String
//! literals and comments are not predicates. `cfg(unix)` is outside the check: a
//! non-Unix target cannot build `org-platform` at all.
//!
//! To see what the check finds, in the allowlist's format:
//! `PLATFORM_CONDITIONALS_PRINT=1 cargo test -p org-platform --test platform_conditionals -- --nocapture`

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::visit::Visit;

const ALLOWLIST: &str = ".github/platform-conditionals.allow";
const EXEMPT: &str = "crates/org-platform";
const TARGET_KEYS: [&str; 4] = ["target_os", "target_vendor", "target_env", "target_family"];
/// Never source: build output, dependencies and tool state.
const SKIPPED_DIRECTORIES: [&str; 2] = ["target", "node_modules"];

/// The line of every platform predicate in one source file.
fn predicates(source: &str) -> Result<Vec<usize>, syn::Error> {
    let file = syn::parse_file(source)?;
    let mut found = Found(Vec::new());
    found.visit_file(&file);
    found.0.sort_unstable();
    Ok(found.0)
}

struct Found(Vec<usize>);

impl Found {
    /// `tokens` is the argument list of one `cfg`, `cfg_attr` or `cfg!`.
    fn predicate(&mut self, tokens: TokenStream, line: usize) {
        if names_a_target(tokens) {
            self.0.push(line);
        }
    }

    /// The body of a macro call or definition is not parsed by `syn`, so the
    /// conditionals inside it are found in its tokens: `#[cfg(..)]`, `#![cfg(..)]`,
    /// `#[cfg_attr(..)]` and `cfg!(..)`.
    fn macro_body(&mut self, tokens: TokenStream) {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        for (i, token) in tokens.iter().enumerate() {
            let TokenTree::Group(group) = token else {
                continue;
            };
            let after_cfg_bang = i >= 2
                && matches!(&tokens[i - 1], TokenTree::Punct(p) if p.as_char() == '!')
                && matches!(&tokens[i - 2], TokenTree::Ident(name) if name == "cfg");
            let after_hash = (i >= 1
                && matches!(&tokens[i - 1], TokenTree::Punct(p) if p.as_char() == '#'))
                || (i >= 2
                    && matches!(&tokens[i - 1], TokenTree::Punct(p) if p.as_char() == '!')
                    && matches!(&tokens[i - 2], TokenTree::Punct(p) if p.as_char() == '#'));
            if after_cfg_bang {
                self.predicate(group.stream(), group.span().start().line);
            } else if after_hash && group.delimiter() == Delimiter::Bracket {
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                match inner.as_slice() {
                    [TokenTree::Ident(name), TokenTree::Group(arguments)]
                        if name == "cfg" || name == "cfg_attr" =>
                    {
                        self.predicate(arguments.stream(), name.span().start().line);
                    }
                    _ => self.macro_body(group.stream()),
                }
            } else {
                self.macro_body(group.stream());
            }
        }
    }
}

impl<'ast> Visit<'ast> for Found {
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        let path = attribute.path();
        if path.is_ident("cfg") || path.is_ident("cfg_attr") {
            if let syn::Meta::List(list) = &attribute.meta {
                let line = path
                    .segments
                    .first()
                    .map_or(0, |segment| segment.ident.span().start().line);
                self.predicate(list.tokens.clone(), line);
            }
        }
        syn::visit::visit_attribute(self, attribute);
    }

    fn visit_macro(&mut self, call: &'ast syn::Macro) {
        let name = call.path.segments.last().map(|segment| &segment.ident);
        match name {
            Some(name) if name == "cfg" => {
                self.predicate(call.tokens.clone(), name.span().start().line);
            }
            _ => self.macro_body(call.tokens.clone()),
        }
        syn::visit::visit_macro(self, call);
    }
}

fn names_a_target(tokens: TokenStream) -> bool {
    tokens.into_iter().any(|token| match token {
        TokenTree::Ident(name) => TARGET_KEYS.iter().any(|key| name == key),
        TokenTree::Group(group) => names_a_target(group.stream()),
        _ => false,
    })
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/org-platform is two levels below the repository root")
        .to_path_buf()
}

/// Every `.rs` file of the repository outside `crates/org-platform`, as a path
/// relative to the root with `/` separators, sorted.
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
            let kind = entry.file_type().expect("a file type");
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .to_string_lossy()
                .replace('\\', "/");
            if kind.is_dir() {
                let skipped = name.starts_with('.')
                    || SKIPPED_DIRECTORIES.contains(&name.as_ref())
                    || relative == EXEMPT;
                if !skipped {
                    pending.push(path);
                }
            } else if kind.is_file() && name.ends_with(".rs") {
                found.push(relative);
            }
        }
    }
    found.sort();
    found
}

/// `path count` lines; blank lines and `#` comments are ignored.
fn allowlist(text: &str) -> Result<BTreeMap<String, usize>, String> {
    let mut listed = BTreeMap::new();
    let mut previous: Option<&str> = None;
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = line
            .rsplit_once(' ')
            .and_then(|(path, count)| Some((path.trim(), count.parse::<usize>().ok()?)))
            .filter(|(path, count)| !path.is_empty() && *count > 0);
        let Some((path, count)) = entry else {
            return Err(format!(
                "{ALLOWLIST}:{}: expected `path count` with a count of 1 or more",
                number + 1
            ));
        };
        if previous.is_some_and(|previous| previous >= path) {
            return Err(format!(
                "{ALLOWLIST}:{}: entries are sorted by path and listed once",
                number + 1
            ));
        }
        previous = Some(path);
        listed.insert(path.to_owned(), count);
    }
    Ok(listed)
}

/// What is wrong, one line each; empty when the sources and the list agree.
fn compare(found: &BTreeMap<String, Vec<usize>>, listed: &BTreeMap<String, usize>) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, lines) in found {
        let lines_text = lines
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        match listed.get(path) {
            None => problems.push(format!(
                "{path} (line {lines_text}): a platform predicate outside {EXEMPT}. Put the \
                 difference behind an org-platform contract (docs/design/linux-host.md, section 1)."
            )),
            Some(&allowed) if lines.len() > allowed => problems.push(format!(
                "{path} (line {lines_text}): {} platform predicates, and its entry allows \
                 {allowed}. A listed file gains none; new differences go into {EXEMPT}.",
                lines.len()
            )),
            Some(&allowed) if lines.len() < allowed => problems.push(format!(
                "{path}: {} platform predicates left, and its entry says {allowed}. Lower the \
                 entry in {ALLOWLIST}.",
                lines.len()
            )),
            Some(_) => {}
        }
    }
    for path in listed.keys() {
        if !found.contains_key(path) {
            problems.push(format!(
                "{path}: no platform predicate left. Delete its line from {ALLOWLIST}."
            ));
        }
    }
    problems
}

#[test]
fn platform_predicates_stay_inside_org_platform() {
    let root = repository();
    let mut found = BTreeMap::new();
    for path in sources(&root) {
        let source =
            fs::read_to_string(root.join(&path)).unwrap_or_else(|error| panic!("{path}: {error}"));
        let lines = predicates(&source)
            .unwrap_or_else(|error| panic!("{path}: not parsed, so not checked: {error}"));
        if !lines.is_empty() {
            found.insert(path, lines);
        }
    }
    if std::env::var_os("PLATFORM_CONDITIONALS_PRINT").is_some() {
        let mut text = String::new();
        for (path, lines) in &found {
            writeln!(text, "{path} {}", lines.len()).expect("a string");
        }
        println!("{text}");
    }
    let text = fs::read_to_string(root.join(ALLOWLIST))
        .unwrap_or_else(|error| panic!("{ALLOWLIST}: {error}"));
    let listed = allowlist(&text).unwrap_or_else(|error| panic!("{error}"));
    let problems = compare(&found, &listed);
    assert!(
        problems.is_empty(),
        "platform conditionals (ADR-0017 LNX-5):\n{}",
        problems.join("\n")
    );
}

mod scanner {
    use super::predicates;

    fn lines(source: &str) -> Vec<usize> {
        predicates(source).expect("the fixture parses")
    }

    #[test]
    fn attributes_on_items_statements_fields_and_arms_are_found() {
        let source = r#"
#![cfg(target_os = "macos")]
#[cfg(target_os = "linux")]
mod linux;
struct S {
    #[cfg(target_vendor = "apple")]
    field: u8,
}
fn f(x: u8) -> u8 {
    #[cfg(not(target_env = "gnu"))]
    let y = 1;
    match x {
        #[cfg(any(unix, target_family = "wasm"))]
        0 => 1,
        _ => 2,
    }
}
"#;
        assert_eq!(lines(source), [2, 3, 6, 10, 13]);
    }

    #[test]
    fn cfg_attr_and_the_cfg_macro_are_found() {
        let source = r#"
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn f() -> bool {
    if cfg!(target_os = "linux") { return true; }
    cfg!(all(test, not(target_vendor = "apple")))
}
"#;
        assert_eq!(lines(source), [2, 4, 5]);
    }

    #[test]
    fn conditionals_inside_macro_calls_and_definitions_are_found() {
        let source = r#"
fn f() {
    assert!(cfg!(target_os = "macos"), "only here");
    let v = vec![1, if cfg!(target_env = "gnu") { 2 } else { 3 }];
    println!("{}", {
        #[cfg(target_os = "linux")]
        let x = 1;
        x
    });
}
macro_rules! gated {
    ($item:item) => {
        #[cfg(target_os = "macos")]
        $item
        #[cfg_attr(target_vendor = "apple", inline)]
        fn g() {}
    };
}
"#;
        assert_eq!(lines(source), [3, 4, 6, 13, 15]);
    }

    #[test]
    fn other_conditions_strings_and_comments_are_not_predicates() {
        let source = r##"
// #[cfg(target_os = "macos")]
/// cfg!(target_os = "linux")
#[cfg(unix)]
#[cfg(test)]
#[cfg(all(feature = "x", target_arch = "x86_64", target_pointer_width = "64"))]
#[cfg_attr(test, allow(dead_code))]
fn f() -> &'static str {
    let target_os = "a local named like the key";
    let _ = (target_os, cfg!(debug_assertions));
    let _ = std::env::var("CARGO_CFG_TARGET_OS");
    assert!(!r#"#[cfg(target_os = "macos")] cfg!(target_family = "unix")"#.is_empty());
    "[target.'cfg(target_os = \"macos\")'.dependencies]"
}
"##;
        assert_eq!(lines(source), [] as [usize; 0]);
    }

    #[test]
    fn a_file_that_does_not_parse_is_an_error_not_a_pass() {
        assert!(predicates("fn broken( {").is_err());
    }
}

mod list {
    use std::collections::BTreeMap;

    use super::{allowlist, compare};

    fn found(entries: &[(&str, usize)]) -> BTreeMap<String, Vec<usize>> {
        entries
            .iter()
            .map(|(path, count)| ((*path).to_owned(), (1..=*count).collect()))
            .collect()
    }

    fn listed(text: &str) -> BTreeMap<String, usize> {
        allowlist(text).expect("a valid list")
    }

    #[test]
    fn the_list_is_sorted_counted_and_commented() {
        let list = listed("# why\n\ncrates/a/src/lib.rs 2\ncrates/b/src/lib.rs 1\n");
        assert_eq!(list.len(), 2);
        assert_eq!(list["crates/a/src/lib.rs"], 2);
        for bad in [
            "crates/a/src/lib.rs\n",
            "crates/a/src/lib.rs 0\n",
            "crates/a/src/lib.rs two\n",
            "crates/b.rs 1\ncrates/a.rs 1\n",
            "crates/a.rs 1\ncrates/a.rs 1\n",
        ] {
            assert!(allowlist(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sources_that_match_the_list_pass() {
        let problems = compare(&found(&[("a.rs", 2)]), &listed("a.rs 2\n"));
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_predicate_in_an_unlisted_file_fails() {
        let problems = compare(&found(&[("a.rs", 2), ("new.rs", 1)]), &listed("a.rs 2\n"));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("new.rs (line 1): a platform predicate outside"));
    }

    #[test]
    fn a_new_predicate_in_a_listed_file_fails() {
        let problems = compare(&found(&[("a.rs", 3)]), &listed("a.rs 2\n"));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("3 platform predicates, and its entry allows 2"));
    }

    #[test]
    fn a_stale_entry_fails_so_the_list_only_shrinks() {
        let problems = compare(&found(&[("a.rs", 1)]), &listed("a.rs 2\ngone.rs 1\n"));
        assert_eq!(problems.len(), 2);
        assert!(problems[0].contains("1 platform predicates left, and its entry says 2"));
        assert!(problems[1].starts_with("gone.rs: no platform predicate left"));
    }
}
