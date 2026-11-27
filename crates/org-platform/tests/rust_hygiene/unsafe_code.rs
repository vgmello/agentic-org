//! 3. Unsafe code stays in the platform crates (`crates/org-platform`,
//!    `crates/org-platform-host`), behind their audited safe wrappers (`org_platform::posix`,
//!    the capabilities):
//!    - every library, binary and build-script (`build.rs`) target of every other package
//!      declares `#![forbid(unsafe_code)]` at its root (only the roots in [`DENY_ROOTS`] may
//!      `#![deny(unsafe_code)]` instead, for their one allowance below);
//!    - no other `.rs` file (tests, examples and build scripts included) has an `unsafe`
//!      keyword, an unsafe attribute (`no_mangle`, `export_name`, `link_section`) or an
//!      `allow`/`expect`/`warn` of `unsafe_code`, beyond the exact counts of
//!      [`ALLOWED_UNSAFE`], which may only shrink.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use super::{
    is_attribute_bracket, named, punct, repository, sources, strip_bom_and_shebang, tracked,
};

/// The crates that hold the workspace's unsafe code.
const PLATFORM_CRATES: [&str; 2] = ["crates/org-platform", "crates/org-platform-host"];

/// The only unsafe code outside the platform crates: path, `unsafe` keywords and unsafe
/// attributes, `unsafe_code` lint allowances, and why. Each count is exact, so the list says
/// what is left: lower it in the change that removes one; never raise it or add a path.
const ALLOWED_UNSAFE: [(&str, usize, usize, &str); 2] = [
    (
        "crates/org-office/src/store/sqlite/references.rs",
        1,
        1,
        "rusqlite has no safe sqlite3_db_status, the deferred-reference counter has no SQL \
         form, and the engine boundary keeps rusqlite out of org-platform",
    ),
    (
        "crates/org-supervise/tests/recovery_native.rs",
        3,
        0,
        "test fixtures that rewrite this child process's own environment block and argv[0] \
         storage, which no safe interface can express",
    ),
];

/// Crate roots that may lower `forbid` to `deny` for their one allowance in [`ALLOWED_UNSAFE`].
const DENY_ROOTS: [&str; 1] = ["crates/org-office/src/lib.rs"];

/// Attributes that are unsafe without the keyword (edition 2021).
const UNSAFE_ATTRIBUTES: [&str; 3] = ["no_mangle", "export_name", "link_section"];

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct UnsafeFindings {
    /// Lines of an `unsafe` keyword or an unsafe attribute.
    pub(crate) code: Vec<usize>,
    /// Lines of an `allow`, `expect` or `warn` of `unsafe_code`.
    pub(crate) allowances: Vec<usize>,
}

/// A lint list (the arguments of `allow(..)`) names `unsafe_code` itself: one entry that is
/// that single, bare name, raw (`r#unsafe_code`) or not. A tool's lint of the same name
/// (`clippy::unsafe_code`) is another lint.
fn lint_names(args: TokenStream) -> bool {
    let tokens: Vec<TokenTree> = args.into_iter().collect();
    tokens
        .split(|token| punct(Some(token), ','))
        .any(|entry| matches!(entry, [TokenTree::Ident(id)] if named(id, &["unsafe_code"])))
}

/// One attribute body: `allow(..)`-like with `unsafe_code`, an unsafe attribute, or
/// `cfg_attr(predicate, attr, ..)` holding either. An attribute's name is the same written
/// raw (`r#no_mangle`), which the compiler honours.
fn check_attribute(attribute: TokenStream, findings: &mut UnsafeFindings) {
    let tokens: Vec<TokenTree> = attribute.into_iter().collect();
    let Some(TokenTree::Ident(head)) = tokens.first() else {
        return;
    };
    let line = head.span().start().line;
    if named(head, &UNSAFE_ATTRIBUTES) {
        findings.code.push(line);
        return;
    }
    let Some(TokenTree::Group(args)) = tokens.get(1) else {
        return;
    };
    if args.delimiter() != Delimiter::Parenthesis {
        return;
    }
    if named(head, &["cfg_attr"]) {
        let inner: Vec<TokenTree> = args.stream().into_iter().collect();
        for attr in inner.split(|tok| punct(Some(tok), ',')).skip(1) {
            check_attribute(attr.iter().cloned().collect(), findings);
        }
    } else if named(head, &["allow", "expect", "warn"]) && lint_names(args.stream()) {
        findings.allowances.push(line);
    }
}

fn walk(tokens: TokenStream, findings: &mut UnsafeFindings) {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => {
                if group.delimiter() == Delimiter::Bracket && is_attribute_bracket(&tokens, index) {
                    check_attribute(group.stream(), findings);
                }
                walk(group.stream(), findings);
            }
            // A raw identifier (`r#unsafe`) is a name, not the keyword.
            TokenTree::Ident(i) if i == "unsafe" => findings.code.push(i.span().start().line),
            _ => {}
        }
    }
}

/// Every unsafe construct of `source` outside a string or comment.
pub(crate) fn scan_unsafe(source: &str) -> Result<UnsafeFindings, String> {
    let tokens: TokenStream = strip_bom_and_shebang(source)
        .parse()
        .map_err(|error| format!("{error}"))?;
    let mut findings = UnsafeFindings::default();
    walk(tokens, &mut findings);
    findings.code.sort_unstable();
    findings.allowances.sort_unstable();
    Ok(findings)
}

/// The top-level inner attributes of a file declare `#![<level>(.., unsafe_code, ..)]`
/// unconditionally (not under `cfg_attr`, not in a nested module, not another tool's lint
/// of that name); `level` and the lint may be written raw.
pub(crate) fn declares(source: &str, level: &str) -> Result<bool, String> {
    let tokens: Vec<TokenTree> = strip_bom_and_shebang(source)
        .parse::<TokenStream>()
        .map_err(|error| format!("{error}"))?
        .into_iter()
        .collect();
    Ok(tokens.windows(3).any(|window| {
        let [hash, bang, TokenTree::Group(body)] = window else {
            return false;
        };
        if !punct(Some(hash), '#')
            || !punct(Some(bang), '!')
            || body.delimiter() != Delimiter::Bracket
        {
            return false;
        }
        let body: Vec<TokenTree> = body.stream().into_iter().collect();
        matches!(
            body.as_slice(),
            [TokenTree::Ident(head), TokenTree::Group(args)]
                if named(head, &[level])
                    && args.delimiter() == Delimiter::Parenthesis
                    && lint_names(args.stream())
        )
    }))
}

fn in_platform(path: &Path) -> bool {
    PLATFORM_CRATES
        .iter()
        .any(|platform| path.starts_with(platform))
}

/// The root file of every library, binary and build-script (`custom-build`) target of every
/// workspace package outside the platform crates, relative to the repository.
fn crate_roots(root: &Path) -> Vec<PathBuf> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
        ])
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");
    let mut roots = Vec::new();
    for package in metadata["packages"].as_array().expect("a package list") {
        for target in package["targets"].as_array().expect("a target list") {
            let kinds = target["kind"].as_array().expect("target kinds");
            let compiled = kinds.iter().any(|kind| {
                matches!(
                    kind.as_str(),
                    Some(
                        "lib"
                            | "rlib"
                            | "dylib"
                            | "staticlib"
                            | "cdylib"
                            | "proc-macro"
                            | "bin"
                            | "custom-build"
                    )
                )
            });
            let source = Path::new(target["src_path"].as_str().expect("a source path"));
            let relative = source
                .strip_prefix(root)
                .expect("workspace sources are inside the repository")
                .to_path_buf();
            if compiled && !in_platform(&relative) {
                roots.push(relative);
            }
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

#[test]
fn unsafe_code_stays_in_the_platform_crates() {
    let root = repository();
    let mut problems = Vec::new();
    let allowed: BTreeMap<&str, (usize, usize)> = ALLOWED_UNSAFE
        .iter()
        .map(|(path, code, allowances, _)| (*path, (*code, *allowances)))
        .collect();
    let mut seen = BTreeMap::new();
    let mut files = sources(&root);
    match tracked(&root) {
        Ok(Some(tracked)) => {
            files.extend(tracked);
            files.sort();
            files.dedup();
        }
        Ok(None) => {}
        Err(error) => problems.push(format!("git completeness check failed: {error}")),
    }
    for path in files.iter().filter(|path| !in_platform(path)) {
        let source = fs::read_to_string(root.join(path))
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let findings = scan_unsafe(&source).unwrap_or_else(|error| {
            panic!("{}: not parsed, so not checked: {error}", path.display())
        });
        let key = path.to_string_lossy().replace('\\', "/");
        let found = (findings.code.len(), findings.allowances.len());
        match allowed.get(key.as_str()) {
            Some(&expected) if expected == found => {}
            Some(&expected) => problems.push(format!(
                "{key}: unsafe-allowance-count: has {found:?} (unsafe code, unsafe_code \
                 allowances) where ALLOWED_UNSAFE says {expected:?}; lower the entry when \
                 you remove one, never raise it"
            )),
            None => {
                for line in findings.code {
                    problems.push(format!(
                        "{key}:{line}: unsafe-outside-platform: move it behind a safe wrapper \
                         in org-platform (`posix` or a capability)"
                    ));
                }
                for line in findings.allowances {
                    problems.push(format!(
                        "{key}:{line}: unsafe-code-allowance-outside-platform: only the \
                         platform crates may allow `unsafe_code`"
                    ));
                }
            }
        }
        seen.insert(key, ());
    }
    for (path, ..) in ALLOWED_UNSAFE {
        if !seen.contains_key(path) {
            problems.push(format!(
                "{path}: stale-unsafe-allowance: the file is gone; delete its entry"
            ));
        }
    }
    for path in crate_roots(&root) {
        let key = path.to_string_lossy().replace('\\', "/");
        let source =
            fs::read_to_string(root.join(&path)).unwrap_or_else(|error| panic!("{key}: {error}"));
        let forbids = declares(&source, "forbid").unwrap_or(false);
        let denies =
            DENY_ROOTS.contains(&key.as_str()) && declares(&source, "deny").unwrap_or(false);
        if !forbids && !denies {
            problems.push(format!(
                "{key}: missing-forbid-unsafe-code: a crate root outside the platform crates \
                 declares `#![forbid(unsafe_code)]`"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "Unsafe-code confinement failed ({} finding(s)):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

mod scanner_tests {
    use super::{declares, scan_unsafe, UnsafeFindings};

    #[test]
    fn finds_every_unsafe_construct_and_no_name_string_or_comment() {
        let source = concat!(
            "// unsafe in a comment\n",
            "/* unsafe { block comment } */\n",
            "fn a() { let _ = \"unsafe { string }\"; let r#unsafe = 1; let _ = r#unsafe; }\n",
            "unsafe fn b() {}\n",
            "fn c() { unsafe { b() } }\n",
            "struct S;\nunsafe impl Send for S {}\n",
            "#[unsafe(no_mangle)]\nextern \"C\" fn d() {}\n",
            "#[no_mangle]\nextern \"C\" fn e() {}\n",
            "#[cfg_attr(all(), export_name = \"f\")]\nextern \"C\" fn f() {}\n",
            "#[allow(dead_code, unsafe_code, reason = \"fixture\")]\nfn g() {}\n",
            "#[cfg_attr(test, expect(unsafe_code, reason = \"fixture\"))]\nfn h() {}\n",
            "#[deny(unsafe_code)]\nfn i() {}\n",
            "#[allow(unsafe_op_in_unsafe_fn, reason = \"name only\")]\nfn j() {}\n",
            "/// unsafe in documentation\nfn k() {}\n",
            "#[allow(clippy::unsafe_code, reason = \"another tool's lint\")]\nfn l() {}\n",
        );
        assert_eq!(
            scan_unsafe(source).expect("parses"),
            UnsafeFindings {
                code: vec![4, 5, 7, 8, 10, 12],
                allowances: vec![14, 16],
            }
        );
    }

    /// The compiler honours an attribute or lint name written raw, so the scanner does.
    #[test]
    fn finds_unsafe_attributes_and_allowances_written_as_raw_identifiers() {
        let source = concat!(
            "#[r#no_mangle]\nextern \"C\" fn a() {}\n",
            "#[r#export_name = \"b\"]\nextern \"C\" fn b() {}\n",
            "#[r#link_section = \".c\"]\nstatic C: u8 = 0;\n",
            "#[r#cfg_attr(all(), r#no_mangle)]\nextern \"C\" fn d() {}\n",
            "#[r#allow(unsafe_code, reason = \"fixture\")]\nfn e() {}\n",
            "#[allow(r#unsafe_code, reason = \"fixture\")]\nfn f() {}\n",
            "#[r#expect(unsafe_code, reason = \"fixture\")]\nfn g() {}\n",
            "#[r#warn(unsafe_code)]\nfn h() {}\n",
            "#[r#cfg_attr(all(), allow(unsafe_code, reason = \"fixture\"))]\nfn i() {}\n",
            "#[cfg_attr(all(), r#allow(r#unsafe_code, reason = \"fixture\"))]\nfn j() {}\n",
            "#[r#deny(r#unsafe_code)]\nfn k() { let r#unsafe = 1; let _ = r#unsafe; }\n",
        );
        assert_eq!(
            scan_unsafe(source).expect("parses"),
            UnsafeFindings {
                code: vec![1, 3, 5, 7],
                allowances: vec![9, 11, 13, 15, 17, 19],
            }
        );
    }

    #[test]
    fn a_crate_root_forbids_only_with_an_unconditional_top_level_attribute() {
        let forbids = |source: &str| declares(source, "forbid").expect("parses");
        assert!(forbids("//! docs\n#![forbid(unsafe_code)]\nfn a() {}\n"));
        assert!(forbids("#![forbid(missing_docs, unsafe_code)]\n"));
        assert!(forbids("#!/usr/bin/env run\n#![forbid(unsafe_code)]\n"));
        assert!(!forbids("#![deny(unsafe_code)]\n"));
        assert!(!forbids("#![cfg_attr(all(), forbid(unsafe_code))]\n"));
        assert!(!forbids("mod inner { #![forbid(unsafe_code)] }\n"));
        assert!(!forbids("#[forbid(unsafe_code)]\nfn a() {}\n"));
        assert!(!forbids(
            "#![forbid(missing_docs)]\n// #![forbid(unsafe_code)]\n"
        ));
        assert!(declares("#![deny(unsafe_code)]\n", "deny").expect("parses"));
        // The compiler honours a raw level or lint name.
        assert!(forbids("#![r#forbid(unsafe_code)]\n"));
        assert!(forbids("#![forbid(r#unsafe_code)]\n"));
        // Another tool's lint of the same name forbids nothing.
        assert!(!forbids("#![forbid(clippy::unsafe_code)]\n"));
        assert!(!forbids("#![forbid(missing_docs, clippy::unsafe_code)]\n"));
        assert!(!forbids("#![forbid(unsafe_code::nested)]\n"));
        // Only a bracketed attribute with a parenthesized lint list.
        assert!(!forbids("#![forbid[unsafe_code]]\n"));
        assert!(!forbids("#![forbid{unsafe_code}]\n"));
        assert!(!forbids("//! docs\n#!{forbid(unsafe_code)}\n"));
        assert!(!forbids("//! docs\n#!(forbid(unsafe_code))\n"));
    }
}
