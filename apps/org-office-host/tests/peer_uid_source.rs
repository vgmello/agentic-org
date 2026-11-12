//! Prevent accidental reintroduction of kernel UID queries in checked-in host Rust source.
//! org-socket owns those queries. Audit-token and PID getsockopt calls stay host-owned.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::visit::Visit;
use syn::{braced, bracketed, parenthesized, token, Ident};

#[derive(Default)]
struct Names(Vec<String>);

impl Parse for Names {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut names = Self::default();
        while !input.is_empty() {
            let nested;
            if input.peek(token::Paren) {
                parenthesized!(nested in input);
                names.0.extend(nested.parse::<Self>()?.0);
            } else if input.peek(token::Brace) {
                braced!(nested in input);
                names.0.extend(nested.parse::<Self>()?.0);
            } else if input.peek(token::Bracket) {
                bracketed!(nested in input);
                names.0.extend(nested.parse::<Self>()?.0);
            } else if input.peek(Ident::peek_any) {
                names
                    .0
                    .push(input.call(Ident::parse_any)?.unraw().to_string());
            } else {
                // Literals are single tokens: never scan strings as code. Descend through
                // all physical groups above, including macro bodies and invocations.
                input.step(|cursor| {
                    cursor
                        .token_tree()
                        .map(|(_, next)| ((), next))
                        .ok_or_else(|| cursor.error("expected a Rust token"))
                })?;
            }
        }
        Ok(names)
    }
}

fn uid_queries(source: &str) -> BTreeSet<String> {
    syn::parse_file(source).expect("host source must parse");
    syn::parse_str::<Names>(source)
        .expect("host source tokens must parse")
        .0
        .into_iter()
        .filter(|name| {
            matches!(
                name.as_str(),
                "getpeereid" | "SO_PEERCRED" | "LOCAL_PEERCRED" | "peer_uid"
            )
        })
        .collect()
}

#[derive(Default)]
struct Calls {
    in_accept: bool,
    shared_accept_checks: usize,
    socket_queries: usize,
    invalid_options: usize,
    accept_order: Vec<&'static str>,
}

impl<'ast> Visit<'ast> for Calls {
    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        let previous = self.in_accept;
        self.in_accept = function.sig.ident.unraw() == "accept_loop";
        syn::visit::visit_item_fn(self, function);
        self.in_accept = previous;
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = &*call.func {
            let names: Vec<_> = function
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.unraw().to_string())
                .collect();
            if self.in_accept && function.qself.is_none() && names == ["org_socket", "peer_allowed"]
            {
                self.shared_accept_checks += 1;
                self.accept_order.push("admission");
            }
            if self.in_accept && function.qself.is_none() {
                match names.as_slice() {
                    [owner, method] if owner == "Accepted" && method == "sample" => {
                        self.accept_order.push("sample");
                    }
                    [owner, method] if owner == "Connection" && method == "new" => {
                        self.accept_order.push("connection");
                    }
                    _ if names == ["thread", "spawn"] || names == ["std", "thread", "spawn"] => {
                        self.accept_order.push("spawn");
                    }
                    _ => {}
                }
            }
            if names.last().is_some_and(|name| name == "getsockopt") {
                self.socket_queries += 1;
                let approved = match call.args.iter().nth(2) {
                    Some(syn::Expr::Path(option)) if option.qself.is_none() => {
                        let names: Vec<_> = option
                            .path
                            .segments
                            .iter()
                            .map(|segment| segment.ident.unraw().to_string())
                            .collect();
                        names == ["libc", "LOCAL_PEERTOKEN"] || names == ["libc", "LOCAL_PEERPID"]
                    }
                    _ => false,
                };
                self.invalid_options += usize::from(!approved);
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if self.in_accept && call.method.unraw() == "spawn" {
            self.accept_order.push("spawn");
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn admission_precedes_setup(calls: &Calls) -> bool {
    let admission = calls
        .accept_order
        .iter()
        .position(|step| *step == "admission");
    ["sample", "connection", "spawn"].iter().all(|setup| {
        let setup = calls.accept_order.iter().position(|step| step == setup);
        matches!((admission, setup), (Some(admission), Some(setup)) if admission < setup)
    })
}

fn invalid_query_report(path: &Path, source: &str, calls: &Calls) -> Option<String> {
    (calls.invalid_options > 0).then(|| {
        // Syntax above decides validity. Numbered excerpts only locate the offending
        // file's query spellings, including aliases and opaque macro bodies.
        let lines: Vec<_> = source
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("getsockopt"))
            .map(|(index, line)| format!("{}:{}: {line}", path.display(), index + 1))
            .collect();
        format!(
            "{}: {} unapproved socket queries\n{}",
            path.display(),
            calls.invalid_options,
            lines.join("\n")
        )
    })
}

fn socket_queries(source: &str) -> Calls {
    let syntax = syn::parse_file(source).expect("host source must parse");
    let names = syn::parse_str::<Names>(source).unwrap().0;
    let mut calls = Calls::default();
    calls.visit_file(&syntax);
    // Macro tokens, aliases and function-pointer references must not hide a query
    // from the option check. Existing audit/PID queries are direct calls.
    let all_queries = names.iter().filter(|name| *name == "getsockopt").count();
    calls.invalid_options += all_queries.saturating_sub(calls.socket_queries);
    calls
}

fn rust_sources(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn duplicated_queries(files: &[PathBuf]) -> Vec<(PathBuf, BTreeSet<String>)> {
    files
        .iter()
        .filter_map(|path| {
            let queries = uid_queries(&fs::read_to_string(path).unwrap());
            (!queries.is_empty()).then(|| (path.clone(), queries))
        })
        .collect()
}

#[test]
fn host_peer_uid_checks_have_one_owner() {
    let mut files = Vec::new();
    rust_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(!files.is_empty(), "host source inventory is absent");
    files.sort();
    let duplicates = duplicated_queries(&files);
    assert!(
        duplicates.is_empty(),
        "host duplicates kernel UID queries instead of using org-socket: {duplicates:?}"
    );
    let mut queries = Vec::new();
    let mut invalid = Vec::new();
    for path in &files {
        let source = fs::read_to_string(path).unwrap();
        let calls = socket_queries(&source);
        if let Some(report) = invalid_query_report(path, &source, &calls) {
            invalid.push(report);
        }
        if calls.shared_accept_checks > 0 {
            assert!(admission_precedes_setup(&calls),
                "{}: admission must precede Accepted::sample, Connection::new and thread spawn: {:?}",
                path.display(), calls.accept_order);
        }
        queries.push(calls);
    }
    assert!(
        invalid.is_empty(),
        "host has unapproved socket queries: {invalid:#?}"
    );
    assert_eq!(
        queries
            .iter()
            .map(|query| query.shared_accept_checks)
            .sum::<usize>(),
        1,
        "host accept must use the single org-socket admission check"
    );
}

#[test]
fn source_guard_recognizes_exact_identifiers_and_macro_bodies() {
    for source in [
        "fn peer_uid() {}",
        "fn r#peer_uid() {}",
        "fn probe() { libc::getpeereid(fd, uid, gid); }",
        "use libc::getpeereid as query;",
        "fn probe() { query(libc::SO_PEERCRED); }",
        "fn probe() { query(libc::LOCAL_PEERCRED); }",
        "macro_rules! query { () => { unsafe { libc::getpeereid(fd, uid, gid) } }; }",
        "fn probe() { query!([libc::SO_PEERCRED]); }",
    ] {
        assert!(!uid_queries(source).is_empty(), "missed {source}");
    }
    for source in [
        "fn peer_uid_backup() {} fn getpeereid_backup() {}",
        "// peer_uid getpeereid SO_PEERCRED\nfn probe() {}",
        "fn probe() { let text = \"peer_uid getpeereid SO_PEERCRED\"; }",
        "fn probe() { let text = r#\"peer_uid getpeereid SO_PEERCRED\"#; }",
        "fn audit_token() { libc::getsockopt(fd, libc::SOL_LOCAL, libc::LOCAL_PEERTOKEN, value, len); }",
        "fn pid() { libc::getsockopt(fd, libc::SOL_LOCAL, libc::LOCAL_PEERPID, value, len); }",
    ] {
        assert!(uid_queries(source).is_empty(), "false match {source}");
    }
}

#[test]
fn socket_query_options_and_admission_pin_are_checked_as_syntax() {
    for option in ["LOCAL_PEERTOKEN", "LOCAL_PEERPID"] {
        let source = format!(
            "fn probe() {{ libc::getsockopt(fd, libc::SOL_LOCAL, libc::{option}, value, len); }}"
        );
        let calls = socket_queries(&source);
        assert_eq!((calls.socket_queries, calls.invalid_options), (1, 0));
    }
    for source in [
        "fn probe() { libc::getsockopt(fd, level, libc::SO_PEERCRED, value, len); }",
        "fn probe() { libc::getsockopt(fd, level, 17, value, len); }",
        "fn probe() { libc::getsockopt(fd, level, other::LOCAL_PEERPID, value, len); }",
        "fn probe() { libc::getsockopt(fd, level, <T as libc>::LOCAL_PEERPID, value, len); }",
        "fn probe() { libc::getsockopt(fd); }",
        "use libc::getsockopt as query;",
        "macro_rules! query { () => { libc::getsockopt(fd, level, 17, value, len) }; }",
    ] {
        assert!(
            socket_queries(source).invalid_options > 0,
            "missed {source}"
        );
    }
    let positive = socket_queries(
        "fn accept_loop() { if !org_socket::peer_allowed(stream, uid) { return; } }",
    );
    assert_eq!(positive.shared_accept_checks, 1);
    assert_eq!(
        socket_queries("fn r#accept_loop() { org_socket::peer_allowed(stream, uid); }")
            .shared_accept_checks,
        1
    );
    for source in [
        "fn wrongfnaccept_loop() { org_socket::peer_allowed(stream, uid); }",
        "fn accept_loop() { wrongorg_socket::peer_allowed(stream, uid); }",
        "fn accept_loop() { org_socket::wrongpeer_allowed(stream, uid); }",
        "fn accept_loop() { <T as org_socket>::peer_allowed(stream, uid); }",
        "fn elsewhere() { org_socket::peer_allowed(stream, uid); }",
        "fn accept_loop() { fn nested() { org_socket::peer_allowed(stream, uid); } }",
    ] {
        assert_eq!(socket_queries(source).shared_accept_checks, 0, "{source}");
    }
}

#[test]
fn source_guard_scans_multiple_files_and_nested_directories() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    fs::write(dir.path().join("good.rs"), "fn allowed() {}").unwrap();
    let bad = dir.path().join("nested/bad.rs");
    fs::write(&bad, "fn probe() { libc::getpeereid(fd, uid, gid); }").unwrap();
    fs::write(dir.path().join("notes.txt"), "getpeereid").unwrap();
    let mut files = Vec::new();
    rust_sources(dir.path(), &mut files);
    assert_eq!(files.len(), 2);
    let duplicates = duplicated_queries(&files);
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].0, bad);
    assert_eq!(duplicates[0].1, BTreeSet::from(["getpeereid".into()]));
}

#[test]
fn admission_order_requires_every_setup_step_after_the_check() {
    let admission = "org_socket::peer_allowed(stream, uid);";
    let setup = [
        "Accepted::sample(stream);",
        "Connection::new(stream);",
        "builder.spawn(work);",
    ];
    let source = |body: &str| format!("fn accept_loop() {{ {body} }}");
    assert!(admission_precedes_setup(&socket_queries(&source(
        &format!("{admission} {}", setup.join(" "))
    ))));
    for spawn in [
        "thread::spawn(work);",
        "std::thread::spawn(work);",
        "builder.r#spawn(work);",
    ] {
        assert!(admission_precedes_setup(&socket_queries(&source(
            &format!("{admission} {} {} {spawn}", setup[0], setup[1])
        ))));
    }
    for index in 0..setup.len() {
        let rest: Vec<_> = setup
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, step)| *step)
            .collect();
        for body in [
            format!("{} {admission} {}", setup[index], rest.join(" ")),
            format!("{admission} {}", rest.join(" ")),
        ] {
            assert!(
                !admission_precedes_setup(&socket_queries(&source(&body))),
                "accepted {body}"
            );
        }
    }
    assert!(!admission_precedes_setup(&socket_queries(&source(
        &setup.join(" ")
    ))));
    for (correct, wrong) in [
        ("Accepted::sample", "WrongAccepted::sample"),
        ("Accepted::sample", "Accepted::wrong_sample"),
        ("Accepted::sample", "<T as Accepted>::sample"),
        ("Connection::new", "WrongConnection::new"),
        ("Connection::new", "Connection::wrong_new"),
        ("Connection::new", "<T as Connection>::new"),
        ("builder.spawn", "builder.wrong_spawn"),
    ] {
        let body = format!("{admission} {}", setup.join(" ")).replace(correct, wrong);
        assert!(
            !admission_precedes_setup(&socket_queries(&source(&body))),
            "accepted {body}"
        );
    }
    let nested = format!("{admission} fn nested() {{ {} }}", setup.join(" "));
    assert!(!admission_precedes_setup(&socket_queries(&source(&nested))));
}

#[test]
fn invalid_socket_query_diagnostics_name_the_file_and_line() {
    let path = Path::new("nested/peer.rs");
    let source = "fn query() {\n    libc::getsockopt(fd, level, 17, value, len);\n}";
    let report = invalid_query_report(path, source, &socket_queries(source)).unwrap();
    assert_eq!(report, "nested/peer.rs: 1 unapproved socket queries\nnested/peer.rs:2:     libc::getsockopt(fd, level, 17, value, len);");
    let allowed = "fn query() { libc::getsockopt(fd, level, libc::LOCAL_PEERPID, value, len); }";
    assert!(invalid_query_report(path, allowed, &socket_queries(allowed)).is_none());
}
