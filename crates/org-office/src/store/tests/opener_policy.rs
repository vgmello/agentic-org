//! Regression policy for in-repository accidental lint/module weakening.
//! This deliberately supports a closed Cargo policy grammar; unfamiliar policy
//! forms fail closed. Environment-only flags and user/ancestor config are outside it.
use std::path::{Path, PathBuf};
use syn::{parse::Parser, visit::Visit};

const GROUPS: &[&str] = &[
    "warnings",
    "renamed_and_removed_lints",
    "clippy",
    "clippy::all",
    "clippy::restriction",
    "clippy::pedantic",
    "clippy::nursery",
    "clippy::cargo",
    "clippy::style",
    "clippy::correctness",
    "clippy::suspicious",
    "clippy::complexity",
    "clippy::perf",
];
fn normalized(lint: &str) -> String {
    lint.replace("r#", "").replace('-', "_")
}
pub(super) fn lint_group(lint: &str) -> bool {
    GROUPS.contains(&normalized(lint).as_str())
}
fn weakening(lint: &str) -> bool {
    lint_group(lint)
        || matches!(
            normalized(lint).as_str(),
            "clippy::disallowed_method" | "clippy::disallowed_methods"
        )
}

// Split only outside single-line TOML strings. Multiline strings and unfamiliar
// key/level forms are intentionally unsupported, rather than silently ignored.
fn split(source: &str, separator: char) -> Vec<&str> {
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    let mut parts = Vec::new();
    for (i, c) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == separator {
            parts.push(source[start..i].trim());
            start = i + c.len_utf8();
        }
    }
    assert!(quote.is_none() && !escaped, "unsupported Cargo string");
    parts.push(source[start..].trim());
    parts
}
fn string(source: &str) -> String {
    let source = source.trim();
    if source.starts_with('"') {
        serde_json::from_str(source).expect("unsupported Cargo basic string")
    } else {
        let value = source
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .expect("expected Cargo string");
        assert!(!value.contains('\''), "unsupported Cargo literal string");
        value.to_owned()
    }
}
fn key(source: &str) -> Vec<String> {
    split(source, '.')
        .into_iter()
        .map(|part| {
            if part.starts_with(['\'', '"']) {
                string(part)
            } else {
                assert!(
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
                    "unsupported Cargo key"
                );
                part.to_owned()
            }
        })
        .collect()
}
fn statements(config: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut pending = String::new();
    let mut depth = 0;
    for line in config.lines() {
        let mut quote = None;
        let mut escaped = false;
        for (i, c) in line.char_indices() {
            if escaped {
                pending.push(c);
                escaped = false;
                continue;
            }
            if quote == Some('"') && c == '\\' {
                pending.push(c);
                escaped = true;
                continue;
            }
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if c == '#' {
                break;
            } else if c == '"' || c == '\'' {
                assert!(
                    !line[i..].starts_with(&c.to_string().repeat(3)),
                    "unsupported multiline Cargo string"
                );
                quote = Some(c);
            } else if c == '[' || c == '{' {
                depth += 1;
            } else if c == ']' || c == '}' {
                depth -= 1;
                assert!(depth >= 0, "unbalanced Cargo statement");
            }
            pending.push(c);
        }
        assert!(
            quote.is_none() && !escaped,
            "unsupported multiline Cargo string"
        );
        if depth == 0 {
            if !pending.trim().is_empty() {
                result.push(pending.trim().to_owned());
            }
            pending.clear();
        } else {
            pending.push(' ');
        }
    }
    assert!(
        pending.trim().is_empty() && depth == 0,
        "incomplete Cargo statement"
    );
    result
}
fn table_field(source: &str, wanted: &str, allowed: &[&str]) -> String {
    let body = source
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .expect("unsupported Cargo policy table");
    let mut value = None;
    for field in split(body, ',').into_iter().filter(|s| !s.is_empty()) {
        let (name, item) = field
            .split_once('=')
            .expect("unsupported Cargo policy field");
        let name = key(name);
        assert!(
            name.len() == 1 && allowed.contains(&name[0].as_str()),
            "unknown Cargo policy field"
        );
        if name[0] == wanted {
            assert!(value.is_none(), "duplicate Cargo policy field");
            value = Some(string(item));
        } else if name[0] == "priority" {
            item.trim()
                .parse::<i64>()
                .expect("unsupported Cargo lint priority");
        } else {
            assert!(
                ["true", "false"].contains(&item.trim()),
                "unsupported Cargo environment option"
            );
        }
    }
    value.expect("missing Cargo policy field")
}
fn lint_level(value: &str) -> String {
    let level = if value.trim().starts_with('{') {
        table_field(value, "level", &["level", "priority"])
    } else {
        string(value)
    };
    assert!(
        ["allow", "warn", "deny", "forbid"].contains(&level.as_str()),
        "unsupported Cargo lint level"
    );
    level
}
fn flags(value: &str) {
    let value = value.trim();
    let values = if let Some(array) = value.strip_prefix('[') {
        split(
            array
                .strip_suffix(']')
                .expect("unsupported rustflags array"),
            ',',
        )
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(string)
        .collect::<Vec<_>>()
    } else {
        vec![string(value)]
    };
    let args = values
        .iter()
        .flat_map(|s| s.split_whitespace())
        .collect::<Vec<_>>();
    for (i, arg) in args.iter().enumerate() {
        assert!(
            *arg != "--cap-lints" && !arg.starts_with("--cap-lints="),
            "Cargo caps the opener lint"
        );
        for prefix in ["-A", "-W", "--allow", "--warn", "--force-warn"] {
            let lint = if *arg == prefix {
                Some(*args.get(i + 1).expect("missing lint flag argument"))
            } else if prefix.len() == 2 {
                arg.strip_prefix(prefix).map(|s| s.trim_start_matches('='))
            } else {
                arg.strip_prefix(&format!("{prefix}="))
            };
            if let Some(lint) = lint {
                assert!(
                    !lint.split(',').any(weakening),
                    "Cargo weakens the opener lint with {lint}"
                );
            }
        }
    }
}
pub(super) fn config(source: &str) {
    let mut section = Vec::new();
    for line in statements(source) {
        if line.starts_with('[') {
            let header = line
                .strip_prefix("[[")
                .and_then(|s| s.strip_suffix("]]"))
                .or_else(|| line.strip_prefix('[').and_then(|s| s.strip_suffix(']')))
                .expect("unsupported Cargo table");
            section = key(header);
            continue;
        }
        let (name, value) = line.split_once('=').expect("unsupported Cargo assignment");
        let mut path = section.clone();
        path.extend(key(name));
        let names = path.iter().map(String::as_str).collect::<Vec<_>>();
        let lint = names
            .strip_prefix(&["lints"])
            .or_else(|| names.strip_prefix(&["workspace", "lints"]));
        if let Some(lint) = lint {
            if lint == ["workspace"] {
                assert_eq!(value.trim(), "true", "unsupported Cargo lint inheritance");
                continue;
            }
            let [tool, name, rest @ ..] = lint else {
                panic!("unsupported inline Cargo lints");
            };
            assert!(["rust", "clippy"].contains(tool), "unknown Cargo lint tool");
            if rest == ["priority"] {
                value
                    .trim()
                    .parse::<i64>()
                    .expect("unsupported Cargo lint priority");
                continue;
            }
            assert!(
                rest.is_empty() || rest == ["level"],
                "unsupported Cargo lint shape"
            );
            let level = lint_level(value);
            let name = if *tool == "clippy" {
                format!("clippy::{name}")
            } else {
                name.to_string()
            };
            assert!(
                !weakening(&name) || ["deny", "forbid"].contains(&level.as_str()),
                "Cargo weakens the opener lint with {name}"
            );
        } else if names.last() == Some(&"rustflags") {
            assert!(
                matches!(
                    names.as_slice(),
                    ["build", "rustflags"] | ["target", _, "rustflags"]
                ),
                "unsupported Cargo rustflags scope"
            );
            flags(value);
        } else if let ["env", name, rest @ ..] = names.as_slice() {
            assert!(
                *name != "CLIPPY_CONF_DIR",
                "Cargo overrides CLIPPY_CONF_DIR"
            );
            if *name == "RUSTFLAGS" || (*name).starts_with("CARGO_") && name.ends_with("_RUSTFLAGS")
            {
                assert!(
                    rest.is_empty() || rest == ["value"],
                    "unsupported Cargo flag environment shape"
                );
                let value = if value.trim().starts_with('{') {
                    table_field(value, "value", &["value", "force", "relative"])
                } else {
                    string(value)
                };
                flags(&serde_json::to_string(&value.replace('\u{1f}', " ")).unwrap());
            }
        } else {
            assert!(
                !matches!(
                    names.as_slice(),
                    ["workspace"] | ["build"] | ["target"] | ["target", _] | ["env"]
                ),
                "unsupported inline Cargo policy"
            );
        }
    }
}

pub(super) fn modules(root: &Path, file: &Path, source: &str) {
    struct Modules<'a> {
        root: &'a Path,
        file: &'a Path,
        depth: usize,
    }
    impl Modules<'_> {
        fn meta(&self, meta: &syn::Meta, allowed: bool) {
            let name = meta
                .path()
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_default();
            match (name.trim_start_matches("r#"), meta) {
                ("cfg_attr", syn::Meta::List(list)) => {
                    let args =
                        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                            .parse2(list.tokens.clone())
                            .expect("unsupported cfg_attr");
                    for child in args.iter().skip(1) {
                        self.meta(child, allowed);
                    }
                }
                ("path", syn::Meta::NameValue(value)) => {
                    assert!(allowed, "path attributes must be on file-level modules");
                    let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(path),
                        ..
                    }) = &value.value
                    else {
                        panic!("module path must be a literal");
                    };
                    let path = PathBuf::from(path.value());
                    assert!(
                        path.is_relative() && path.extension().is_some_and(|s| s == "rs"),
                        "module path must be a relative .rs file"
                    );
                    let target = self
                        .file
                        .parent()
                        .unwrap()
                        .join(path)
                        .canonicalize()
                        .expect("missing module path");
                    let relative = target
                        .strip_prefix(self.root)
                        .expect("module path escapes repository");
                    let mut prefix = PathBuf::new();
                    for part in relative.components() {
                        prefix.push(part);
                        assert!(
                            !super::skip_policy_directory(&prefix),
                            "module path is skipped by inventory"
                        );
                    }
                    assert!(target.is_file(), "module path is not a file");
                }
                ("path", _) => panic!("unsupported module path attribute"),
                _ => {}
            }
        }
    }
    impl<'ast> Visit<'ast> for Modules<'_> {
        fn visit_block(&mut self, block: &'ast syn::Block) {
            self.depth += 1;
            syn::visit::visit_block(self, block);
            self.depth -= 1;
        }
        fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
            self.meta(&attr.meta, false);
        }
        fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
            for attr in &module.attrs {
                self.meta(&attr.meta, self.depth == 0 && module.content.is_none());
            }
            if let Some((_, items)) = &module.content {
                self.depth += 1;
                for item in items {
                    self.visit_item(item);
                }
                self.depth -= 1;
            }
        }
        fn visit_macro(&mut self, node: &'ast syn::Macro) {
            fn tokens(stream: proc_macro2::TokenStream) {
                let mut iter = stream.into_iter().peekable();
                while let Some(token) = iter.next() {
                    match token {
                        proc_macro2::TokenTree::Ident(name)
                            if name.to_string().trim_start_matches("r#") == "include" =>
                        {
                            assert!(
                                !matches!(iter.peek(),Some(proc_macro2::TokenTree::Punct(p)) if p.as_char()=='!'),
                                "code include is outside the module inventory"
                            )
                        }
                        proc_macro2::TokenTree::Group(group) => tokens(group.stream()),
                        _ => {}
                    }
                }
            }
            assert!(
                !node.path.segments.last().is_some_and(|s| s
                    .ident
                    .to_string()
                    .trim_start_matches("r#")
                    == "include"),
                "code include is outside the module inventory"
            );
            tokens(node.tokens.clone());
        }
    }
    let root = root.canonicalize().unwrap();
    let syntax = syn::parse_file(source).expect("unsupported Rust source");
    Modules {
        root: &root,
        file,
        depth: 0,
    }
    .visit_file(&syntax);
}

pub(super) struct Function<'a> {
    pub body: &'a str,
    pub statements: Vec<&'a str>,
}

pub(super) fn function<'a>(source: &'a str, name: &str) -> Function<'a> {
    use syn::spanned::Spanned;
    struct Functions<'ast, 'name> {
        name: &'name str,
        blocks: Vec<&'ast syn::Block>,
    }
    impl<'ast> Visit<'ast> for Functions<'ast, '_> {
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            if item.sig.ident.to_string().trim_start_matches("r#") == self.name {
                self.blocks.push(&item.block);
            }
            syn::visit::visit_item_fn(self, item);
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            if item.sig.ident.to_string().trim_start_matches("r#") == self.name {
                self.blocks.push(&item.block);
            }
            syn::visit::visit_impl_item_fn(self, item);
        }
    }
    let syntax = syn::parse_file(source).expect("unsupported exception source");
    let mut functions = Functions {
        name,
        blocks: Vec::new(),
    };
    functions.visit_file(&syntax);
    assert_eq!(
        functions.blocks.len(),
        1,
        "exception must name exactly one function"
    );
    let block = functions.blocks[0];
    let open = block.brace_token.span.open().byte_range().end;
    let close = block.brace_token.span.close().byte_range().start;
    struct Statements {
        ranges: Vec<std::ops::Range<usize>>,
    }
    impl<'ast> Visit<'ast> for Statements {
        fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
            self.ranges.push(stmt.span().byte_range());
            syn::visit::visit_stmt(self, stmt);
        }
        // Closures belong to their function; a nested function/module does not.
        fn visit_item_fn(&mut self, _: &'ast syn::ItemFn) {}
        fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {}
        fn visit_impl_item_fn(&mut self, _: &'ast syn::ImplItemFn) {}
    }
    let mut statements = Statements { ranges: Vec::new() };
    statements.visit_block(block);
    Function {
        body: &source[open..close],
        statements: statements
            .ranges
            .into_iter()
            .map(|range| &source[range])
            .collect(),
    }
}

#[test]
fn sqlcipher_config_policy_pairs_allowed_and_weakening_forms() {
    assert!(lint_group("r#warnings"));
    assert!(lint_group("clippy::r#restriction"));
    assert!(!lint_group("r#too_many_arguments"));
    for scope in ["lints", "workspace.lints"] {
        for name in [
            "all",
            "style",
            "restriction",
            "pedantic",
            "nursery",
            "cargo",
            "correctness",
            "suspicious",
            "complexity",
            "perf",
            "disallowed_method",
            "disallowed_methods",
        ] {
            for level in ["allow", "warn", "deny", "forbid"] {
                for value in [
                    format!("\"{level}\""),
                    format!("{{ level = \"{level}\", priority = -1 }}"),
                ] {
                    let source = format!("[{scope}.clippy]\n{name} = {value}");
                    assert_eq!(
                        std::panic::catch_unwind(|| config(&source)).is_ok(),
                        ["deny", "forbid"].contains(&level)
                    );
                }
            }
        }
    }
    for source in [
        "[package.metadata]\nnote = \"clippy::style\"",
        "[package.metadata.'a.b']\nnote = \"escaped \\\"quote\\\"\"",
        "[lints.clippy]\ntoo_many_arguments = \"allow\"",
        "[lints.rust]\nwarnings = \"deny\"",
        "[lints.clippy.style]\npriority = -1\nlevel = \"deny\"",
        "[build]\nrustflags = [\n\"-D\", # unrelated \\\"\n\"clippy::style\",\n]",
        "[target.'cfg(target_os = \"macos\")'.dependencies]\nlibc = \"1\"",
        "# triple quotes are harmless in comments: \"\"\"\n[env]\nOTHER = \"/\"",
        "[[bin]]\nname = 'main'\npath = 'main.rs'",
        "lints.clippy.style = 'deny'",
        "[lints]\nworkspace = true",
        "[env]\nCARGO_SOMETHING = '-Aclippy::style'\nFOO_RUSTFLAGS = '-Aclippy::style'",
        "[\"lints\".'clippy']\n'style' = 'deny'",
        "[env]\nRUSTFLAGS = { value = '-D clippy::all', force = true }",
    ] {
        config(source);
    }
    for source in [
        "[lints.rust]\nwarnings = 'warn'",
        "[workspace.lints.rust]\nrenamed_and_removed_lints = 'allow'",
        "[lints.clippy.style]\nlevel = 'allow'",
        "lints.clippy.style = 'allow'",
    ] {
        assert!(
            std::panic::catch_unwind(|| config(source)).is_err(),
            "accepted {source}"
        );
    }
    for source in [
        "lints = { clippy = { style = 'allow' } }",
        "[lints.clippy]\nstyle = true",
        "[lints]\nworkspace = false",
        "[profile.dev]\nrustflags = ['-Aclippy::style']",
        "[lints.clippy]\nstyle = { level = 'deny', surprise = true }",
        "[lints.clippy]\nstyle = { level = 'deny', priority = true }",
        "[lints.clippy.style]\npriority = 'bad'",
        "[env]\nRUSTFLAGS = { value = '-Dclippy::all', force = 'true' }",
        "[lints.clippy]\nstyle = { priority = -1 }",
        "[lints.clippy]\nstyle = { level = 'deny', level = 'deny' }",
        "[lints.clippy]\nstyle = 'future_level'",
        "[lints.clippy]\ntoo_many_arguments = 'future_level'",
        "[lints.unknown]\nstyle = 'deny'",
        "[lints.clippy.style]\nother = 'deny'",
        "[build]\nrustflags = 1",
        "[build]\nrustflags = ['-A']",
        "[env]\nRUSTFLAGS = { force = true }",
        "[env.RUSTFLAGS]\nforce = true",
        "build = { rustflags = ['-A','clippy::style'] }",
        "target = { unix = { rustflags = ['-A','clippy::style'] } }",
        "env = { CLIPPY_CONF_DIR = '/' }",
        "[package]\ndescription = \"\"\"unsupported multiline\"\"\"",
        "[build]\nrustflags = [",
        "[build]\nrustflags = ]",
        "[build]\nrustflags = 'unterminated",
        "[build]\nno_assignment",
    ] {
        assert!(
            std::panic::catch_unwind(|| config(source)).is_err(),
            "accepted unsupported {source}"
        );
    }
}

#[test]
fn sqlcipher_config_policy_checks_flags_and_environment() {
    for flag in [
        "-A clippy::style",
        "-Aclippy::disallowed_methods",
        "-Aclippy::disallowed-method",
        "-W=clippy::all",
        "--allow=warnings",
        "--warn clippy::restriction",
        "--force-warn clippy::style",
        "--cap-lints allow",
        "--cap-lints=deny",
    ] {
        for section in ["build", "target.'cfg(unix)'"] {
            for value in [
                serde_json::to_string(flag).unwrap(),
                format!(
                    "[{}]",
                    flag.split_whitespace()
                        .map(|s| serde_json::to_string(s).unwrap())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ] {
                let source = format!("[{section}]\nrustflags = {value}");
                assert!(
                    std::panic::catch_unwind(|| config(&source)).is_err(),
                    "accepted {source}"
                );
            }
        }
    }
    for name in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS",
    ] {
        let source = format!("[env]\n{name} = {{ value = '-A clippy::style', force = true }}");
        assert!(std::panic::catch_unwind(|| config(&source)).is_err());
    }
    for source in [
        "[env]\nCLIPPY_CONF_DIR = '/'",
        "[env.CLIPPY_CONF_DIR]\nvalue = '/'",
        "[env]\n'CLIPPY_CONF_DIR' = { value = '/', relative = true }",
        "[env]\nCARGO_ENCODED_RUSTFLAGS = \"-A\\u001fclippy::all\"",
        "[build]\nrustflags = [\"-A\",\"clippy::style,dead_code\"]",
        "[\"lints\".clippy]\n\"\\u0073tyle\" = 'allow'",
    ] {
        assert!(
            std::panic::catch_unwind(|| config(source)).is_err(),
            "accepted {source}"
        );
    }
    config("[build]\nrustflags = ['-Adead_code','-Wclippy::too_many_arguments','-Dclippy::all','-Fwarnings','--deny=clippy::style']");
    config("[env]\nCARGO_ENCODED_RUSTFLAGS = \"-D\\u001fclippy::all\"");
}

#[test]
fn sqlcipher_module_policy_checks_real_target_coverage_and_code_macros() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    for file in [
        "crate/src/lib.rs",
        "crate/src/ok.rs",
        "crate/src/build/ok.rs",
        "crate/tests/core_limit.rs",
        "crate/build/rogue.rs",
        "crate/src/rogue.inc",
    ] {
        let file = root.join(file);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "").unwrap();
    }
    std::fs::create_dir(root.join("crate/src/directory.rs")).unwrap();
    let file = root.join("crate/src/lib.rs");
    for source in [
        "mod normal; mod inline { mod nested; }",
        "#[path=\"ok.rs\"] mod child;",
        "#[path=r#\"ok.rs\"#] mod child;",
        "#[path=\"\\u{6f}k.rs\"] mod child;",
        "#[r#path=\"ok.rs\"] mod child;",
        "#[cfg_attr(unix,path=\"ok.rs\")] mod child;",
        "#[cfg_attr(unix,cfg_attr(test,path=\"ok.rs\"))] mod child;",
        "#[path=\"build/ok.rs\"] mod child;",
        "#[path=\"../tests/core_limit.rs\"] mod child;",
        "// #[path=\"bad.inc\"] mod x;\nconst TEXT:&str=\"include!(bad)\";",
        "const TEXT:&str=include_str!(\"data\"); const BYTES:&[u8]=include_bytes!(\"data\");",
        "other! { include data; myinclude!(\"data\"); }",
    ] {
        super::check_rust_file(root, &file, source);
    }
    for source in [
        "#[path=\"rogue.inc\"] mod child;",
        "#[path=\"../build/rogue.rs\"] mod child;",
        "#[cfg_attr(unix,path=\"../build/rogue.rs\")] mod child;",
        "#[r#path=\"rogue.inc\"] mod child;",
        "#[path=concat!(\"ok\",\".rs\")] mod child;",
        "#[path=\"missing.rs\"] mod child;",
        "#[path=\"directory.rs\"] mod child;",
        "mod inline { #[path=\"ok.rs\"] mod child; }",
        "#[path=\"ok.rs\"] mod inline {}",
        "#[path=\"ok.rs\"] fn not_a_module(){}",
        "fn outer(){ #[path=\"ok.rs\"] mod child; }",
        "impl Host { fn method(){ #[path=\"ok.rs\"] mod child; } }",
        "const VALUE:() = { #[path=\"ok.rs\"] mod child; };",
        "#[path] mod child;",
        "include!(\"rogue.inc\");",
        "std::include!(\"rogue.inc\");",
        "r#include!(\"rogue.inc\");",
        "other! { include!(\"rogue.inc\"); }",
        "macro_rules! wrapper { () => { include!(\"rogue.inc\"); } }",
        "not valid Rust",
    ] {
        assert!(
            std::panic::catch_unwind(|| super::check_rust_file(root, &file, source)).is_err(),
            "accepted {source}"
        );
    }
    let absolute = format!("#[path={:?}] mod child;", root.join("crate/src/ok.rs"));
    assert!(std::panic::catch_unwind(|| super::check_rust_file(root, &file, &absolute)).is_err());
    let outside = tempfile::TempDir::new().unwrap();
    let external = outside.path().join("external.rs");
    std::fs::write(&external, "").unwrap();
    #[cfg(unix)]
    {
        let link = root.join("crate/src/link.rs");
        std::os::unix::fs::symlink(external, link).unwrap();
        assert!(std::panic::catch_unwind(|| modules(
            root,
            &file,
            "#[path=\"link.rs\"] mod child;"
        ))
        .is_err());
    }
}

#[test]
fn sqlcipher_exact_function_policy_checks_free_and_impl_items_without_nested_theft() {
    let attr = "#[allow(clippy::disallowed_methods)]";
    for name in [
        "connect",
        "connection_for_test",
        "sqlcipher_ready",
        "ready",
        "every_parent_pipe_is_cloexec_and_stays_out_of_an_unrelated_exec",
    ] {
        let source = format!("// éé 👩‍💻\nimpl Host {{ fn {name}(&self){{{attr}let x=op();}} }}");
        super::require_exception_statement(&source, name, attr, "letx=op();");
    }
    for source in [
        format!("fn connect(){{{attr}let x=op();}}"),
        format!("impl Host {{ fn connect(&self){{{attr}let x=op();}} }}"),
        format!("impl Host {{ fn r#connect(&self){{{attr}let x=op();}} }}"),
        format!("fn wrongfnconnect(){{}} fn connect(){{{attr}let x=op();}}"),
        format!("fn connect(){{ let probe=||{{{attr}let x=op();}}; }}"),
    ] {
        super::require_exception_statement(&source, "connect", attr, "letx=op();");
    }
    for source in [
        format!("fn wrongfnconnect(){{{attr}let x=op();}}"),
        format!("impl Host {{fn wrongfnconnect(&self){{{attr}let x=op();}}}}"),
        format!("fn connect(){{}} fn other(){{{attr}let x=op();}}"),
        format!("fn connect(){{fn child(){{{attr}let x=op();}}}}"),
        format!("fn connect(){{mod nested{{fn child(){{{attr}let x=op();}}}}}}"),
        format!("fn connect(){{mod nested{{const X:()={{{attr}let x=op();}};}}}}"),
        format!("fn connect(){{impl Host{{fn child(&self){{{attr}let x=op();}}}}}}"),
        format!("fn connect(){{{attr}let x=op();}} impl Host{{fn connect(&self){{}}}}"),
        format!("trait Host{{fn connect(){{{attr}let x=op();}}}}"),
    ] {
        assert!(
            std::panic::catch_unwind(|| super::require_exception_statement(
                &source,
                "connect",
                attr,
                "letx=op();"
            ))
            .is_err(),
            "accepted {source}"
        );
    }
}
