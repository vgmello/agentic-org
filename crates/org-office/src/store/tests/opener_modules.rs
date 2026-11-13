//! Resolve audited module contexts after lexical policy checks on every tracked Rust file.
use super::opener_scope::SourceScope;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use syn::{parse::Parser, visit::Visit};

/// Requires Cargo-validated roots and lexical-policy-approved source. Every reached
/// file must be tracked, regular, confined Rust; unsupported or missing paths panic.
pub(super) fn check(scope: &SourceScope, roots: impl IntoIterator<Item = PathBuf>) {
    struct Modules<'a> {
        directory: PathBuf,
        file_parent: &'a Path,
        pending: &'a mut Vec<(PathBuf, Option<PathBuf>)>,
    }
    fn name(meta: &syn::Meta) -> String {
        meta.path()
            .get_ident()
            .map(|name| name.to_string().trim_start_matches("r#").to_owned())
            .unwrap_or_default()
    }
    fn paths(meta: &syn::Meta, result: &mut Vec<PathBuf>) {
        match (name(meta).as_str(), meta) {
            ("cfg_attr", syn::Meta::List(list)) => {
                let args =
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                        .parse2(list.tokens.clone())
                        .expect("unsupported module cfg_attr");
                for child in args.iter().skip(1) {
                    paths(child, result);
                }
            }
            ("path", syn::Meta::NameValue(value)) => {
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(path),
                    ..
                }) = &value.value
                else {
                    panic!("module path must be literal");
                };
                let path = PathBuf::from(path.value());
                assert!(path.is_relative(), "module path must be relative");
                result.push(path);
            }
            ("path" | "cfg_attr", _) => panic!("unsupported module path attribute"),
            _ => {}
        }
    }
    impl<'ast> Visit<'ast> for Modules<'_> {
        fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
            let ident = module.ident.to_string();
            let ident = ident.trim_start_matches("r#");
            if let Some((_, items)) = &module.content {
                let previous = self.directory.clone();
                self.directory.push(ident);
                for item in items {
                    self.visit_item(item);
                }
                self.directory = previous;
            } else {
                let mut explicit = Vec::new();
                for attr in &module.attrs {
                    paths(&attr.meta, &mut explicit);
                }
                // Lexical policy refuses path attributes in inline modules or blocks.
                for path in explicit {
                    self.pending.push((self.file_parent.join(path), None));
                }
                // Conditional paths may be inactive; only a direct path replaces the default.
                if !module.attrs.iter().any(|attr| name(&attr.meta) == "path") {
                    let candidates = [
                        self.directory.join(format!("{ident}.rs")),
                        self.directory.join(ident).join("mod.rs"),
                    ];
                    let present = candidates
                        .iter()
                        .filter(|path| match std::fs::symlink_metadata(path) {
                            Ok(_) => true,
                            Err(error) => {
                                assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
                                false
                            }
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(present.len(), 1, "missing or ambiguous module {ident}");
                    self.pending
                        .push((present[0].clone(), Some(self.directory.join(ident))));
                }
            }
        }
    }
    let mut pending = roots
        .into_iter()
        .map(|file| (file, None))
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some((file, directory)) = pending.pop() {
        let file = scope.require_rust(&file);
        let directory = directory.unwrap_or_else(|| file.parent().unwrap().to_owned());
        if seen.insert((file.clone(), directory.clone())) {
            let source = std::fs::read_to_string(&file).unwrap();
            let syntax = syn::parse_file(&source).expect("unsupported Rust module source");
            Modules {
                directory,
                file_parent: file.parent().unwrap(),
                pending: &mut pending,
            }
            .visit_file(&syntax);
        }
    }
}
