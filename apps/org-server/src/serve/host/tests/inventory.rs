//! Source-backed inventory: actual Call definitions, parser reachability and server dispatch.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use syn::visit::Visit;

// X-02 rev15 §4.4 names these explicitly; they do not have Rust Call types yet.
const UNTYPED: &[&str] = &[
    "chat.read_as_owner",
    "chat.mute",
    "chat.mark_read",
    "chat.pin",
    "dm.open",
];

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg") && a.meta.require_list().unwrap().tokens.to_string() == "test"
    })
}

// Follow the Rust module graph, including inline modules, rather than a file-name allowlist.
fn modules(path: &Path, inspect: &mut impl FnMut(&syn::File)) {
    let file = syn::parse_file(&std::fs::read_to_string(path).unwrap()).unwrap();
    inspect(&file);
    let directory = if matches!(path.file_stem().unwrap().to_str().unwrap(), "lib" | "mod") {
        path.parent().unwrap().to_path_buf()
    } else {
        path.with_extension("")
    };
    fn descend(items: &[syn::Item], directory: &Path, inspect: &mut impl FnMut(&syn::File)) {
        for item in items {
            if let syn::Item::Mod(module) = item {
                if test_only(&module.attrs) {
                    continue;
                }
                if let Some((_, items)) = &module.content {
                    descend(items, &directory.join(module.ident.to_string()), inspect);
                } else {
                    let explicit = module.attrs.iter().find(|a| a.path().is_ident("path"));
                    let path = if let Some(attribute) = explicit {
                        let syn::Meta::NameValue(value) = &attribute.meta else {
                            panic!("module path");
                        };
                        let syn::Expr::Lit(value) = &value.value else {
                            panic!("module path literal");
                        };
                        let syn::Lit::Str(value) = &value.lit else {
                            panic!("module path string");
                        };
                        directory.join(value.value())
                    } else {
                        let path = directory.join(format!("{}.rs", module.ident));
                        if path.exists() {
                            path
                        } else {
                            directory.join(module.ident.to_string()).join("mod.rs")
                        }
                    };
                    modules(&path, inspect);
                }
            }
        }
    }
    descend(&file.items, &directory, inspect);
}

#[derive(Default)]
struct Declarations(BTreeMap<String, String>);
impl Declarations {
    fn add(&mut self, call: String, method: String) {
        assert!(
            self.0.insert(call.clone(), method).is_none(),
            "duplicate Call type {call}"
        );
    }
}
impl<'ast> Visit<'ast> for Declarations {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if item
            .trait_
            .as_ref()
            .is_some_and(|(_, path, _)| path.segments.last().unwrap().ident == "Call")
        {
            let syn::Type::Path(call) = &*item.self_ty else {
                panic!("Call type");
            };
            let method = item
                .items
                .iter()
                .find_map(|item| match item {
                    syn::ImplItem::Const(item) if item.ident == "METHOD" => {
                        let syn::Expr::Lit(expr) = &item.expr else {
                            panic!("METHOD must be a literal");
                        };
                        let syn::Lit::Str(method) = &expr.lit else {
                            panic!("METHOD must be a string");
                        };
                        Some(method.value())
                    }
                    _ => None,
                })
                .expect("Call has METHOD");
            self.add(call.path.segments.last().unwrap().ident.to_string(), method);
        }
    }
    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        let generator = item.mac.path.segments.last().unwrap().ident.to_string();
        if generator == "macro_rules" && item.mac.tokens.to_string().contains("impl Call") {
            assert!(
                matches!(
                    item.ident.as_ref().unwrap().to_string().as_str(),
                    "command" | "write_call"
                ),
                "new Call generator needs inventory support"
            );
        }
        if matches!(generator.as_str(), "command" | "write_call") {
            use syn::parse::Parser;
            let (call, method) = (|input: syn::parse::ParseStream<'_>| {
                input.call(syn::Attribute::parse_outer)?;
                let call: syn::Ident = input.parse()?;
                input.parse::<syn::Token![,]>()?;
                let method: syn::LitStr = input.parse()?;
                input.parse::<proc_macro2::TokenStream>()?;
                Ok((call.to_string(), method.value()))
            })
            .parse2(item.mac.tokens.clone())
            .unwrap();
            self.add(call, method);
        }
    }
}

struct Dispatch<'a> {
    declarations: &'a BTreeMap<String, String>,
    methods: BTreeSet<String>,
    candidates: BTreeSet<String>,
}
fn method_expression(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Field(field) => {
            matches!(&field.member, syn::Member::Named(name) if name == "method")
        }
        syn::Expr::MethodCall(call) if call.method == "as_str" => method_expression(&call.receiver),
        syn::Expr::Reference(reference) => method_expression(&reference.expr),
        syn::Expr::Paren(paren) => method_expression(&paren.expr),
        _ => false,
    }
}
impl Dispatch<'_> {
    fn pattern(&mut self, pattern: &syn::Pat) {
        match pattern {
            syn::Pat::Lit(syn::ExprLit {
                lit: syn::Lit::Str(value),
                ..
            }) => {
                self.methods.insert(value.value());
            }
            syn::Pat::Or(pattern) => {
                for case in &pattern.cases {
                    self.pattern(case);
                }
            }
            _ => {}
        }
    }

    fn candidate(&mut self, value: &str) {
        if !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_' || b == b'.')
        {
            self.candidates.insert(value.into());
        }
    }
    fn tokens(&mut self, tokens: proc_macro2::TokenStream) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Group(group) => self.tokens(group.stream()),
                proc_macro2::TokenTree::Literal(literal) => {
                    if let Ok(value) = syn::parse_str::<syn::LitStr>(&literal.to_string()) {
                        self.candidate(&value.value());
                    }
                }
                _ => {}
            }
        }
    }
}
impl<'ast> Visit<'ast> for Dispatch<'_> {
    fn visit_lit_str(&mut self, value: &'ast syn::LitStr) {
        self.candidate(&value.value());
    }
    fn visit_macro(&mut self, value: &'ast syn::Macro) {
        self.tokens(value.tokens.clone());
        if value.path.is_ident("matches") {
            use syn::parse::Parser;
            let (expression, pattern) = (|input: syn::parse::ParseStream<'_>| {
                let expression: syn::Expr = input.parse()?;
                input.parse::<syn::Token![,]>()?;
                let pattern = input.call(syn::Pat::parse_multi_with_leading_vert)?;
                input.parse::<proc_macro2::TokenStream>()?;
                Ok((expression, pattern))
            })
            .parse2(value.tokens.clone())
            .unwrap();
            if method_expression(&expression) {
                self.pattern(&pattern);
            }
        }
    }
    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        for (subject, value) in [
            (&*binary.left, &*binary.right),
            (&*binary.right, &*binary.left),
        ] {
            if method_expression(subject) {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(value),
                    ..
                }) = value
                {
                    self.methods.insert(value.value());
                }
            }
        }
        syn::visit::visit_expr_binary(self, binary);
    }
    fn visit_expr_match(&mut self, expression: &'ast syn::ExprMatch) {
        if method_expression(&expression.expr) {
            for arm in &expression.arms {
                self.pattern(&arm.pat);
            }
        }
        syn::visit::visit_expr_match(self, expression);
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if matches!(call.method.to_string().as_str(), "is" | "params") {
            if let Some(arguments) = &call.turbofish {
                for argument in &arguments.args {
                    if let syn::GenericArgument::Type(syn::Type::Path(path)) = argument {
                        if let Some(method) = self
                            .declarations
                            .get(&path.path.segments.last().unwrap().ident.to_string())
                        {
                            self.methods.insert(method.clone());
                        }
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn inventory() -> (BTreeSet<String>, BTreeSet<String>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut declarations = Declarations::default();
    modules(&root.join("crates/org-proto/src/lib.rs"), &mut |file| {
        declarations.visit_file(file)
    });
    let mut dispatch = Dispatch {
        declarations: &declarations.0,
        methods: BTreeSet::new(),
        candidates: BTreeSet::new(),
    };
    modules(&root.join("apps/org-server/src/lib.rs"), &mut |file| {
        dispatch.visit_file(file)
    });
    let shared = shared();
    let candidates: BTreeSet<_> = declarations
        .0
        .values()
        .chain(dispatch.candidates.iter())
        .cloned()
        .collect();
    for method in &candidates {
        let request = signed(&shared, method, Value::Null);
        // Exercise the real registries. A new OrgCommand or WorkCall is discovered even if
        // no direct request.is::<Call>() appears in org-server.
        if !matches!(OrgCommand::parse(&request), Ok(None))
            || !matches!(WorkCall::parse(&request), Ok(None))
        {
            dispatch.methods.insert(method.clone());
        }
        let response = super::super::super::super::answer(
            &request,
            &shared,
            &mut Binding::Member {
                id: OWNER_ID.into(),
                with_app_token: true,
            },
        );
        if !matches!(response.outcome, org_proto::Outcome::Error(error) if error.code == ErrorCode::UnknownMethod)
        {
            dispatch.methods.insert(method.clone());
        }
    }
    let mut declared: BTreeSet<_> = declarations.0.values().cloned().collect();
    declared.extend(UNTYPED.iter().map(|method| (*method).into()));
    (declared, dispatch.methods)
}

#[test]
fn operation_table_covers_declared_and_served_methods_in_both_directions() {
    let (declared, served) = inventory();
    let rows = operations();
    let names: BTreeSet<_> = rows.iter().map(|row| row.method.clone()).collect();
    assert_eq!(names.len(), rows.len(), "duplicate policy row");
    let universe: BTreeSet<_> = declared.union(&served).cloned().collect();
    assert_eq!(names, universe, "missing or stale operation policy row");
    for row in rows {
        let expected = match row.method.as_str() {
            "hello" => HostPolicy::Hello,
            "msg.bind" | "msg.wait" | "status.set" => HostPolicy::InvalidRequest,
            "subscribe" | "member.set_run_mode" => HostPolicy::Refuse,
            _ if served.contains(&row.method) => HostPolicy::Serve,
            _ if [
                "work.",
                "task.",
                "bug.",
                "checklist.",
                "project.",
                "area.",
                "msg.",
                "chat.",
                "dm.",
            ]
            .iter()
            .any(|prefix| row.method.starts_with(prefix)) =>
            {
                HostPolicy::Unserved
            }
            _ => HostPolicy::Refuse,
        };
        assert_eq!(row.policy, expected, "incorrect policy for {}", row.method);
    }
}

#[test]
fn each_unserved_row_has_exact_error_after_authority_and_each_refused_row_is_fixed() {
    let shared = shared();
    let ctx = context();
    for row in operations() {
        if !matches!(row.policy, HostPolicy::Unserved | HostPolicy::Refuse) {
            continue;
        }
        let mut request = signed(&shared, &row.method, json!({}));
        let response = answer(&shared, &ctx, &request);
        if row.policy == HostPolicy::Unserved {
            assert_eq!(
                response["error"]["code"], "unknown_method",
                "{}: {response}",
                row.method
            );
            assert_eq!(
                response["error"]["message"],
                format!("this office does not know `{}`", row.method)
            );
        } else {
            refused(response, "not_authorized");
        }
        request.principal.as_mut().unwrap()["grant"]["office_id"] = "wrong".into();
        sign(&mut request.principal.as_mut().unwrap()["grant"]);
        refused(answer(&shared, &ctx, &request), "not_found");
        request.principal = None;
        refused(answer(&shared, &ctx, &request), "not_authorized");
    }
}

#[test]
fn known_operation_names_do_not_grant_suffixes_or_case_aliases() {
    let shared = shared();
    let ctx = context();
    for row in operations() {
        for method in [
            format!("{}.future", row.method),
            row.method.to_ascii_uppercase(),
            format!("{}\0", row.method),
        ] {
            refused(
                answer(&shared, &ctx, &signed(&shared, &method, json!({}))),
                "not_authorized",
            );
        }
    }
}
