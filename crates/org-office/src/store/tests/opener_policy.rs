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

// Split only outside single-line TOML strings and nested arrays/tables. Multiline strings and unfamiliar
// key/level forms are intentionally unsupported, rather than silently ignored.
fn split(source: &str, separator: char) -> Vec<&str> {
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    let mut depth = 0;
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
        } else if c == '[' || c == '{' {
            depth += 1;
        } else if c == ']' || c == '}' {
            depth -= 1;
        } else if c == separator && depth == 0 {
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
        } else if name[0] == "reason" {
            string(item);
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
    let mut args = values.iter().flat_map(|s| s.split_whitespace());
    while let Some(arg) = args.next() {
        // Compiler, linker, extern and argument-file options cannot enter through
        // a lint-only grammar, regardless of spelling or Cargo config location.
        let (flag, lint) = [
            "-A",
            "-W",
            "-D",
            "-F",
            "--allow",
            "--warn",
            "--deny",
            "--forbid",
            "--force-warn",
        ]
        .into_iter()
        .find_map(|flag| {
            let lint = if arg == flag {
                Some(args.next().expect("missing lint flag argument"))
            } else if flag.len() == 2 {
                arg.strip_prefix(flag)
                    .map(|rest| rest.strip_prefix('=').unwrap_or(rest))
            } else {
                arg.strip_prefix(&format!("{flag}="))
            };
            lint.map(|lint| (flag, lint))
        })
        .expect("unsupported non-lint rustflag");
        assert!(
            !lint.is_empty() && !lint.starts_with('-'),
            "invalid lint flag argument"
        );
        if ["-A", "-W", "--allow", "--warn", "--force-warn"].contains(&flag) {
            assert!(
                !lint.split(',').any(weakening),
                "Cargo weakens the opener lint with {lint}"
            );
        }
    }
}
// Cargo manifests have extensible package/dependency metadata. Cargo configuration
// is a separate input: only the tables and keys modeled by this fence are admitted.
pub(super) fn cargo_config(source: &str) {
    // No tracked Cargo configuration currently sets an environment variable:
    // the reviewed environment-name allowlist is empty.
    let mut section = Vec::new();
    for line in statements(source) {
        if line.starts_with('[') {
            let header = line
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
                .expect("unsupported Cargo config table");
            section = key(header);
            let names = section.iter().map(String::as_str).collect::<Vec<_>>();
            assert!(
                matches!(
                    names.as_slice(),
                    ["build"] | ["target"] | ["target", _] | ["env"]
                ),
                "unknown Cargo config table"
            );
            continue;
        }
        let fields = split(&line, '=');
        let [name, value] = fields.as_slice() else {
            panic!("unsupported Cargo config key");
        };
        let mut path = section.clone();
        path.extend(key(name));
        let names = path.iter().map(String::as_str).collect::<Vec<_>>();
        match names.as_slice() {
            ["build", "rustflags"] | ["target", _, "rustflags"] => flags(value),
            _ => panic!("unknown Cargo config key"),
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

const OPENERS: &[&str] = &[
    "open",
    "open_with_flags",
    "open_in_memory",
    "open_in_memory_with_flags",
    "open_with_flags_and_vfs",
    "open_in_memory_with_flags_and_vfs",
];
pub(super) fn clippy(source: &str) {
    let mut methods = None;
    for record in statements(source) {
        assert!(methods.is_none(), "duplicate Clippy configuration record");
        let (name, value) = record
            .split_once('=')
            .expect("invalid Clippy configuration record");
        assert_eq!(
            key(name),
            ["disallowed-methods"],
            "unknown Clippy configuration key"
        );
        let array = value
            .trim()
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .expect("expected Clippy methods array");
        let entries: std::collections::BTreeSet<_> = split(array, ',')
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(|entry| {
                if entry.starts_with('{') {
                    table_field(entry, "path", &["path", "reason"])
                } else {
                    string(entry)
                }
            })
            .collect();
        methods = Some(entries);
    }
    let methods = methods.expect("absent Clippy configuration record");
    for method in OPENERS {
        assert!(
            methods.contains(&format!("rusqlite::Connection::{method}")),
            "Clippy drops the init fence for {method}"
        );
    }
}

// Standard-library glob/prelude imports and named aliases are opaque to this
// syntax-only policy. Local glob imports and the anonymous trait-import idiom remain usable.
fn imports(tree: &syn::UseTree, standard: bool) {
    match tree {
        syn::UseTree::Path(path) => {
            let name = normalized(&path.ident.to_string());
            let standard = standard || ["std", "core"].contains(&name.as_str());
            assert!(
                !(standard && name == "prelude"),
                "opaque standard prelude import"
            );
            imports(&path.tree, standard);
        }
        syn::UseTree::Name(name) => assert!(
            normalized(&name.ident.to_string()) != "include",
            "code include import"
        ),
        syn::UseTree::Rename(rename) => {
            let name = normalized(&rename.ident.to_string());
            assert!(name != "include", "code include alias");
            let standard = standard || ["std", "core"].contains(&name.as_str());
            assert!(
                !standard || rename.rename == "_",
                "opaque standard named alias"
            );
        }
        syn::UseTree::Glob(_) => assert!(!standard, "opaque standard glob import"),
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                imports(tree, standard);
            }
        }
    }
}
fn extern_import(item: &syn::ItemExternCrate) {
    let standard = ["std", "core"].contains(&normalized(&item.ident.to_string()).as_str());
    assert!(
        !standard || item.rename.as_ref().is_none_or(|(_, name)| name == "_"),
        "opaque standard extern alias"
    );
}

// Every in-scope macro_rules definition is a reviewed source contract. Fixture
// changes require fence review; invocations never inherit definition exceptions.
const MACRO_PINS: &[(&str, &str, &str, &str)] = &[
    (
        "crates/org-host-control/src/secret.rs",
        "",
        "secret_text",
        include_str!("opener_policy_macros/secret_text.tokens"),
    ),
    (
        "crates/org-mailbox/src/testkit/conformance.rs",
        "",
        "__mailbox_conformance_cases",
        include_str!("opener_policy_macros/__mailbox_conformance_cases.tokens"),
    ),
    (
        "crates/org-mailbox/src/testkit/conformance.rs",
        "",
        "mailbox_conformance_tests",
        include_str!("opener_policy_macros/mailbox_conformance_tests.tokens"),
    ),
    (
        "crates/org-mailbox/src/testkit/triage_conformance.rs",
        "",
        "__triage_conformance_cases",
        include_str!("opener_policy_macros/__triage_conformance_cases.tokens"),
    ),
    (
        "crates/org-mailbox/src/testkit/triage_conformance.rs",
        "",
        "triage_conformance_tests",
        include_str!("opener_policy_macros/triage_conformance_tests.tokens"),
    ),
    (
        "crates/org-office/src/rules/command.rs",
        "impl OrgCommand/fn parse/block",
        "read",
        include_str!("opener_policy_macros/read.tokens"),
    ),
    (
        "crates/org-office/src/rules/command.rs",
        "impl OrgCommand/fn method_and_params/block",
        "of",
        include_str!("opener_policy_macros/of.tokens"),
    ),
    (
        "crates/org-office/src/store/work/tests/assembly.rs",
        "",
        "view_queries",
        include_str!("opener_policy_macros/view_queries.tokens"),
    ),
    (
        "crates/org-proto/src/gateway/org_mutations.rs",
        "",
        "mutations",
        include_str!("opener_policy_macros/mutations.tokens"),
    ),
    (
        "crates/org-proto/src/gateway/views.rs",
        "",
        "view_enum",
        include_str!("opener_policy_macros/view_enum.tokens"),
    ),
    (
        "crates/org-proto/src/org.rs",
        "",
        "command",
        include_str!("opener_policy_macros/command.tokens"),
    ),
    (
        "crates/org-proto/src/token.rs",
        "",
        "hex_secret",
        include_str!("opener_policy_macros/hex_secret.tokens"),
    ),
    (
        "crates/org-proto/src/work.rs",
        "",
        "wire_enum",
        include_str!("opener_policy_macros/wire_enum.tokens"),
    ),
    (
        "crates/org-proto/src/work.rs",
        "",
        "write_call",
        include_str!("opener_policy_macros/write_call.tokens"),
    ),
    (
        "crates/org-runtime/src/token.rs",
        "",
        "validated_string",
        include_str!("opener_policy_macros/validated_string.tokens"),
    ),
    (
        "crates/org-socket/src/rights.rs",
        "",
        "cloexec_result",
        include_str!("opener_policy_macros/cloexec_result.tokens"),
    ),
    (
        "crates/org-sync-proto/src/values.rs",
        "",
        "identity",
        include_str!("opener_policy_macros/identity.tokens"),
    ),
    (
        "crates/org-sync-proto/src/values.rs",
        "",
        "sequence",
        include_str!("opener_policy_macros/sequence.tokens"),
    ),
    (
        "crates/org-work/src/call.rs",
        "",
        "work_calls",
        include_str!("opener_policy_macros/work_calls.tokens"),
    ),
];

fn pinned_macro(root: &Path, file: &Path, scope: &str, item: &syn::ItemMacro) -> bool {
    let Some(name) = &item.ident else {
        assert!(
            !item.mac.path.is_ident("macro_rules"),
            "unnamed macro definition"
        );
        return false;
    };
    assert!(
        item.mac.path.is_ident("macro_rules"),
        "pinned macro kind changed"
    );
    let file = file.canonicalize().unwrap();
    let relative = file.strip_prefix(root).unwrap();
    let name = name.to_string();
    let (_, _, _, expected) = MACRO_PINS
        .iter()
        .find(|(path, owner, ident, _)| {
            relative == Path::new(path) && scope == *owner && name == *ident
        })
        .expect("unpinned macro definition");
    let expected: proc_macro2::TokenStream = expected.parse().expect("invalid macro pin");
    assert_eq!(
        item.mac.tokens.to_string(),
        expected.to_string(),
        "pinned macro body changed: {}::{scope}::{name}",
        relative.display()
    );
    true
}

pub(super) fn modules(root: &Path, file: &Path, source: &str) {
    struct Modules<'a> {
        root: &'a Path,
        file: &'a Path,
        source: &'a str,
        scope: Vec<String>,
        depth: usize,
        opaque_definition: bool,
        seen_pins: std::collections::BTreeSet<String>,
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
        fn visit_item(&mut self, item: &'ast syn::Item) {
            assert!(
                !matches!(item, syn::Item::Verbatim(_)),
                "unsupported opaque Rust item"
            );
            syn::visit::visit_item(self, item);
        }
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            imports(&item.tree, false);
            syn::visit::visit_item_use(self, item);
        }
        fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
            extern_import(item);
            syn::visit::visit_item_extern_crate(self, item);
        }

        fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
            let previous = self.opaque_definition;
            let scope = self.scope.join("/");
            self.opaque_definition = pinned_macro(self.root, self.file, &scope, item);
            if self.opaque_definition {
                assert!(
                    self.seen_pins
                        .insert(format!("{scope}/{}", item.ident.as_ref().unwrap())),
                    "duplicate pinned macro definition"
                );
            }
            syn::visit::visit_item_macro(self, item);
            self.opaque_definition = previous;
        }
        fn visit_block(&mut self, block: &'ast syn::Block) {
            self.depth += 1;
            self.scope.push("block".into());
            syn::visit::visit_block(self, block);
            self.scope.pop();
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
                self.scope.push(format!("mod {}", module.ident));
                for item in items {
                    self.visit_item(item);
                }
                self.scope.pop();
                self.depth -= 1;
            }
        }
        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            let header = &self.source[item.impl_token.span.byte_range().start
                ..item.brace_token.span.open().byte_range().start];
            let header: proc_macro2::TokenStream = header.parse().expect("unsupported impl owner");
            self.scope.push(header.to_string());
            syn::visit::visit_item_impl(self, item);
            self.scope.pop();
        }
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            self.scope.push(format!("fn {}", item.sig.ident));
            syn::visit::visit_item_fn(self, item);
            self.scope.pop();
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            self.scope.push(format!("fn {}", item.sig.ident));
            syn::visit::visit_impl_item_fn(self, item);
            self.scope.pop();
        }
        fn visit_macro(&mut self, node: &'ast syn::Macro) {
            check_macro(node, self.opaque_definition);
        }
    }
    let root = root.canonicalize().unwrap();
    let syntax = syn::parse_file(source).expect("unsupported Rust source");
    let mut visitor = Modules {
        root: &root,
        file,
        source,
        scope: Vec::new(),
        depth: 0,
        opaque_definition: false,
        seen_pins: std::collections::BTreeSet::new(),
    };
    visitor.visit_file(&syntax);
    let file = file.canonicalize().unwrap();
    let relative = file.strip_prefix(&root).unwrap();
    let expected: std::collections::BTreeSet<_> = MACRO_PINS
        .iter()
        .filter(|(path, _, _, _)| relative == Path::new(path))
        .map(|(_, scope, name, _)| format!("{scope}/{name}"))
        .collect();
    assert_eq!(
        visitor.seen_pins, expected,
        "missing pinned macro definition"
    );
}

fn check_macro(node: &syn::Macro, opaque_definition: bool) {
    // Macro output is opaque: never allow it to add an unscanned source file.
    fn attribute(stream: proc_macro2::TokenStream, opaque_definition: bool) {
        let mut iter = stream.into_iter();
        let Some(proc_macro2::TokenTree::Ident(name)) = iter.next() else {
            assert!(opaque_definition, "opaque macro attribute name");
            return;
        };
        match name.to_string().trim_start_matches("r#") {
            "path" => panic!("macro path attributes are outside the module inventory"),
            "cfg_attr" => {
                let Some(proc_macro2::TokenTree::Group(args)) = iter.next() else {
                    panic!("unsupported macro cfg_attr");
                };
                assert!(
                    args.delimiter() == proc_macro2::Delimiter::Parenthesis
                        && iter.next().is_none(),
                    "unsupported macro cfg_attr"
                );
                let mut args = args.stream().into_iter();
                // A predicate can contain metavariables; only output attributes matter.
                assert!(
                    args.by_ref().any(
                        |t| matches!(t, proc_macro2::TokenTree::Punct(p) if p.as_char() == ',')
                    ),
                    "missing macro cfg_attr output"
                );
                let mut child = proc_macro2::TokenStream::new();
                for token in args {
                    if matches!(&token, proc_macro2::TokenTree::Punct(p) if p.as_char() == ',') {
                        attribute(child, opaque_definition);
                        child = proc_macro2::TokenStream::new();
                    } else {
                        child.extend([token]);
                    }
                }
                if !child.is_empty() {
                    attribute(child, opaque_definition);
                }
            }
            _ => {}
        }
    }
    fn tokens(stream: proc_macro2::TokenStream, opaque_definition: bool, definition: bool) {
        let mut iter = stream.into_iter().peekable();
        while let Some(token) = iter.next() {
            match token {
                proc_macro2::TokenTree::Ident(name)
                    if name.to_string().trim_start_matches("r#") == "macro_rules"
                        && matches!(iter.peek(), Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == '!') =>
                {
                    panic!("nested macro definition");
                }
                proc_macro2::TokenTree::Ident(name) if name == "use" => {
                    let tree: proc_macro2::TokenStream = iter.by_ref().take_while(|t| !matches!(t, proc_macro2::TokenTree::Punct(p) if p.as_char() == ';')).collect();
                    let tree: syn::UseTree = syn::parse2(tree).expect("opaque macro use import");
                    imports(&tree, false);
                }
                proc_macro2::TokenTree::Ident(name)
                    if name == "extern"
                        && matches!(iter.peek(), Some(proc_macro2::TokenTree::Ident(next)) if next == "crate") =>
                {
                    iter.next();
                    let body: proc_macro2::TokenStream = iter.by_ref().take_while(|t| !matches!(t, proc_macro2::TokenTree::Punct(p) if p.as_char() == ';')).collect();
                    let item: syn::ItemExternCrate =
                        syn::parse_str(&format!("extern crate {body};"))
                            .expect("opaque macro extern import");
                    extern_import(&item);
                }
                proc_macro2::TokenTree::Punct(mark) if mark.as_char() == '$' => {
                    let mut look = iter.clone();
                    let target = look.next();
                    let literal_crate = matches!(&target, Some(proc_macro2::TokenTree::Ident(name)) if name == "crate");
                    let mut literal_path = false;
                    let mut call = match target {
                                Some(proc_macro2::TokenTree::Ident(_)) => look.next(),
                                Some(proc_macro2::TokenTree::Group(_)) => {
                                    look.find(|token| matches!(token, proc_macro2::TokenTree::Punct(p) if ['*','+','?'].contains(&p.as_char())))
                                        .and_then(|_| look.next())
                                }
                                _ => None,
                            };
                    if definition {
                        assert!(
                            !matches!(&call, Some(proc_macro2::TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Bracket),
                            "opaque macro attribute introducer"
                        );
                    }
                    while matches!(&call, Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == ':')
                    {
                        match (look.next(), look.next()) {
                            (
                                Some(proc_macro2::TokenTree::Punct(separator)),
                                Some(proc_macro2::TokenTree::Ident(_)),
                            ) if separator.as_char() == ':' => {
                                literal_path = true;
                                call = look.next();
                            }
                            _ => break,
                        }
                    }
                    if literal_crate && literal_path {
                        call = None;
                    }
                    assert!(
                        !matches!(call, Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == '!'),
                        "opaque macro call name"
                    );
                }
                proc_macro2::TokenTree::Punct(mark) if mark.as_char() == '#' => {
                    if matches!(iter.peek(), Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == '!')
                    {
                        iter.next();
                    }
                    match iter.peek() {
                        Some(proc_macro2::TokenTree::Group(group))
                            if group.delimiter() == proc_macro2::Delimiter::Bracket =>
                        {
                            attribute(group.stream(), opaque_definition);
                        }
                        _ => panic!("unstructured macro attribute introducer"),
                    }
                }
                proc_macro2::TokenTree::Ident(name)
                    if name.to_string().trim_start_matches("r#") == "include" =>
                {
                    assert!(
                        !matches!(iter.peek(),Some(proc_macro2::TokenTree::Punct(p)) if p.as_char()=='!'),
                        "code include is outside the module inventory"
                    )
                }
                proc_macro2::TokenTree::Group(group) => {
                    tokens(group.stream(), opaque_definition, definition);
                }
                _ => {}
            }
        }
    }
    assert!(
        !node
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident.to_string().trim_start_matches("r#") == "include"),
        "code include is outside the module inventory"
    );
    tokens(
        node.tokens.clone(),
        opaque_definition,
        node.path.is_ident("macro_rules"),
    );
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
        "exception must name exactly one function: {name}"
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
        // Closures belong to this function; every nested item has a separate owner.
        fn visit_item(&mut self, _: &'ast syn::Item) {}
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
    config(
        "[build]\nrustflags = ['-Adead_code','-Wclippy::too_many_arguments','-Dclippy::all','-Fwarnings','--deny=clippy::style']",
    );
    config("[env]\nCARGO_ENCODED_RUSTFLAGS = \"-D\\u001fclippy::all\"");
}

#[test]
fn sqlcipher_clippy_policy_requires_actual_complete_method_entries() {
    // Independent names: removing one production entry must lose a negative case.
    let names = [
        "open",
        "open_with_flags",
        "open_in_memory",
        "open_in_memory_with_flags",
        "open_with_flags_and_vfs",
        "open_in_memory_with_flags_and_vfs",
    ];
    let paths: Vec<_> = names
        .iter()
        .map(|name| format!("rusqlite::Connection::{name}"))
        .collect();
    let strings: Vec<_> = paths.iter().map(|path| format!("\"{path}\"")).collect();
    let valid = format!("disallowed-methods = [{}]", strings.join(","));
    clippy(&valid);
    clippy(&format!(
        "# comment\n'disallowed-methods' = [\n{}, # comment\n]",
        strings.join(",")
    ));
    clippy(&format!(
        "\"disallowed-\\u006dethods\" = [{}]",
        paths
            .iter()
            .map(|path| format!("{{ 'path' = '{path}', reason = \"a, b # c\" }}"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    clippy(&valid.replace(']', ", 'unrelated::method']"));
    for (missing, name) in names.iter().enumerate() {
        let entries = strings
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != missing)
            .map(|(_, entry)| entry.as_str())
            .collect::<Vec<_>>()
            .join(",");
        assert!(
            std::panic::catch_unwind(|| clippy(&format!("disallowed-methods = [{entries}]")))
                .is_err(),
            "accepted missing {}",
            name
        );
    }
    for source in [
        String::new(),
        " \n\t".into(),
        "# only a comment\n".into(),
        "disallowed-methods = []".into(),
        format!("# {}\ndisallowed-methods = []", valid),
        format!("{valid}\n{valid}"),
        valid.replace("disallowed-methods", "unknown"),
        valid.replace("disallowed-methods", "methods.disallowed-methods"),
        "disallowed-methods = true".into(),
        "disallowed-methods = [1]".into(),
        "disallowed-methods = [{reason = 'missing path'}]".into(),
        valid.replace(
            &strings[0],
            &format!("{{path={},path={}}}", strings[0], strings[0]),
        ),
        valid.replace(
            &strings[0],
            &format!("{{path={},surprise=true}}", strings[0]),
        ),
        valid.replace(&strings[0], &format!("{{path={},reason=true}}", strings[0])),
        format!("{valid}\nunknown = true"),
        format!("{valid}\n[other]"),
    ] {
        assert!(
            std::panic::catch_unwind(|| clippy(&source)).is_err(),
            "accepted {source}"
        );
    }
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
        "macro_rules! ordinary { () => { #[derive(Debug)] struct Data; #[doc = \"path\"] fn call() {} }; }",
        "macro_rules! ordinary { ($predicate:meta) => { #[cfg_attr($predicate, derive(Debug), doc = \"data\",)] struct Data; }; }",
        "macro_rules! ordinary { () => { #[cfg_attr(any(path), cfg_attr(test, derive(Debug)))] struct Data; }; }",
        "other! { #[doc = \"#[path=rogue.inc]\"] struct Data; #![doc = \"inner\"] mod nested {} }",
        "use std::{fs, io::Write as _}; use super::*; use local::prelude::*; extern crate std;",
        "use local::prelude::v1; use local::Thing as Alias; extern crate local as alias;",
        "other! { $x[0]; }",
        "macro_rules! ordinary { ($($x:tt)*) => { $($x)*(0); }; }",
        "macro_rules! ordinary { ($x:expr) => { $x(0); }; }",
        "macro_rules! ordinary { ($m:ident) => { let _: $m::Type; }; }",
        "macro_rules! ordinary { () => { use std::fmt::Write as _; use super::*; extern crate core; }; }",
        "macro_rules! ordinary { ($m:ident, $p:tt) => { literal!($m, $p); }; }",
        "macro_rules! ordinary { ($($m:tt)*) => { literal!($($m)*); }; }",
    ] {
        if source.starts_with("macro_rules!") {
            let item: syn::ItemMacro = syn::parse_str(source).unwrap();
            check_macro(&item.mac, false);
            assert!(std::panic::catch_unwind(|| super::check_rust_file(root, &file, source)).is_err());
        } else { super::check_rust_file(root, &file, source); }
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
        "use std::include as include_source; include_source!(\"rogue.inc\");",
        "#[path=\"ok.rs\"] use std::fs;",
        "#[path=\"ok.rs\"] extern crate std;",
        "macro_rules! wrapper { ($($m:ident)::*,$p:tt) => { $($m)::*!($p); }; }",
        "macro_rules! wrapper { ($m:ident) => { std::$m!(\"rogue.inc\"); }; }",
        "macro_rules! wrapper { ($m:ident) => { $m::literal!(\"rogue.inc\"); }; }",
        "macro_rules! wrapper { ($m:ident) => { $m::include!(\"rogue.inc\"); }; }",
        "other! { # (path) [path] path data; }",
        "other! { # (doc = \"ok\") }",
        "other! { #doc }", "other! { #!doc }", "other! { # }",
        "macro_rules! wrapper { ($x:tt) => { $x[path = \"rogue.inc\"] mod child; }; }",
        "other! { macro_rules! wrapper { ($x:tt) => { $x[path = \"rogue.inc\"] mod child; }; } }",
        "macro_rules! wrapper { ($hash:tt,$attr:ident,$p:tt) => { $hash[$attr=$p] mod child; }; } wrapper!(#,path,\"rogue.inc\");",
        "macro_rules! wrapper { ($( $hash:tt $rest:tt )*) => { $($hash)*[path=\"rogue.inc\"] mod child; }; } wrapper!(#[doc=\"ok\"]);",
        "macro_rules! wrapper { ($( $hash:tt $rest:tt )+) => { $($hash)+[path=\"rogue.inc\"] mod child; }; } wrapper!(#[doc=\"ok\"]);",
        "macro_rules! wrapper { ($( $hash:tt $rest:tt )?) => { $($hash)?[path=\"rogue.inc\"] mod child; }; } wrapper!(#[doc=\"ok\"]);",
        "use exported::include;", "use exported::include as x;",
        "use std::fs as files;", "use core as c;", "use std::prelude::v1;",
        "use std::{self, {r#include as include_source}};",
        "use {std::{include as x}, local::Thing};",
        "use std::*;", "use core::prelude::*;", "use std::prelude::v1 as p;",
        "use std as s;", "extern crate std as s;", "extern crate r#core as c;",
        "other! { use std::include as x; }", "other! { use std::*; }",
        "other! { extern crate std as s; }",
        "macro_rules! wrapper { ($m:ident, $p:tt) => { $m!($p); }; }",
        "macro_rules! wrapper { ($m:ident, $p:tt) => { $m!{$p} }; }",
        "macro_rules! wrapper { ($m:ident, $p:tt) => { $m![$p]; }; }",
        "macro_rules! wrapper { ($($m:tt)*) => { $($m)*!(\"rogue.inc\"); }; }",
        "macro_rules! wrapper { ($($m:tt),*) => { $($m),*!(\"rogue.inc\"); }; }",
        "macro_rules! wrapper { () => { #[path=\"rogue.inc\"] mod child; }; }",
        "macro_rules! wrapper { () => { #[r#path=\"ok.rs\"] mod child; }; }",
        "macro_rules! wrapper { ($predicate:meta) => { #[cfg_attr($predicate, path=\"ok.rs\")] mod child; }; }",
        "other! { #[cfg_attr(unix, doc=\"ok\", path=\"rogue.inc\")] mod child; }",
        "other! { #[cfg_attr(unix, path=\"rogue.inc\", doc=\"ok\")] mod child; }",
        "other! { #[cfg_attr(unix, cfg_attr(test, path=\"ok.rs\"))] mod child; }",
        "other! { #![path=\"rogue.inc\"] mod child; }",
        "macro_rules! wrapper { ($attr:meta) => { #[$attr] mod child; }; }",
        "macro_rules! wrapper { ($($attr:tt)*) => { #[$($attr)*] mod child; }; }",
        "macro_rules! wrapper { ($($attr:tt)*) => { #[cfg_attr(test, $($attr)*)] mod child; }; }",
        "macro_rules! wrapper { ($attr:meta) => { #[cfg_attr(test, $attr)] mod child; }; }",
        "other! { #[cfg_attr] mod child; }",
        "other! { #[cfg_attr{test,doc=\"ok\"}] mod child; }",
        "other! { #[cfg_attr(test, doc=\"ok\") extra] mod child; }",
        "other! { #[cfg_attr(test)] mod child; }",
        "not valid Rust",
    ] {
        if source.starts_with("macro_rules!") {
            let syntax = syn::parse_file(source).unwrap();
            let syn::Item::Macro(item) = &syntax.items[0] else { unreachable!() };
            assert!(std::panic::catch_unwind(|| check_macro(&item.mac, false)).is_err(), "token policy accepted {source}");
        }
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
        format!("fn connect(){{static CHILD:std::sync::LazyLock<()> = std::sync::LazyLock::new(||{{{attr}let x=op();}});}}"),
        format!("fn connect(){{const CHILD:() = {{{attr}let x=op();}};}}"),
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

#[test]
fn sqlcipher_reserved_crate_macro_paths_remain_literal() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let results = [
        "crates/org-mailbox/src/testkit/conformance.rs",
        "crates/org-mailbox/src/testkit/triage_conformance.rs",
    ]
    .map(|relative| {
        let file = root.join(relative);
        let source = std::fs::read_to_string(&file).unwrap();
        (
            relative,
            std::panic::catch_unwind(|| modules(&root, &file, &source)).is_ok(),
        )
    });
    assert!(results.iter().all(|(_, allowed)| *allowed), "{results:?}");
    let temporary = tempfile::TempDir::new().unwrap();
    let root = temporary.path();
    let file = root.join("lib.rs");
    std::fs::write(&file, "").unwrap();
    let literal: syn::ItemMacro =
        syn::parse_str("macro_rules! wrapper { () => { $crate::literal::call!(); }; }").unwrap();
    check_macro(&literal.mac, false);
    for source in [
        "macro_rules! wrapper { ($m:ident) => { $crate::$m!(); }; }",
        "macro_rules! wrapper { ($m:ident) => { $crate::literal::$m!(); }; }",
        "macro_rules! wrapper { () => { $crate::include!(\"rogue.inc\"); }; }",
        "macro_rules! wrapper { () => { $crate!(); }; }",
    ] {
        assert!(
            std::panic::catch_unwind(|| modules(root, &file, source)).is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn sqlcipher_macro_definition_pins_match_exact_keys_and_token_bodies() {
    use syn::spanned::Spanned;
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // Independent expected inventory: every record must retain its positive.
    for (path, scope, name) in [
        ("crates/org-host-control/src/secret.rs", "", "secret_text"),
        (
            "crates/org-mailbox/src/testkit/conformance.rs",
            "",
            "__mailbox_conformance_cases",
        ),
        (
            "crates/org-mailbox/src/testkit/conformance.rs",
            "",
            "mailbox_conformance_tests",
        ),
        (
            "crates/org-mailbox/src/testkit/triage_conformance.rs",
            "",
            "__triage_conformance_cases",
        ),
        (
            "crates/org-mailbox/src/testkit/triage_conformance.rs",
            "",
            "triage_conformance_tests",
        ),
        (
            "crates/org-office/src/rules/command.rs",
            "impl OrgCommand/fn parse/block",
            "read",
        ),
        (
            "crates/org-office/src/rules/command.rs",
            "impl OrgCommand/fn method_and_params/block",
            "of",
        ),
        (
            "crates/org-office/src/store/work/tests/assembly.rs",
            "",
            "view_queries",
        ),
        (
            "crates/org-proto/src/gateway/org_mutations.rs",
            "",
            "mutations",
        ),
        ("crates/org-proto/src/gateway/views.rs", "", "view_enum"),
        ("crates/org-proto/src/org.rs", "", "command"),
        ("crates/org-proto/src/token.rs", "", "hex_secret"),
        ("crates/org-proto/src/work.rs", "", "wire_enum"),
        ("crates/org-proto/src/work.rs", "", "write_call"),
        ("crates/org-runtime/src/token.rs", "", "validated_string"),
        ("crates/org-socket/src/rights.rs", "", "cloexec_result"),
        ("crates/org-sync-proto/src/values.rs", "", "identity"),
        ("crates/org-sync-proto/src/values.rs", "", "sequence"),
        ("crates/org-work/src/call.rs", "", "work_calls"),
    ] {
        let source = std::fs::read_to_string(repository.join(path)).unwrap();
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &source).unwrap();
        modules(&root, &file, &source);
        struct Find<'ast, 'name> {
            name: &'name str,
            found: Vec<&'ast syn::ItemMacro>,
        }
        impl<'ast> Visit<'ast> for Find<'ast, '_> {
            fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
                if item.ident.as_ref().is_some_and(|ident| ident == self.name) {
                    self.found.push(item);
                }
                syn::visit::visit_item_macro(self, item);
            }
        }
        let syntax = syn::parse_file(&source).unwrap();
        let mut find = Find {
            name,
            found: Vec::new(),
        };
        find.visit_file(&syntax);
        assert_eq!(find.found.len(), 1);
        let item = find.found[0];
        assert!(pinned_macro(&root, &file, scope, item));
        let mut pending = vec![(item.mac.tokens.clone(), 0)];
        let mut nested_identifier = None;
        let mut nested_literal = None;
        while let Some((tokens, depth)) = pending.pop() {
            for token in tokens {
                match token {
                    proc_macro2::TokenTree::Group(group) => {
                        pending.push((group.stream(), depth + 1));
                    }
                    proc_macro2::TokenTree::Ident(ident) if depth > 0 => {
                        nested_identifier = Some(ident.span().byte_range());
                    }
                    proc_macro2::TokenTree::Literal(literal) if depth > 0 => {
                        nested_literal = Some(literal.span().byte_range());
                    }
                    _ => {}
                }
            }
        }
        assert!(nested_identifier.is_some());
        assert_eq!(
            nested_literal.is_none(),
            [
                "__mailbox_conformance_cases",
                "mailbox_conformance_tests",
                "__triage_conformance_cases",
                "triage_conformance_tests",
                "view_queries"
            ]
            .contains(&name)
        );
        for (token, replacement) in [
            (nested_identifier, "changed_nested_identifier"),
            (nested_literal, "\"changed nested literal\""),
        ] {
            if let Some(token) = token {
                let definition = item.span().byte_range();
                let mut changed = source[definition.clone()].to_owned();
                changed.replace_range(
                    token.start - definition.start..token.end - definition.start,
                    replacement,
                );
                let changed = syn::parse_str(&changed).unwrap();
                assert!(
                    std::panic::catch_unwind(|| pinned_macro(&root, &file, scope, &changed))
                        .is_err()
                );
            }
        }
        let leaked = format!("{source}\nfn unknown_scope(){{ unknown!{{#[$meta] Value}}; }}");
        assert!(std::panic::catch_unwind(|| modules(&root, &file, &leaked)).is_err());
        let range = item.span().byte_range();
        let definition = &source[range.clone()];
        for changed in [
            source.replacen(definition, &format!("{definition} {definition}"), 1),
            source.replacen(definition, &format!("mod moved {{ {definition} }}"), 1),
            source.replacen(definition, "", 1),
        ] {
            assert!(
                std::panic::catch_unwind(|| modules(&root, &file, &changed)).is_err(),
                "accepted changed {path}::{name}"
            );
        }
        let moved_scope = format!("mod moved/{scope}");
        assert!(
            std::panic::catch_unwind(|| pinned_macro(&root, &file, &moved_scope, item)).is_err()
        );
        let moved = root.join(format!("moved_{name}.rs"));
        std::fs::write(&moved, &source).unwrap();
        assert!(std::panic::catch_unwind(|| pinned_macro(&root, &moved, scope, item)).is_err());
        let mut changed: syn::ItemMacro =
            syn::parse_str(&format!("macro_rules! {name} {{ {} ; }}", item.mac.tokens)).unwrap();
        assert!(std::panic::catch_unwind(|| pinned_macro(&root, &file, scope, &changed)).is_err());
        changed.mac.tokens = item.mac.tokens.clone();
        changed.ident = Some(syn::parse_str(&format!("other_{name}")).unwrap());
        assert!(std::panic::catch_unwind(|| pinned_macro(&root, &file, scope, &changed)).is_err());
        changed.ident = Some(syn::parse_str(name).unwrap());
        changed.mac.path = syn::parse_str("other_rules").unwrap();
        assert!(std::panic::catch_unwind(|| pinned_macro(&root, &file, scope, &changed)).is_err());
    }
}

#[test]
fn sqlcipher_macro_allowlist_refuses_reassembly_and_generated_definitions() {
    let dir = tempfile::TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    std::fs::write(&file, "").unwrap();
    for source in [
        r#"macro_rules! wrapper { ($m:ident,$bang:tt,$p:tt)=>{$m $bang ($p);}; } wrapper!(include,!,"rogue.inc");"#,
        r#"macro_rules! wrapper { ($hash:tt $rest:tt;$attr:tt)=>{$hash $attr mod child;}; } wrapper!(#[doc="ok"];[path="rogue.inc"]);"#,
        r#"macro_rules! wrapper { ($root:ident,$name:ident,$bang:tt,$p:tt)=>{$root::$name $bang ($p);}; } wrapper!(std,include,!,"rogue.inc");"#,
        "other! { macro_rules! hidden { () => {} } }",
        "other! { { other! { macro_rules! hidden { () => {} } } } }",
        "other! { r#macro_rules! hidden { () => {} } }",
        "macro_rules! ordinary { () => { struct Data; }; }",
    ] {
        assert!(
            std::panic::catch_unwind(|| modules(dir.path(), &file, source)).is_err(),
            "accepted {source}"
        );
    }
    for source in [
        "other! { struct Data; }",
        "// macro_rules! hidden {}\nconst TEXT:&str=\"macro_rules! hidden {}\";",
    ] {
        modules(dir.path(), &file, source);
    }
}

#[test]
fn sqlcipher_macro_invocations_reject_path_attributes() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let file = root.join("lib.rs");
    std::fs::write(&file, "").unwrap();
    for name in [
        "secret_text",
        "validated_string",
        "command",
        "wire_enum",
        "write_call",
        "mutations",
        "unlisted",
    ] {
        modules(
            root,
            &file,
            &format!("{name}! {{ #[doc = \"ordinary\"] Value }}"),
        );
        for attribute in [
            "#[path = \"x.rs\"]",
            "#[cfg_attr(unix, path = \"x.rs\")]",
            "#[cfg_attr(unix, cfg_attr(test, path = \"x.rs\"))]",
            "#[$meta]",
        ] {
            let source = format!("{name}! {{ {attribute} Value }}");
            assert!(
                std::panic::catch_unwind(|| modules(root, &file, &source)).is_err(),
                "accepted {source}"
            );
        }
    }
}

#[test]
fn sqlcipher_policy_option_guards_pair_present_and_absent_inputs() {
    assert_eq!(split("a,'b,c',d", ','), ["a", "'b,c'", "d"]);
    assert_eq!(
        statements("k='x#y' # comment\nk2='z'"),
        ["k='x#y'", "k2='z'"]
    );
    assert_eq!(string("'ok'"), "ok");
    for value in ["plain", "'open", "close'"] {
        assert!(std::panic::catch_unwind(|| string(value)).is_err());
    }
    assert_eq!(table_field("{ level='deny' }", "level", &["level"]), "deny");
    for value in ["level='deny'", "{ level }", "{}"] {
        assert!(std::panic::catch_unwind(|| table_field(value, "level", &["level"])).is_err());
    }
    flags("'-Ddead_code'");
    flags("['-Ddead_code']");
    for value in ["['--cfg=feature'", "'-A'"] {
        assert!(std::panic::catch_unwind(|| flags(value)).is_err());
    }
    config("name='ok'");
    config("[package]\nname='ok'");
    for value in ["[package", "no_assignment"] {
        assert!(std::panic::catch_unwind(|| config(value)).is_err());
    }
    let paths = OPENERS
        .iter()
        .map(|name| format!("'rusqlite::Connection::{name}'"))
        .collect::<Vec<_>>()
        .join(",");
    clippy(&format!("disallowed-methods=[{paths}]"));
    for value in ["", "no_assignment", "disallowed-methods=true"] {
        assert!(std::panic::catch_unwind(|| clippy(value)).is_err());
    }
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("input.rs");
    std::fs::write(&file, "").unwrap();
    modules(temp.path(), &file, "mod external;");
    modules(temp.path(), &file, "mod inline { fn allowed() {} }");
    assert!(std::panic::catch_unwind(|| modules(
        temp.path(),
        &file,
        "mod inline { include!(\"x\"); }"
    ))
    .is_err());
    for source in [
        "other! { #[doc=\"ok\"] struct Data; }",
        "other! { #[cfg_attr(test,doc=\"ok\")] struct Data; }",
    ] {
        let tree = syn::parse_file(source).unwrap();
        let syn::Item::Macro(item) = &tree.items[0] else {
            panic!("fixture must be a macro invocation")
        };
        check_macro(&item.mac, false);
    }
    for source in [
        "other! { #[] struct Data; }",
        "other! { #[cfg_attr] struct Data; }",
    ] {
        let tree = syn::parse_file(source).unwrap();
        let syn::Item::Macro(item) = &tree.items[0] else {
            panic!("fixture must be a macro invocation")
        };
        assert!(std::panic::catch_unwind(|| check_macro(&item.mac, false)).is_err());
    }
}

#[test]
fn sqlcipher_policy_directions_preserve_grammar_and_opaque_data() {
    assert_eq!(split(r#""a\",b",c"#, ','), [r#""a\",b""#, "c"]);
    assert_eq!(split(r#"'a\',b"#, ','), [r#"'a\'"#, "b"]);
    assert_eq!(
        statements(r##"name="a\"#b" # comment"##),
        [r##"name="a\"#b""##]
    );
    assert_eq!(statements(r#"name='a\' # comment"#), [r#"name='a\'"#]);
    assert_eq!(table_field("{level='deny',}", "level", &["level"]), "deny");
    assert!(
        std::panic::catch_unwind(|| table_field("{level.extra='deny'}", "level", &["level"]))
            .is_err()
    );
    flags("'--allow dead_code'");
    config("[env]\nRUSTFLAGS.value='-Dclippy::all'");
    for value in [
        "[package..metadata]\nname='ok'",
        "[package.bad key]\nname='ok'",
        "[package.!]\nname='ok'",
        "[build]\nrustflags=['a''b']",
        "one=]\ntwo=[",
        "name='unterminated",
        "[profile.dev]\nrustflags=['--cfg=ok']",
        "[env]\nRUSTFLAGS.other='-Dclippy::all'",
        "[package}",
    ] {
        assert!(
            std::panic::catch_unwind(|| config(value)).is_err(),
            "accepted {value}"
        );
    }
    for value in [
        "name='unterminated",
        "flags=[\n'-Aclippy::disallowed_methods'",
    ] {
        assert!(
            std::panic::catch_unwind(|| statements(value)).is_err(),
            "accepted incomplete statement {value}"
        );
    }
    for value in [
        "[['-Aclippy::disallowed_methods'],'--cfg=ok']",
        "['--cfg=ok',['-Aclippy::disallowed_methods']]",
    ] {
        assert!(
            std::panic::catch_unwind(|| flags(value)).is_err(),
            "accepted nested flag array {value}"
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("input.rs");
    std::fs::write(&file, "").unwrap();
    modules(temp.path(), &file, "extern crate std as _;");
    modules(temp.path(), &file, "extern crate std;");
    modules(temp.path(), &file, "#[clippy::anything] mod inline {}");
    std::fs::write(temp.path().join("no_extension"), "").unwrap();
    assert!(std::panic::catch_unwind(|| modules(
        temp.path(),
        &file,
        "#[path=\"no_extension\"] mod external;"
    ))
    .is_err());
    for source in [
        "stringify!(macro_rules, data);",
        "stringify!(macro_rules data);",
        "stringify!(pretend crate std as named;);",
        "stringify!(extern value std as named;);",
        "stringify!($root : ! tail ! ());",
        "stringify!(include, data);",
        "stringify!($);",
        "stringify!($m);",
        "stringify!($(x));",
        "stringify!($($x)*);",
        "stringify!($m:);",
        "stringify!($m::);",
        "stringify!($m::literal);",
    ] {
        let tree = syn::parse_file(source).unwrap();
        let syn::Item::Macro(item) = &tree.items[0] else {
            panic!("fixture must be a macro invocation")
        };
        check_macro(&item.mac, false);
    }
    for source in [
        "stringify!(extern crate local + data;);",
        "stringify!($root ! : harmless);",
        "stringify!($crate!());",
        "stringify!(# , [doc=\"ok\"]);",
        "stringify!(#);",
        "stringify!($m!());",
        "stringify!($($x)*!());",
        "stringify!($m::literal!());",
    ] {
        let tree = syn::parse_file(source).unwrap();
        let syn::Item::Macro(item) = &tree.items[0] else {
            panic!("fixture must be a macro invocation")
        };
        assert!(
            std::panic::catch_unwind(|| check_macro(&item.mac, false)).is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn sqlcipher_macro_metadata_distinguishes_reviewed_and_opaque_inputs() {
    let syntax = syn::parse_file("other! { #[$attribute] struct Data; }").unwrap();
    let syn::Item::Macro(item) = &syntax.items[0] else {
        panic!("macro fixture")
    };
    check_macro(&item.mac, true);
    assert!(std::panic::catch_unwind(|| check_macro(&item.mac, false)).is_err());
    for source in [
        r#"other! { #[cfg_attr(target_os = "linux", doc = "ok")] struct Data; }"#,
        "other! { #[cfg_attr(unix, doc = \"ok\")] struct Data; }",
        "other! { #[cfg_attr(all(unix), derive(Debug), doc = \"ok\")] struct Data; }",
    ] {
        let syntax = syn::parse_file(source).unwrap();
        let syn::Item::Macro(item) = &syntax.items[0] else {
            panic!("macro fixture")
        };
        check_macro(&item.mac, false);
    }
}

#[test]
fn sqlcipher_reserved_definition_name_refuses_aliased_invocations() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("input.rs");
    std::fs::write(&file, "").unwrap();
    modules(
        temp.path(),
        &file,
        "use crate::identity as ordinary; ordinary!{Boundary}",
    );
    assert!(std::panic::catch_unwind(|| modules(
        temp.path(),
        &file,
        "use crate::identity as macro_rules; macro_rules!{Boundary}"
    ))
    .is_err());
}

#[test]
fn sqlcipher_cargo_config_pairs_strict_grammar_and_flag_refusals() {
    for input in [
        "[build]",
        "[target]",
        "[env]",
        "build.rustflags='-Dclippy::all'",
        "target.'cfg(target_os = \"linux\")'.rustflags=['-Dclippy::all']",
        "[target.'cfg(unix)']\nrustflags='-Dclippy::all'",
    ] {
        cargo_config(input);
    }
    for input in [
        "[build] = 'extra'",
        "build.rustflags",
        "[unknown]",
        "include=['outside.toml']",
        "[env]\nANY='data'",
        "[target.'cfg(unix)']\nrustflags='-Aclippy::disallowed_methods'",
        "[target.x86_64-unknown-linux-gnu]\nrustflags='--cap-lints=allow'",
    ] {
        assert!(
            std::panic::catch_unwind(|| cargo_config(input)).is_err(),
            "accepted {input}"
        );
    }
}

#[test]
fn sqlcipher_argument_files_and_opaque_items_fail_closed() {
    for source in [
        "[build]\nrustflags = '@absolute.args'",
        "[target.'cfg(unix)']\nrustflags = ['@absolute.args']",
        "build.rustflags = ['@absolute.args']",
    ] {
        assert!(std::panic::catch_unwind(|| cargo_config(source)).is_err());
    }
    for source in [
        "[build]\nrustflags = '-Dclippy::all'",
        "[target.'cfg(unix)']\nrustflags = ['-Dclippy::all']",
        "build.rustflags = ['-Dclippy::all']",
    ] {
        cargo_config(source);
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("lib.rs");
    std::fs::write(&file, "").unwrap();
    for source in ["macro hidden() {}", "pub macro hidden($x:expr) {$x}"] {
        assert!(std::panic::catch_unwind(|| modules(dir.path(), &file, source)).is_err());
    }
    modules(dir.path(), &file, "pub struct Supported;");
}

pub(super) fn manifest_paths(
    scope: &super::opener_scope::SourceScope,
    manifest: &Path,
    source: &str,
) {
    fn entry(
        scope: &super::opener_scope::SourceScope,
        base: &Path,
        path: Vec<String>,
        value: &str,
    ) {
        assert!(
            !matches!(path.first().map(String::as_str), Some("patch" | "replace")),
            "unsupported Cargo override"
        );
        if let Some(inline) = value.trim().strip_prefix('{') {
            let inline = inline
                .strip_suffix('}')
                .expect("unsupported inline Cargo table");
            for field in split(inline, ',').into_iter().filter(|s| !s.is_empty()) {
                let (name, value) = field
                    .split_once('=')
                    .expect("unsupported inline Cargo field");
                let mut child = path.clone();
                child.extend(key(name));
                entry(scope, base, child, value);
            }
        } else {
            let names = path.iter().map(String::as_str).collect::<Vec<_>>();
            if matches!(
                names.as_slice(),
                ["workspace", "members" | "default-members" | "exclude"]
            ) {
                let values = value
                    .trim()
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .expect("literal workspace entries required");
                for value in split(values, ',').into_iter().filter(|s| !s.is_empty()) {
                    let path = string(value);
                    assert!(
                        !path.contains(['*', '?', '[', ']']),
                        "workspace globs unsupported"
                    );
                    scope.require(&base.join(path).join("Cargo.toml"));
                }
            } else if names == ["package", "workspace"] {
                scope.require(&base.join(string(value)).join("Cargo.toml"));
            } else if matches!(
                names.as_slice(),
                [
                    "dependencies"
                        | "dev-dependencies"
                        | "build-dependencies"
                        | "dev_dependencies"
                        | "build_dependencies",
                    _,
                    _
                ] | ["workspace", "dependencies", _, _]
                    | [
                        "target",
                        _,
                        "dependencies"
                            | "dev-dependencies"
                            | "build-dependencies"
                            | "dev_dependencies"
                            | "build_dependencies",
                        _,
                        _
                    ]
            ) {
                match names.last().copied() {
                    Some("path") => {
                        scope.require(&base.join(string(value)).join("Cargo.toml"));
                    }
                    Some("git") => {
                        let url = string(value);
                        assert!(
                            !url.bytes()
                                .any(|byte| byte.is_ascii_control() || byte == b' '),
                            "unsupported Git URL whitespace"
                        );
                        assert!(
                            !url.to_ascii_lowercase().starts_with("file:"),
                            "local Git sources unsupported"
                        );
                    }
                    _ => {}
                }
            }
        }
    }
    let base = manifest.parent().unwrap();
    let mut section = Vec::new();
    for line in statements(source) {
        if line.starts_with('[') {
            let header = line
                .strip_prefix("[[")
                .and_then(|s| s.strip_suffix("]]"))
                .or_else(|| line.strip_prefix('[').and_then(|s| s.strip_suffix(']')))
                .expect("unsupported Cargo table");
            section = key(header);
            assert!(
                !matches!(
                    section.first().map(String::as_str),
                    Some("patch" | "replace")
                ),
                "unsupported Cargo override"
            );
        } else {
            let (name, value) = line.split_once('=').expect("unsupported Cargo assignment");
            let mut path = section.clone();
            path.extend(key(name));
            entry(scope, base, path, value);
        }
    }
}
