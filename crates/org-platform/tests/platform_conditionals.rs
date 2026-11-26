//! The platform-conditional check (ADR-0017 LNX-5).
//!
//! Every operating-system difference lives in `crates/org-platform`, or, for what only
//! the host links, in `crates/org-platform-host`. This test reads every other `.rs`
//! file of the repository, requires that `syn` parses it, and
//! counts in its tokens:
//!
//! - each `cfg` or `cfg_attr` attribute, each `cfg!` call, and each `cfg_select!` /
//!   `cfg_match!` arm predicate that names `target_os`, `target_vendor`, `target_env`,
//!   `target_family`, `unix`, or `windows`, wherever it is written, including inside
//!   macro calls and definitions;
//! - each other use of `target_os`, `target_vendor`, `target_env`, or `target_family` as
//!   an identifier: a key handed to a macro that builds the `cfg`. Those four names are
//!   reserved for `org-platform`.
//!
//! The files that had such predicates when the check was written (increment X1a) are
//! listed, each with its count, in `.github/platform-conditionals.allow`. The test fails when:
//!
//! - a file that is not listed has a predicate;
//! - a listed file has more predicates than its entry (a listed file gains none);
//! - a listed file has fewer than its entry, or is gone (the entry is lowered or
//!   deleted in the same change, so the list always states what is left);
//! - a file includes source this test cannot follow: `include!` of anything but a
//!   literal `.rs` path, `include` imported under another name, or a `path`
//!   attribute that does not end in `.rs`;
//! - git tracks a `.rs` file outside `crates/org-platform` that exists and the walk
//!   did not read (a hidden directory, `node_modules`, a directory holding
//!   `CACHEDIR.TAG`).
//!
//! The list is empty when Track X is complete. CI also checks that a change never
//! adds a line to it or raises a count (`.github/scripts/platform_allowlist_shrinks.py`).
//!
//! Not predicates: string literals, comments, and ordinary identifiers named `unix` or
//! `windows` outside configuration predicates (such as `std::os::unix::...`). Outside
//! what this scan sees, which review covers: a build script that emits its own `cfg`, a
//! `--cfg` flag, `std::env::consts::OS` compared at run time, code in a
//! documentation test (it is a string literal here), a Cargo target whose `path`
//! is not a `.rs` file, a `[target.'cfg(..)']` table in a manifest,
//! `cfg(target_feature = "crt-static")`, and an `include!` or `path` attribute
//! that a macro assembles from its arguments. If a later `syn`
//! cannot parse valid new syntax, the check fails and the `syn` pin is raised.
//!
//! To see what the check finds, in the allowlist's format:
//! `PLATFORM_CONDITIONALS_PRINT=1 cargo test -p org-platform --test platform_conditionals -- --nocapture`

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, Ident, TokenStream, TokenTree};

const ALLOWLIST: &str = ".github/platform-conditionals.allow";
/// The two crates that hold platform code: what every executable links, and what only
/// the host links (`crates/org-platform-host/src/lib.rs` says why there are two).
const EXEMPT: [&str; 2] = ["crates/org-platform", "crates/org-platform-host"];
/// How the messages name them.
const HOME: &str = "crates/org-platform (or crates/org-platform-host for what only the host links)";
const TARGET_KEYS: [&str; 4] = ["target_os", "target_vendor", "target_env", "target_family"];
/// Bare platform flags forbidden in configuration predicates outside `org-platform`
/// (prevents fail-open `#[cfg(unix)]` / `#[cfg(not(unix))]` / `#[cfg(windows)]` seams).
const PLATFORM_FLAGS: [&str; 2] = ["unix", "windows"];
/// The attributes and macros whose arguments are configuration predicates.
const CONDITIONALS: [&str; 2] = ["cfg", "cfg_attr"];
/// Macros whose arms pair a predicate with ordinary code. Their bodies are walked
/// like any other code, so each target key in an arm's predicate counts by itself.
const SELECTORS: [&str; 2] = ["cfg_select", "cfg_match"];
/// A build output directory, wherever it is and whatever it is called, has this file.
const BUILD_OUTPUT_MARKER: &str = "CACHEDIR.TAG";
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

/// What one source file holds.
#[derive(Debug, Default, PartialEq, Eq)]
struct Findings {
    /// The line of every platform predicate.
    predicates: Vec<usize>,
    /// `(line, what)` for every inclusion of source the check cannot follow.
    unfollowed: Vec<(usize, &'static str)>,
}

fn scan(source: &str) -> Result<Findings, String> {
    syn::parse_file(source).map_err(|error| error.to_string())?;
    let tokens: TokenStream = source.parse().map_err(|error| format!("{error}"))?;
    let mut findings = Findings::default();
    walk(tokens, false, &mut findings);
    findings.predicates.sort_unstable();
    findings.unfollowed.sort_unstable();
    findings.unfollowed.dedup();
    Ok(findings)
}

/// A raw identifier (`r#cfg`) is the same name.
fn named(ident: &Ident, names: &[&str]) -> bool {
    let name = ident.to_string();
    names.contains(&name.strip_prefix("r#").unwrap_or(&name))
}

fn punct(token: Option<&TokenTree>, character: char) -> bool {
    matches!(token, Some(TokenTree::Punct(p)) if p.as_char() == character)
}

/// `inside` is true within the arguments of a conditional that names a target and is
/// already counted: a bare key there belongs to it, but a conditional nested in it
/// (an item of a `cfg_select!` arm, a `cfg` inside a `cfg_attr`) is counted again, and
/// what it includes is still checked.
fn walk(tokens: TokenStream, inside: bool, findings: &mut Findings) {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            TokenTree::Ident(ident) if named(ident, &CONDITIONALS) => {
                // `cfg(..)` / `cfg_attr(..)` in an attribute, or `cfg!(..)` with any delimiter.
                let after_bang = usize::from(punct(tokens.get(index + 1), '!'));
                if let Some(TokenTree::Group(arguments)) = tokens.get(index + 1 + after_bang) {
                    if named(ident, &["cfg_attr"]) {
                        let args: Vec<TokenTree> = arguments.stream().into_iter().collect();
                        let split = args
                            .iter()
                            .position(|token| punct(Some(token), ','))
                            .unwrap_or(args.len());
                        let predicate: TokenStream = args[..split].iter().cloned().collect();
                        let rest: TokenStream = args[split..].iter().cloned().collect();
                        let counted = names_a_target_directly(predicate.clone());
                        if counted {
                            findings.predicates.push(ident.span().start().line);
                        }
                        walk(predicate, counted, findings);
                        walk(rest, false, findings);
                    } else {
                        let counted = names_a_target_directly(arguments.stream());
                        if counted {
                            findings.predicates.push(ident.span().start().line);
                        }
                        walk(arguments.stream(), counted, findings);
                    }
                    index += 2 + after_bang;
                    continue;
                }
            }
            TokenTree::Ident(ident) if named(ident, &SELECTORS) => {
                if let (true, Some(TokenTree::Group(arms))) =
                    (punct(tokens.get(index + 1), '!'), tokens.get(index + 2))
                {
                    walk_selector_arms(arms.stream(), findings);
                    index += 3;
                    continue;
                }
            }
            TokenTree::Ident(ident) if named(ident, &TARGET_KEYS) => {
                if !inside {
                    findings.predicates.push(ident.span().start().line);
                }
            }
            TokenTree::Ident(ident) if named(ident, &["include"]) => {
                let line = ident.span().start().line;
                if let (true, Some(TokenTree::Group(arguments))) =
                    (punct(tokens.get(index + 1), '!'), tokens.get(index + 2))
                {
                    if !rust_path_literal(arguments.stream()) {
                        findings
                            .unfollowed
                            .push((line, "include! of something other than a literal .rs path"));
                    }
                } else if matches!(tokens.get(index + 1), Some(TokenTree::Ident(next)) if next == "as")
                {
                    findings
                        .unfollowed
                        .push((line, "include imported under another name"));
                }
            }
            TokenTree::Group(group) => {
                if group.delimiter() == Delimiter::Bracket && punct_before(&tokens, index) {
                    path_attribute(group.stream(), findings);
                }
                walk(group.stream(), inside, findings);
            }
            _ => {}
        }
        index += 1;
    }
}

/// Whether the bracket group at `index` follows `#` or `#!`: an attribute.
fn punct_before(tokens: &[TokenTree], index: usize) -> bool {
    let before = |back: usize| index.checked_sub(back).and_then(|at| tokens.get(at));
    punct(before(1), '#') || (punct(before(1), '!') && punct(before(2), '#'))
}

/// `#[path = "…"]` names a module file, directly or through `cfg_attr` at any depth.
/// The check follows it only as a `.rs` file. `path = …` among the arguments of any
/// other attribute is that attribute's own business.
fn path_attribute(attribute: TokenStream, findings: &mut Findings) {
    let tokens: Vec<TokenTree> = attribute.into_iter().collect();
    match tokens.as_slice() {
        [TokenTree::Ident(name), TokenTree::Group(arguments)] if named(name, &["cfg_attr"]) => {
            // Each comma-separated argument after the predicate is an attribute.
            let arguments: Vec<TokenTree> = arguments.stream().into_iter().collect();
            for argument in arguments.split(|token| punct(Some(token), ',')) {
                path_attribute(argument.iter().cloned().collect(), findings);
            }
        }
        [TokenTree::Ident(name), equals, value @ ..]
            if named(name, &["path"])
                && punct(Some(equals), '=')
                && !rust_path_literal(value.iter().cloned().collect()) =>
        {
            findings.unfollowed.push((
                name.span().start().line,
                "path attribute that is not a literal .rs path",
            ));
        }
        _ => {}
    }
}

/// Whether `tokens` is exactly one string literal ending in `.rs`.
fn rust_path_literal(tokens: TokenStream) -> bool {
    syn::parse2::<syn::LitStr>(tokens).is_ok_and(|literal| literal.value().ends_with(".rs"))
}

/// Walks the arms of `cfg_select!` / `cfg_match!`: each arm's predicate (tokens before
/// `=>`) is checked for both target keys and bare `unix` / `windows` flags, and each
/// arm's body is walked as ordinary code.
fn walk_selector_arms(tokens: TokenStream, findings: &mut Findings) {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut index = 0;
    while index < tokens.len() {
        let start = index;
        while index + 1 < tokens.len()
            && !(punct(tokens.get(index), '=') && punct(tokens.get(index + 1), '>'))
        {
            index += 1;
        }
        if index + 1 >= tokens.len() {
            let tail: TokenStream = tokens[start..].iter().cloned().collect();
            walk(tail, false, findings);
            break;
        }
        let predicate_slice = &tokens[start..index];
        let predicate: TokenStream = predicate_slice.iter().cloned().collect();
        let counted = names_a_target_directly(predicate.clone());
        if counted {
            if let Some(first) = predicate_slice.first() {
                findings.predicates.push(first.span().start().line);
            }
        }
        walk(predicate, counted, findings);
        // Skip `=>`.
        index += 2;
        if let Some(body) = tokens.get(index) {
            walk(TokenStream::from(body.clone()), false, findings);
            index += 1;
        }
        if punct(tokens.get(index), ',') {
            index += 1;
        }
    }
}

/// Whether a conditional's own predicate names a target key or bare platform flag
/// (`unix`, `windows`). A conditional nested in it is not this one's: the walk counts it
/// by itself.
fn names_a_target_directly(tokens: TokenStream) -> bool {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            TokenTree::Ident(ident) if named(ident, &CONDITIONALS) => {
                let after_bang = usize::from(punct(tokens.get(index + 1), '!'));
                if matches!(
                    tokens.get(index + 1 + after_bang),
                    Some(TokenTree::Group(_))
                ) {
                    index += 2 + after_bang;
                    continue;
                }
            }
            TokenTree::Ident(ident)
                if named(ident, &TARGET_KEYS) || named(ident, &PLATFORM_FLAGS) =>
            {
                return true;
            }
            TokenTree::Group(group) if names_a_target_directly(group.stream()) => return true,
            _ => {}
        }
        index += 1;
    }
    false
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/org-platform is two levels below the repository root")
        .to_path_buf()
}

/// `git -C root`, without the calling environment's repository variables. A test run
/// from a git hook (or a worktree hook) inherits `GIT_DIR` and `GIT_INDEX_FILE`, which
/// would point every command, those on a temporary repository included, at the calling
/// repository and its index.
fn git(root: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command.arg("-C").arg(root);
    for name in REPOSITORY_VARIABLES {
        command.env_remove(name);
    }
    command
}

/// Every `.rs` file under `root` outside `crates/org-platform`, as a path relative to
/// the root with `/` separators, sorted. Skipped: hidden directories, `node_modules`,
/// and build output (a directory holding `CACHEDIR.TAG`, as every cargo target
/// directory does), so a source directory that happens to be named `target` is read.
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
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().expect("a file type").is_dir() {
                let skipped = name.starts_with('.')
                    || name == "node_modules"
                    || EXEMPT.contains(&relative.as_str())
                    || path.join(BUILD_OUTPUT_MARKER).exists();
                if !skipped {
                    pending.push(path);
                }
            } else if name.ends_with(".rs") && path.is_file() {
                // `is_file` follows a symbolic link, so a linked source file is read.
                found.push(relative);
            }
        }
    }
    found.sort();
    found
}

/// The `.rs` files git tracks under `root`, outside `crates/org-platform`; `None` where
/// there is no git checkout to ask (a source archive).
fn tracked(root: &Path) -> Option<Vec<String>> {
    let listed = git(root)
        .args(["ls-files", "-z", "--", "*.rs"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let exempt = EXEMPT.map(|crate_path| format!("{crate_path}/"));
    Some(
        String::from_utf8_lossy(&listed.stdout)
            .split('\0')
            // A file renamed or deleted without staging is still listed.
            .filter(|path| {
                !path.is_empty() && !exempt.iter().any(|prefix| path.starts_with(prefix))
            })
            .filter(|path| root.join(path).is_file())
            .map(str::to_owned)
            .collect(),
    )
}

/// Tracked sources the walk did not read, so the check did not see them.
fn unread(tracked: &[String], read: &[String]) -> Vec<String> {
    tracked
        .iter()
        .filter(|path| read.binary_search(path).is_err())
        .map(|path| {
            format!(
                "{path}: tracked, but in a directory the check does not read (hidden, \
                 node_modules or build output). Move it."
            )
        })
        .collect()
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
fn compare(found: &BTreeMap<String, Findings>, listed: &BTreeMap<String, usize>) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, findings) in found {
        for (line, what) in &findings.unfollowed {
            problems.push(format!(
                "{path} (line {line}): {what}. The check cannot read what it includes."
            ));
        }
        let lines = &findings.predicates;
        if lines.is_empty() {
            continue;
        }
        let lines_text = lines
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        match listed.get(path) {
            None => problems.push(format!(
                "{path} (line {lines_text}): a platform predicate outside {HOME}, or one of \
                 the names target_os, target_vendor, target_env and target_family, which are \
                 reserved for it. Put the difference behind an org-platform contract \
                 (docs/design/linux-host.md, section 1)."
            )),
            Some(&allowed) if lines.len() > allowed => problems.push(format!(
                "{path} (line {lines_text}): {} platform predicates, and its entry allows \
                 {allowed}. A listed file gains none; new differences go into {HOME}.",
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
        if found
            .get(path)
            .is_none_or(|findings| findings.predicates.is_empty())
        {
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
    let read = sources(&root);
    let mut found = BTreeMap::new();
    for path in read.clone() {
        let source =
            fs::read_to_string(root.join(&path)).unwrap_or_else(|error| panic!("{path}: {error}"));
        let findings = scan(&source)
            .unwrap_or_else(|error| panic!("{path}: not parsed, so not checked: {error}"));
        if findings != Findings::default() {
            found.insert(path, findings);
        }
    }
    if std::env::var_os("PLATFORM_CONDITIONALS_PRINT").is_some() {
        let mut text = String::new();
        for (path, findings) in &found {
            if !findings.predicates.is_empty() {
                writeln!(text, "{path} {}", findings.predicates.len()).expect("a string");
            }
        }
        println!("{text}");
    }
    let text = fs::read_to_string(root.join(ALLOWLIST))
        .unwrap_or_else(|error| panic!("{ALLOWLIST}: {error}"));
    let listed = allowlist(&text).unwrap_or_else(|error| panic!("{error}"));
    let mut problems = compare(&found, &listed);
    if let Some(tracked) = tracked(&root) {
        problems.extend(unread(&tracked, &read));
    }
    assert!(
        problems.is_empty(),
        "platform conditionals (ADR-0017 LNX-5):\n{}",
        problems.join("\n")
    );
}

mod scanner {
    use super::scan;

    fn lines(source: &str) -> Vec<usize> {
        let findings = scan(source).expect("the fixture parses");
        assert_eq!(
            findings.unfollowed,
            [],
            "nothing unfollowed in this fixture"
        );
        findings.predicates
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
    fn cfg_attr_and_the_cfg_macros_are_found_once_each() {
        let source = r#"
#[cfg_attr(target_os = "macos", allow(dead_code))]
#[cfg_attr(test, cfg(target_os = "linux"))]
fn f() -> bool {
    if cfg!(target_os = "linux") { return true; }
    if core::cfg!(any(target_os = "macos", target_os = "ios")) { return true; }
    cfg!(all(test, not(target_vendor = "apple")))
}
cfg_select! {
    target_os = "linux" => { fn os() -> u8 { 1 } }
    target_os = "macos" => { fn os() -> u8 { 2 } }
    _ => { fn os() -> u8 { 0 } }
}
"#;
        assert_eq!(lines(source), [2, 3, 5, 6, 7, 10, 11]);
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
    fn items_syn_keeps_verbatim_and_arguments_of_other_attributes_are_read() {
        let source = r#"
unsafe extern "C" {
    #[cfg(target_os = "linux")]
    pub unsafe static mut errno_x: i32;
    #[cfg(target_os = "linux")]
    safe fn abs(x: i32) -> i32;
}
#[derive(Debug)]
#[error("built for linux: {}", cfg!(target_os = "linux"))]
struct E;
"#;
        assert_eq!(lines(source), [3, 5, 9]);
    }

    #[test]
    fn raw_identifiers_are_the_same_names() {
        let source = r#"
#[cfg(r#target_os = "linux")]
fn a() {}
#[r#cfg(target_os = "linux")]
fn b() {}
#[r#cfg_attr(target_os = "linux", allow(dead_code))]
fn c() -> bool { r#cfg!(target_os = "linux") }
"#;
        assert_eq!(lines(source), [2, 4, 6, 7]);
    }

    #[test]
    fn a_target_key_outside_a_conditional_is_found() {
        let source = r#"
macro_rules! when { ([$($p:tt)*] $i:item) => { #[cfg($($p)*)] $i }; }
when!([target_os = "linux"] fn indirect() {});
fn f() {
    let target_env = "a local named like the key";
    let _ = target_env;
}
"#;
        assert_eq!(lines(source), [3, 5, 6]);
    }

    #[test]
    fn bare_unix_and_windows_predicates_are_found_while_ordinary_idents_pass() {
        let source = r#"
#[cfg(unix)]
fn a() {}
#[cfg(not(unix))]
fn b() {}
#[cfg(windows)]
fn c() {}
#[cfg(all(test, any(unix, windows)))]
fn d() {}
#[cfg_attr(unix, allow(dead_code))]
fn e() -> bool {
    cfg!(windows) || cfg!(not(unix))
}
cfg_select! {
    unix => { fn os() -> u8 { 1 } }
    windows => { fn os() -> u8 { 2 } }
    _ => { fn os() -> u8 { 0 } }
}
fn ordinary() {
    let unix = 1;
    let windows = 2;
    let _ = (unix, windows);
    #[cfg_attr(test, allow(dead_code))]
    let _ = std::os::unix::fs::PermissionsExt::mode;
}
"#;
        assert_eq!(lines(source), [2, 4, 6, 8, 10, 12, 12, 15, 16]);
    }

    #[test]
    fn other_conditions_strings_and_comments_are_not_predicates() {
        let source = r##"
// #[cfg(target_os = "macos")]
/// cfg!(target_os = "linux")
#[cfg(test)]
#[cfg(all(feature = "x", target_arch = "x86_64", target_pointer_width = "64"))]
#[cfg_attr(test, allow(dead_code))]
#[doc = "#[cfg(target_os = \"macos\")]"]
fn f() -> &'static str {
    let cfg = 1;
    let _ = (cfg, cfg!(debug_assertions));
    let _ = std::env::var("CARGO_CFG_TARGET_OS");
    assert!(!r#"#[cfg(target_os = "macos")] cfg!(target_family = "unix")"#.is_empty());
    "[target.'cfg(target_os = \"macos\")'.dependencies]"
}
"##;
        assert_eq!(lines(source), [] as [usize; 0]);
    }

    #[test]
    fn included_source_must_be_a_rust_file_the_walk_reads() {
        let followed = r#"
#[path = "tests/instance.rs"]
mod instance;
include!("generated.rs");
const TEXT: &str = include_str!("notes.txt");
const BYTES: &[u8] = include_bytes!("blob.bin");
"#;
        assert_eq!(scan(followed).unwrap().unfollowed, []);

        let unfollowed = r#"
#[path = "linux.inc"]
mod os;
include!("linux.inc");
include!(concat!(env!("OUT_DIR"), "/generated.rs"));
macro_rules! m { () => { include!("inner.inc"); }; }
"#;
        let findings = scan(unfollowed).unwrap();
        assert_eq!(
            findings.unfollowed,
            [
                (2, "path attribute that is not a literal .rs path"),
                (4, "include! of something other than a literal .rs path"),
                (5, "include! of something other than a literal .rs path"),
                (6, "include! of something other than a literal .rs path"),
            ]
        );
        assert_eq!(findings.predicates, [] as [usize; 0]);
    }

    #[test]
    fn conditionals_nested_in_a_conditional_are_each_counted() {
        let source = r#"
cfg_select! {
    target_os = "linux" => {
        #[cfg(target_env = "gnu")]
        fn a() -> u8 { 1 }
        #[cfg(target_env = "musl")]
        fn a() -> u8 { 2 }
        fn b() -> bool { cfg!(target_vendor = "unknown") }
    }
    _ => { fn a() -> u8 { 0 } }
}
#[cfg_attr(test, cfg(target_os = "macos"))]
fn c() {}
#[cfg_attr(target_os = "macos", cfg(target_env = "gnu"))]
fn d() {}
#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
fn e() {}
"#;
        assert_eq!(lines(source), [3, 4, 6, 8, 12, 14, 14, 16]);
    }

    #[test]
    fn a_selector_arm_hides_nothing() {
        let source = r#"
macro_rules! when { ([$($p:tt)*] $i:item) => { #[cfg($($p)*)] $i }; }
cfg_select! {
    target_os = "linux" => {
        when!([target_env = "gnu"] fn libc() -> u8 { 1 });
        when!([target_env = "musl"] fn libc() -> u8 { 2 });
    }
    _ => { fn libc() -> u8 { 0 } }
}
"#;
        assert_eq!(lines(source), [4, 5, 6]);
    }

    #[test]
    fn path_among_another_attributes_arguments_is_not_a_module_path() {
        let source = r#"
#[instrument(fields(path = %p.display()))]
#[route(get, path = "/pets/{id}")]
fn handler() {}
"#;
        assert_eq!(scan(source).unwrap(), super::Findings::default());
    }

    #[test]
    fn inclusion_inside_a_conditional_or_under_another_name_is_checked() {
        let source = r#"
cfg_select! { test => { include!("os.inc"); } _ => {} }
#[cfg_attr(all(), path = "os.inc")]
mod os;
#[cfg_attr(debug_assertions, cfg_attr(test, path = "nested.inc"))]
mod nested;
#[cfg_attr(debug_assertions, path = "fine.rs", allow(dead_code))]
mod fine;
use std::include as pull;
pull!("os.inc");
"#;
        let findings = scan(source).unwrap();
        assert_eq!(
            findings.unfollowed,
            [
                (2, "include! of something other than a literal .rs path"),
                (3, "path attribute that is not a literal .rs path"),
                (5, "path attribute that is not a literal .rs path"),
                (9, "include imported under another name"),
            ]
        );
        assert_eq!(findings.predicates, [] as [usize; 0]);

        // A field or a local named `include` or `path` is not an inclusion.
        let honest = r#"
struct Options { include: bool, path: String }
fn f(o: &Options) -> bool { let path = &o.path; o.include && !path.is_empty() }
"#;
        assert_eq!(scan(honest).unwrap(), super::Findings::default());
    }

    #[test]
    fn a_file_that_does_not_parse_is_an_error_not_a_pass() {
        assert!(scan("fn broken( {").is_err());
    }
}

mod walk {
    use std::fs;

    use super::sources;

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
        write("apps/b/build.rs");
        write("apps/b/notes.md");
        write("crates/org-platform/src/lib.rs");
        write("crates/org-platform-host/src/names/macos.rs");
        write("crates/org-platform-extra/src/lib.rs");
        write("target/debug/build/generated.rs");
        write("target/CACHEDIR.TAG");
        write("other-output/CACHEDIR.TAG");
        write("other-output/generated.rs");
        write("apps/ui/node_modules/dep/index.rs");
        write(".git/hooks/sample.rs");
        std::os::unix::fs::symlink(
            root.path().join("crates/a/src/lib.rs"),
            root.path().join("crates/a/src/linked.rs"),
        )
        .unwrap();
        assert_eq!(
            sources(root.path()),
            [
                "apps/b/build.rs",
                "crates/a/src/lib.rs",
                "crates/a/src/linked.rs",
                "crates/a/src/target/mod.rs",
                "crates/org-platform-extra/src/lib.rs",
            ]
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
        let run_git = |arguments: &[&str]| {
            git(root.path())
                .args(arguments)
                .output()
                .is_ok_and(|output| output.status.success())
        };
        std::fs::write(root.path().join("kept.rs"), "").unwrap();
        std::fs::write(root.path().join("renamed.rs"), "").unwrap();
        if !(run_git(&["init", "-q"]) && run_git(&["add", "kept.rs", "renamed.rs"])) {
            return; // no usable git here; the rule is then skipped altogether
        }
        std::fs::rename(root.path().join("renamed.rs"), root.path().join("new.rs")).unwrap();
        assert_eq!(tracked(root.path()), Some(vec!["kept.rs".to_owned()]));
    }

    #[test]
    fn every_tracked_source_of_this_checkout_is_read() {
        let root = repository();
        if let Some(tracked) = tracked(&root) {
            assert!(!tracked.is_empty(), "git lists the workspace's sources");
            assert!(tracked.iter().all(|path| {
                !path.starts_with("crates/org-platform/")
                    && !path.starts_with("crates/org-platform-host/")
            }));
            assert_eq!(unread(&tracked, &sources(&root)), [] as [String; 0]);
        }
    }
}

mod list {
    use std::collections::BTreeMap;

    use super::{allowlist, compare, Findings};

    fn found(entries: &[(&str, usize)]) -> BTreeMap<String, Findings> {
        entries
            .iter()
            .map(|(path, count)| {
                let findings = Findings {
                    predicates: (1..=*count).collect(),
                    unfollowed: Vec::new(),
                };
                ((*path).to_owned(), findings)
            })
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
    fn a_stale_entry_fails_so_the_list_states_what_is_left() {
        let problems = compare(&found(&[("a.rs", 1)]), &listed("a.rs 2\ngone.rs 1\n"));
        assert_eq!(problems.len(), 2);
        assert!(problems[0].contains("1 platform predicates left, and its entry says 2"));
        assert!(problems[1].starts_with("gone.rs: no platform predicate left"));
    }

    #[test]
    fn source_the_check_cannot_follow_fails_even_in_a_listed_file() {
        let mut sources = found(&[("a.rs", 1), ("b.rs", 0)]);
        for findings in sources.values_mut() {
            findings
                .unfollowed
                .push((7, "include! of something other than a literal .rs path"));
        }
        let problems = compare(&sources, &listed("a.rs 1\n"));
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("a.rs (line 7): include! of something other"));
        assert!(problems[1].starts_with("b.rs (line 7): include! of something other"));
    }
}
