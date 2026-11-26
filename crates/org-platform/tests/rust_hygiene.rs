//! Cross-platform Rust hygiene checks across all `cfg` branches and build profiles.
//!
//! Uses `proc_macro2` and `syn` (the exact Rust lexer and literal parser) so macOS,
//! Linux, debug, and release branches are checked on every host without hand-rolled
//! tokenizing:
//! 1. Every `#[allow(...)]` / `#![allow(...)]` attribute (including inside `cfg_attr`
//!    at any depth) specifies a top-level `reason = "..."` unsuffixed string literal
//!    with non-whitespace text.
//! 2. `write_volatile` stays confined to the canonical secret-zeroing primitives so
//!    workspace crates reuse `org_proto::zero_bytes` (which zeroes through `zeroize`) instead
//!    of duplicating `unsafe`.
//! 3. Unsafe code stays in the platform crates, and every other crate root forbids it
//!    (`rust_hygiene/unsafe_code.rs`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, Ident, Spacing, TokenStream, TokenTree};

/// A build output directory, wherever it is and whatever it is called, has this file.
const BUILD_OUTPUT_MARKER: &str = "CACHEDIR.TAG";

/// The only source files allowed to call `ptr::write_volatile` directly.
const ALLOWED_WRITE_VOLATILE_PATHS: [&str; 1] = ["crates/org-platform-host/src/credentials/mod.rs"];

#[path = "rust_hygiene/unsafe_code.rs"]
mod unsafe_code;

#[derive(Debug, Default, PartialEq, Eq)]
struct Findings {
    allow_without_reason: Vec<usize>,
    volatile_writes: Vec<usize>,
}

/// Exact Rust `Pattern_White_Space` set (matching `rustc_lexer::is_whitespace`).
fn is_rust_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{0085}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

fn is_ordinary_line_comment(rest: &str) -> bool {
    rest.starts_with("//")
        && !rest.starts_with("//!")
        && !(rest.starts_with("///") && !rest.starts_with("////"))
}

fn is_ordinary_block_comment(rest: &str) -> bool {
    rest.starts_with("/*")
        && !rest.starts_with("/*!")
        && !(rest.starts_with("/**") && !rest.starts_with("/***") && !rest.starts_with("/**/"))
}

fn skip_block_comment(bytes: &[u8], mut index: usize) -> usize {
    let mut depth = 1usize;
    index += 2;
    while index < bytes.len() && depth > 0 {
        if bytes[index..].starts_with(b"/*") {
            depth += 1;
            index += 2;
        } else if bytes[index..].starts_with(b"*/") {
            depth -= 1;
            index += 2;
        } else {
            index += 1;
        }
    }
    index
}

fn skip_rust_trivia(input: &str) -> &str {
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let rest = &input[index..];
        if let Some(ch) = rest.chars().next().filter(|&c| is_rust_whitespace(c)) {
            index += ch.len_utf8();
        } else if is_ordinary_line_comment(rest) {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if is_ordinary_block_comment(rest) {
            index = skip_block_comment(bytes, index);
        } else {
            break;
        }
    }
    &input[index..]
}

/// Strips a leading UTF-8 BOM and, when present, a Rust shebang line (`#!...` not
/// followed by optional trivia + `[`) while preserving the terminating `\n` so every
/// remaining token keeps its exact 1-based source line number.
fn strip_bom_and_shebang(source: &str) -> &str {
    let input = source.strip_prefix('\u{FEFF}').unwrap_or(source);
    let Some(after_hash_bang) = input.strip_prefix("#!") else {
        return input;
    };
    if skip_rust_trivia(after_hash_bang).starts_with('[') {
        return input;
    }
    match input.find('\n') {
        Some(nl) => &input[nl..],
        None => "",
    }
}

fn scan(source: &str) -> Result<Findings, String> {
    let cleaned = strip_bom_and_shebang(source);
    let tokens: TokenStream = cleaned.parse().map_err(|error| format!("{error}"))?;
    let mut findings = Findings::default();
    walk(tokens, &mut findings);
    findings.allow_without_reason.sort_unstable();
    findings.volatile_writes.sort_unstable();
    Ok(findings)
}

fn named(ident: &Ident, names: &[&str]) -> bool {
    let text = ident.to_string();
    names.contains(&text.strip_prefix("r#").unwrap_or(&text))
}

fn punct(token: Option<&TokenTree>, character: char) -> bool {
    matches!(token, Some(TokenTree::Punct(p)) if p.as_char() == character)
}

fn punct_with(token: Option<&TokenTree>, character: char, spacing: Spacing) -> bool {
    matches!(
        token,
        Some(TokenTree::Punct(p)) if p.as_char() == character && p.spacing() == spacing
    )
}

fn is_attribute_bracket(tokens: &[TokenTree], index: usize) -> bool {
    let before = |back: usize| index.checked_sub(back).and_then(|at| tokens.get(at));
    punct(before(1), '#') || (punct(before(1), '!') && punct(before(2), '#'))
}

fn has_non_empty_reason(args: TokenStream) -> bool {
    let tokens: Vec<TokenTree> = args.into_iter().collect();
    tokens.split(|tok| punct(Some(tok), ',')).any(|entry| {
        let [TokenTree::Ident(key), eq, value @ ..] = entry else {
            return false;
        };
        if !named(key, &["reason"]) || !punct(Some(eq), '=') || value.is_empty() {
            return false;
        }
        let stream: TokenStream = value.iter().cloned().collect();
        syn::parse2::<syn::LitStr>(stream)
            .is_ok_and(|lit| lit.suffix().is_empty() && !lit.value().trim().is_empty())
    })
}

/// Checks an attribute body (`allow(...)`, or recursively every attribute inside
/// `cfg_attr(predicate, attr1, attr2, ...)`). Non-lint attributes (such as
/// `custom(allow(example))`) are ignored.
fn check_attribute(attribute: TokenStream, findings: &mut Findings) {
    let tokens: Vec<TokenTree> = attribute.into_iter().collect();
    let [TokenTree::Ident(head), TokenTree::Group(args)] = tokens.as_slice() else {
        return;
    };
    if args.delimiter() != Delimiter::Parenthesis {
        return;
    }
    if named(head, &["cfg_attr"]) {
        let arg_tokens: Vec<TokenTree> = args.stream().into_iter().collect();
        for attr in arg_tokens.split(|tok| punct(Some(tok), ',')).skip(1) {
            if !attr.is_empty() {
                check_attribute(attr.iter().cloned().collect(), findings);
            }
        }
    } else if named(head, &["allow"]) && !has_non_empty_reason(args.stream()) {
        findings.allow_without_reason.push(head.span().start().line);
    }
}

fn is_lifetime_ident(tokens: &[TokenTree], index: usize) -> bool {
    index > 0 && punct_with(tokens.get(index - 1), '\'', Spacing::Joint)
}

fn walk(tokens: TokenStream, findings: &mut Findings) {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => {
                if group.delimiter() == Delimiter::Bracket && is_attribute_bracket(&tokens, index) {
                    check_attribute(group.stream(), findings);
                }
                walk(group.stream(), findings);
            }
            TokenTree::Ident(ident)
                if named(ident, &["write_volatile"]) && !is_lifetime_ident(&tokens, index) =>
            {
                findings.volatile_writes.push(ident.span().start().line);
            }
            _ => {}
        }
    }
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/org-platform is two levels below the repository root")
        .to_path_buf()
}

/// Every `.rs` file under `root`, as native relative paths sorted lexicographically.
/// Skips hidden directories, `node_modules`, and Cargo build directories (`CACHEDIR.TAG`).
fn sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        for entry in entries {
            let entry = entry.expect("a directory entry");
            let path = entry.path();
            let name = entry.file_name();
            let name_lossy = name.to_string_lossy();
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .to_path_buf();
            if entry.file_type().expect("a file type").is_dir() {
                let skipped = name_lossy.starts_with('.')
                    || name_lossy == "node_modules"
                    || path.join(BUILD_OUTPUT_MARKER).exists();
                if !skipped {
                    pending.push(path);
                }
            } else if name.as_encoded_bytes().ends_with(b".rs") && path.is_file() {
                found.push(relative);
            }
        }
    }
    found.sort();
    found
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> Result<PathBuf, String> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> Result<PathBuf, String> {
    std::str::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|error| format!("non-UTF-8 git path: {error}"))
}

/// Lists git-tracked `.rs` files under `root`. Returns `Ok(None)` only outside a git
/// checkout (a source archive without `.git`), and fails closed on `git` errors inside
/// a checkout so the completeness guarantee cannot be silently skipped.
fn tracked(root: &Path) -> Result<Option<Vec<PathBuf>>, String> {
    match root.join(".git").symlink_metadata() {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to stat .git: {error}")),
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--", "*.rs"])
        .output()
        .map_err(|error| format!("failed to run git ls-files: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git ls-files exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    let mut paths = Vec::new();
    for entry in output.stdout.split(|&b| b == 0) {
        if entry.is_empty() {
            continue;
        }
        let rel = bytes_to_path(entry)?;
        if root.join(&rel).is_file() {
            paths.push(rel);
        }
    }
    Ok(Some(paths))
}

fn is_allowed_write_volatile(path: &Path) -> bool {
    ALLOWED_WRITE_VOLATILE_PATHS
        .iter()
        .any(|allowed| path == Path::new(allowed))
}

#[test]
fn rust_sources_have_reasoned_allows_and_confined_write_volatile() {
    let root = repository();
    let read = sources(&root);
    let mut problems = Vec::new();
    let mut scanned = BTreeSet::new();

    for path in read {
        let source = fs::read_to_string(root.join(&path))
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let findings = scan(&source).unwrap_or_else(|error| {
            panic!("{}: not parsed, so not checked: {error}", path.display())
        });
        for line in &findings.allow_without_reason {
            problems.push(format!(
                "{}:{line}: allow-without-reason: `allow(...)` attribute must specify a \
                 non-empty `reason = \"...\"` (or use `#[expect(..., reason = \"...\")]`)",
                path.display()
            ));
        }
        if !is_allowed_write_volatile(&path) {
            for line in &findings.volatile_writes {
                problems.push(format!(
                    "{}:{line}: duplicate-write-volatile: use `org_proto::zero_bytes` \
                     instead of duplicating `ptr::write_volatile`",
                    path.display()
                ));
            }
        }
        scanned.insert(path);
    }

    match tracked(&root) {
        Ok(Some(tracked_files)) => {
            for path in tracked_files {
                if !scanned.contains(&path) {
                    problems.push(format!(
                        "{}: tracked `.rs` file was skipped by the hygiene walk",
                        path.display()
                    ));
                }
            }
        }
        Ok(None) => {}
        Err(error) => problems.push(format!("git completeness check failed: {error}")),
    }

    assert!(
        problems.is_empty(),
        "Rust hygiene check failed ({} finding(s)):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

mod scanner_tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{scan, sources, tracked, Findings};

    #[test]
    fn accepts_reasoned_allows_raw_reason_custom_attrs_shebang_and_unicode() {
        let source = concat!(
            "\u{FEFF}#!/usr/bin/env rust \"unmatched quote write_volatile\n",
            "#![cfg_attr(not(debug_assertions), allow(dead_code, reason = \"debug-only helper\"))]\n",
            "#[allow(clippy::too_many_arguments, reason /* note */ = \"explicit fields\")]\n",
            "#[allow(dead_code, r#reason = \"raw reason key\")]\n",
            "#[expect(dead_code, reason = \"held guard\")]\n",
            "#[cfg_attr(any(), custom(allow(example)))]\n",
            "fn ok() {\n",
            "    let fixture = \"#[allow(dead_code)]\";\n",
            "    let raw = r#\"#[cfg_attr(target_os = \"linux\", allow(dead_code))]\"#;\n",
            "    let ch = '\\u{61_____________________}';\n",
            "    macro_rules! eat_lit { ($x:literal) => { () }; }\n",
            "    eat_lit!(1write_volatile);\n",
            "    let _ = (fixture, raw, ch);\n",
            "}\n",
            "fn cafe\u{0301}write_volatile<'write_volatile>(x: &'write_volatile str) -> &'write_volatile str { x }\n",
            "fn kawi\u{11f00}write_volatile() {}\n",
        );
        assert_eq!(scan(source).expect("parses"), Findings::default());
    }

    #[test]
    fn preserves_line_numbers_after_shebang_and_catches_inner_attrs_after_trivia() {
        for shebang in [
            "#!/usr/bin/env rust write_volatile\n#[allow(dead_code)]\nfn f() {}\n",
            "#!/** docs */[write_volatile]\n#[allow(dead_code)]\nfn f() {}\n",
            "#!/*! docs */[write_volatile]\n#[allow(dead_code)]\nfn f() {}\n",
            "#!\u{00A0}[write_volatile]\n#[allow(dead_code)]\nfn f() {}\n",
            "#!/// docs write_volatile\n#[allow(dead_code)]\nfn f() {}\n",
            "#!//! docs write_volatile\n#[allow(dead_code)]\nfn f() {}\n",
        ] {
            let findings = scan(shebang).expect("parses");
            assert_eq!(findings.allow_without_reason, vec![2], "{shebang:?}");
            assert!(findings.volatile_writes.is_empty(), "{shebang:?}");
        }

        for inner in [
            "#! [allow(dead_code)]\nfn a() {}\n",
            "#!\u{200E}[allow(dead_code)]\nfn b() {}\n",
            "#!/* comment */[allow(dead_code)]\nfn c() {}\n",
            "#!/**/[allow(dead_code)]\nfn d() {}\n",
            "#!/***/[allow(dead_code)]\nfn e() {}\n",
            "#!// line comment\n[allow(dead_code)]\nfn f() {}\n",
            "#!//// four-slash ordinary comment\n[allow(dead_code)]\nfn g() {}\n",
        ] {
            let f = scan(inner).expect("parses");
            assert_eq!(f.allow_without_reason.len(), 1, "{inner:?} -> {f:?}");
        }
    }

    #[test]
    fn rejects_unreasoned_allows_suffixed_reason_unicode_ws_and_macro_inputs() {
        let source = concat!(
            "macro_rules! m { ($($tt:tt)*) => { $($tt)* }; }\n",
            "m!(::core::stringify!(#[allow(dead_code)] fn hidden_in_macro() {}));\n",
            "#[allow(dead_code /* reason = \"fixture\" */)]\n",
            "fn spoofed() {}\n",
            "#[allow(dead_code, reason = \"\\n\\t\\x20\\u{0020}\")]\n",
            "fn escaped_ws() {}\n",
            "#[allow(dead_code, reason = \"text\"suffix)]\n",
            "fn suffixed_reason() {}\n",
            "#[allow\u{200e}(dead_code)]\n",
            "fn bidi_ws() {}\n",
            "mod inner {\n",
            "    # /* gap */ ![allow(dead_code)]\n",
            "}\n",
            "#[cfg_attr(\n",
            "    not(debug_assertions),\n",
            "    cfg_attr(all(), allow(unused_mut))\n",
            ")]\n",
            "fn nested_cfg_attr() {}\n",
        );
        let findings = scan(source).expect("parses");
        assert_eq!(findings.allow_without_reason.len(), 7, "{findings:?}");
        assert!(findings.volatile_writes.is_empty());
    }

    #[test]
    fn flags_write_volatile_and_raw_write_volatile_only() {
        let source = r#"
fn wipe(b: &mut u8) {
    unsafe { std::ptr::write_volatile(b, 0) };
    unsafe { std::ptr::r#write_volatile(b, 0) };
}
"#;
        syn::parse_file(source).expect("fixture is valid Rust");
        let findings = scan(source).expect("parses");
        assert_eq!(findings.volatile_writes, vec![3, 4]);
    }

    #[test]
    fn tracked_reports_git_failures_and_allows_source_archives() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(tracked(dir.path()).expect("archive fallback"), None);

        // Passing a file as root makes `<root>/.git` fail with ENOTDIR (not NotFound).
        let not_a_dir = dir.path().join("file");
        fs::write(&not_a_dir, "").expect("write file");
        assert!(
            tracked(&not_a_dir).is_err(),
            "non-NotFound stat errors on .git must fail closed"
        );

        // A corrupt `.git` marker inside a checkout must fail closed instead of returning None.
        fs::write(dir.path().join(".git"), "not a valid gitdir").expect("write .git");
        assert!(
            tracked(dir.path()).is_err(),
            "git error inside a checkout must not be silently ignored"
        );
    }

    #[test]
    fn sources_includes_dot_rs_filename() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dot_rs = PathBuf::from(".rs");
        fs::write(dir.path().join(&dot_rs), "fn ok() {}\n").expect("write .rs");
        assert_eq!(sources(dir.path()), vec![dot_rs]);
    }

    #[cfg(unix)]
    #[test]
    fn sources_preserves_native_backslash_filenames_on_unix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backslash_name = PathBuf::from(r"a\b.rs");
        fs::write(dir.path().join(&backslash_name), "fn ok() {}\n").expect("write file");
        let found = sources(dir.path());
        assert_eq!(found, vec![backslash_name.clone()]);
        assert!(dir.path().join(&backslash_name).is_file());
    }
}
