//! The architecture guard of `TASK-213` (`.agents/rules/architecture-and-layout.md`): what the
//! folder layout promises is checked on every build, so the layout cannot drift back.
//!
//! The test reads the production modules of every crate target of the workspace (`src/lib.rs`,
//! `src/main.rs` and `src/bin/*.rs` of each `crates/*` and `apps/*` package). `syn` parses each
//! file and gives its module declarations (`mod x;`, `#[path]`, inline `mod x { .. }`) and the
//! extent of every test-only item (`#[cfg(test)]`, `#![cfg(test)]`); the file's tokens (with
//! their lines) give every crate-internal path it names: `crate::..`, `super::..`, `self::..`
//! and `$crate::..`, in `use` trees, signatures, bodies and macro definitions alike. Test code is
//! left out by path, exactly as `file_size_ratchet` decides it (`tests`, `*_tests`, `*-tests`,
//! `testing`, `testkit`, `test_support`), and by `cfg(test)`. A path that reaches an item of the
//! crate root is followed through the root's own `use`/`pub use` to the module it names.
//!
//! It enforces four invariants:
//!
//! 1. **`crates/org-agent-cli` is a two-tier VSA:** no horizontal layer file (`*_table.rs`,
//!    `*_dispatch.rs`, `help_guidance.rs`, `aliases.rs`); `runtime` names nothing outside
//!    `runtime`; each `features::<domain>` names only itself and `runtime` (no other domain, no
//!    `registry`, no crate-root item); `lib.rs` declares no `pub mod features|runtime|registry`.
//! 2. **`apps/agentic-org` is a 1:1 VSA:** no `features::<a>` names `features::<b>`, and the root
//!    shell modules (`args`, `completers`, `error`, `host`, `selection`, `view`) never name
//!    `features`.
//! 3. **Intra-crate module cycles only shrink:** the strongly connected components (two or more
//!    top-level modules) of every crate target are exactly [`KNOWN_CYCLES`]. A new cycle, a listed
//!    cycle that grows or shrinks, and a listed cycle that is gone all fail: the entry is changed
//!    or removed in the same change, so the list always says what is left.
//! 4. **`org-office` domains do not reach into each other's `features`:** a file of a journaled
//!    family (a top-level module with a `features` module), or a store operation file
//!    (`store/open/<name>_ops.rs`, owned by the top-level module `<name>` or `<name>s`), naming
//!    another family's `features::*` is exactly [`OFFICE_REACH_INS`], which only shrinks.
//!
//! To print what the scan finds: `VSA_STRUCTURE_PRINT=1 cargo test -p org-platform --test
//! vsa_structure -- --nocapture`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

const AGENT_CLI: &str = "crates/org-agent-cli/src/lib.rs";
const OPERATOR_CLI: &str = "apps/agentic-org/src/lib.rs";
const OFFICE: &str = "crates/org-office/src/lib.rs";

/// File names that are horizontal layers in the agent CLI: a command lives in its domain slice.
const AGENT_CLI_LAYER_SUFFIXES: [&str; 2] = ["_table.rs", "_dispatch.rs"];
const AGENT_CLI_LAYER_FILES: [&str; 2] = ["help_guidance.rs", "aliases.rs"];
/// The agent CLI modules that are never public API.
const AGENT_CLI_PRIVATE: [&str; 3] = ["features", "registry", "runtime"];
/// The operator CLI's root shell and helper modules, which features use and never the reverse.
const OPERATOR_SHELL: [&str; 6] = ["args", "completers", "error", "host", "selection", "view"];

/// Every strongly connected component of top-level production modules that exists today:
/// `(crate root, members in name order)`. Shrink only.
const KNOWN_CYCLES: &[(&str, &[&str])] = &[
    (
        "crates/org-agent-runtime/src/lib.rs",
        &["caps", "event", "spec", "usage"],
    ),
    ("crates/org-agent-runtime/src/lib.rs", &["path", "token"]),
    (
        "crates/org-host/src/lib.rs",
        &["admin", "recovery", "service"],
    ),
    (
        "crates/org-office-sessions/src/lib.rs",
        &["external", "lifecycle", "manager", "pump", "recovery"],
    ),
    ("crates/org-store/src/lib.rs", &["change", "error"]),
    ("crates/org-supervise/src/lib.rs", &["child", "stop"]),
];

/// Every `org-office` file that names another domain's `features` today: `(file, domain)`.
/// Shrink only.
const OFFICE_REACH_INS: &[(&str, &str)] = &[
    (
        "crates/org-office/src/messaging/features/mod.rs",
        "organization",
    ),
    (
        "crates/org-office/src/projects/features/contributing_teams.rs",
        "organization",
    ),
    (
        "crates/org-office/src/projects/features/create_project.rs",
        "organization",
    ),
    (
        "crates/org-office/src/projects/features/mod.rs",
        "organization",
    ),
    (
        "crates/org-office/src/projects/features/repositories.rs",
        "organization",
    ),
    ("crates/org-office/src/store/open/git_ops.rs", "projects"),
    ("crates/org-office/src/store/open/git_ops.rs", "tasks"),
    // TASK-218 moves the shared helpers out of `tasks::features`.
    ("crates/org-office/src/store/open/process_ops.rs", "tasks"),
];

// ---------------------------------------------------------------------------------------------
// Reading a crate target

/// One production module of a crate target.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Module {
    /// From the crate root; empty for the root itself.
    path: Vec<String>,
    /// The file it is written in, relative to the repository.
    file: PathBuf,
}

/// A crate-internal path a module names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reference {
    /// The naming module's path.
    from: Vec<String>,
    file: PathBuf,
    line: usize,
    /// The path named, from the crate root, as far as it is written (`[]` is the root).
    to: Vec<String>,
}

/// What the checks read of one crate target.
#[derive(Debug, Default)]
struct Target {
    root: PathBuf,
    modules: Vec<Module>,
    references: Vec<Reference>,
    /// Names the root binds with `use`/`pub use`, and the path each stands for.
    root_uses: BTreeMap<String, Vec<String>>,
    /// `pub mod` declarations of the root file: `(name, line)`.
    root_pub_mods: Vec<(String, usize)>,
}

/// The source of a file, by path relative to the repository.
type Read<'a> = &'a dyn Fn(&Path) -> Option<String>;

/// Whether `path` is dedicated test code (the rule of `file_size_ratchet`).
fn is_test_path(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        let stem = name.strip_suffix(".rs").unwrap_or(&name);
        matches!(stem, "tests" | "testing" | "testkit" | "test_support")
            || stem.ends_with("_tests")
            || stem.ends_with("-tests")
    })
}

/// Whether a `cfg` predicate holds only in a test build.
fn requires_test(predicate: TokenStream) -> bool {
    let tokens: Vec<TokenTree> = predicate.into_iter().collect();
    match tokens.as_slice() {
        [TokenTree::Ident(word)] => word == "test",
        [TokenTree::Ident(word), TokenTree::Group(args)] => {
            let parts = split_commas(args.stream());
            if word == "all" {
                parts.into_iter().any(requires_test)
            } else if word == "any" {
                !parts.is_empty() && parts.into_iter().all(requires_test)
            } else {
                false
            }
        }
        _ => false,
    }
}

fn split_commas(stream: TokenStream) -> Vec<TokenStream> {
    let mut parts = vec![Vec::new()];
    for token in stream {
        if matches!(&token, TokenTree::Punct(p) if p.as_char() == ',') {
            parts.push(Vec::new());
        } else if let Some(last) = parts.last_mut() {
            last.push(token);
        }
    }
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| part.into_iter().collect())
        .collect()
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        syn::Meta::List(list) if list.path.is_ident("cfg") => requires_test(list.tokens.clone()),
        _ => false,
    })
}

fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        syn::Meta::NameValue(pair) if pair.path.is_ident("path") => match &pair.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(text),
                ..
            }) => Some(text.value()),
            _ => None,
        },
        _ => None,
    })
}

fn line(span: proc_macro2::Span) -> usize {
    span.start().line
}

/// The last line of an item, where its closing brace or semicolon is.
fn item_end(item: &syn::Item) -> Option<usize> {
    use syn::Item;
    let fields_end = |fields: &syn::Fields, semi: &Option<syn::token::Semi>| match (fields, semi) {
        (_, Some(semi)) => line(semi.span),
        (syn::Fields::Named(named), None) => line(named.brace_token.span.close()),
        (syn::Fields::Unnamed(unnamed), None) => line(unnamed.paren_token.span.close()),
        (syn::Fields::Unit, None) => 0,
    };
    let macro_end = |mac: &syn::Macro, semi: &Option<syn::token::Semi>| match semi {
        Some(semi) => line(semi.span),
        None => line(match &mac.delimiter {
            syn::MacroDelimiter::Paren(d) => d.span.close(),
            syn::MacroDelimiter::Brace(d) => d.span.close(),
            syn::MacroDelimiter::Bracket(d) => d.span.close(),
        }),
    };
    Some(match item {
        Item::Const(i) => line(i.semi_token.span),
        Item::Enum(i) => line(i.brace_token.span.close()),
        Item::ExternCrate(i) => line(i.semi_token.span),
        Item::Fn(i) => line(i.block.brace_token.span.close()),
        Item::ForeignMod(i) => line(i.brace_token.span.close()),
        Item::Impl(i) => line(i.brace_token.span.close()),
        Item::Macro(i) => macro_end(&i.mac, &i.semi_token),
        Item::Mod(i) => match (&i.content, &i.semi) {
            (Some((brace, _)), _) => line(brace.span.close()),
            (None, Some(semi)) => line(semi.span),
            (None, None) => return None,
        },
        Item::Static(i) => line(i.semi_token.span),
        Item::Struct(i) => fields_end(&i.fields, &i.semi_token),
        Item::Trait(i) => line(i.brace_token.span.close()),
        Item::TraitAlias(i) => line(i.semi_token.span),
        Item::Type(i) => line(i.semi_token.span),
        Item::Union(i) => line(i.fields.brace_token.span.close()),
        Item::Use(i) => line(i.semi_token.span),
        _ => return None,
    })
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    use syn::Item;
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

/// A `mod name;` to load from another file.
#[derive(Debug)]
struct ChildDecl {
    /// The inline modules it is declared in, within the file.
    inline: Vec<String>,
    name: String,
    path_attr: Option<String>,
    public: bool,
    line: usize,
}

/// What `syn` gives of one file: its module declarations and test-only line ranges.
#[derive(Debug, Default)]
struct FileItems {
    children: Vec<ChildDecl>,
    /// Inline modules: path within the file and line range.
    inline: Vec<(Vec<String>, usize, usize)>,
    /// Line ranges of test-only items.
    skipped: Vec<(usize, usize)>,
}

fn collect_items(items: &[syn::Item], inline: &[String], out: &mut FileItems) {
    for item in items {
        let attrs = item_attrs(item);
        if test_only(attrs) {
            let start = attrs.first().map_or(0, |attr| line(attr.pound_token.span));
            out.skipped
                .push((start, item_end(item).unwrap_or(usize::MAX)));
            continue;
        }
        match item {
            syn::Item::Mod(module) => {
                let name = module.ident.to_string();
                match &module.content {
                    None => out.children.push(ChildDecl {
                        inline: inline.to_vec(),
                        name,
                        path_attr: path_attr(&module.attrs),
                        public: matches!(module.vis, syn::Visibility::Public(_)),
                        line: line(module.mod_token.span),
                    }),
                    Some((brace, content)) => {
                        let mut path = inline.to_vec();
                        path.push(name);
                        out.inline.push((
                            path.clone(),
                            line(module.mod_token.span),
                            line(brace.span.close()),
                        ));
                        collect_items(content, &path, out);
                    }
                }
            }
            syn::Item::Impl(block) => {
                for member in &block.items {
                    if let syn::ImplItem::Fn(function) = member {
                        if test_only(&function.attrs) {
                            let start = line(function.attrs[0].pound_token.span);
                            out.skipped
                                .push((start, line(function.block.brace_token.span.close())));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// `::` at `index`.
fn colons(tokens: &[TokenTree], index: usize) -> bool {
    matches!(
        (tokens.get(index), tokens.get(index + 1)),
        (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
            if a.as_char() == ':' && a.spacing() == Spacing::Joint && b.as_char() == ':'
    )
}

/// Every path a `use` tree or path written from `at` names, each as its segments (`self` in a
/// group names the prefix itself; `*` and `as` add nothing).
fn path_tails(tokens: &[TokenTree], mut at: usize, prefix: Vec<String>) -> Vec<Vec<String>> {
    let mut path = prefix;
    loop {
        match tokens.get(at) {
            Some(TokenTree::Ident(word)) => {
                let word = word.to_string();
                if word != "self" {
                    path.push(word.trim_start_matches("r#").to_owned());
                }
                if !colons(tokens, at + 1) {
                    return vec![path];
                }
                at += 3;
            }
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                let mut out = Vec::new();
                for part in split_commas(group.stream()) {
                    let part: Vec<TokenTree> = part.into_iter().collect();
                    out.extend(path_tails(&part, 0, path.clone()));
                }
                return out;
            }
            _ => return vec![path],
        }
    }
}

/// The crate-internal paths `tokens` name, each with its line and the path segments from where
/// it is written: `(line, supers, rooted, tail)` where `rooted` is `crate::`/`$crate::` and
/// `supers` counts `super::` (`self::` is zero supers, not rooted).
fn scan_paths(tokens: &[TokenTree], out: &mut Vec<(usize, usize, bool, Vec<String>)>) {
    let mut at = 0;
    while at < tokens.len() {
        match &tokens[at] {
            TokenTree::Group(group) => {
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                scan_paths(&inner, out);
            }
            TokenTree::Ident(word) if word == "pub" => {
                // `pub(in crate::x)` restricts visibility; it names no dependency.
                if matches!(tokens.get(at + 1), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
                {
                    at += 1;
                }
            }
            TokenTree::Ident(word)
                if (word == "crate" || word == "super" || word == "self")
                    && colons(tokens, at + 1)
                    && !(at >= 2 && colons(tokens, at - 2)) =>
            {
                let start = line(word.span());
                let rooted = word == "crate";
                let mut supers = usize::from(word == "super");
                let mut next = at + 3;
                while matches!(tokens.get(next), Some(TokenTree::Ident(w)) if w == "super")
                    && colons(tokens, next + 1)
                {
                    supers += 1;
                    next += 3;
                }
                for tail in path_tails(tokens, next, Vec::new()) {
                    out.push((start, supers, rooted, tail));
                }
                at = next;
                continue;
            }
            _ => {}
        }
        at += 1;
    }
}

/// The directory a file's `mod x;` children are in.
fn child_dir(file: &Path, root_like: bool) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("")).to_path_buf();
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned());
    if root_like || name.as_deref() == Some("mod.rs") {
        dir
    } else {
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned());
        dir.join(stem.unwrap_or_default())
    }
}

/// Reads one crate target from its root file.
fn load_target(root: &Path, read: Read<'_>) -> Result<Target, String> {
    let mut target = Target {
        root: root.to_path_buf(),
        ..Target::default()
    };
    // (file, module path, whether it is the crate root)
    let mut pending: Vec<(PathBuf, Vec<String>, bool)> = vec![(root.to_path_buf(), vec![], true)];
    while let Some((file, path, is_root)) = pending.pop() {
        let source = read(&file).ok_or_else(|| format!("{}: cannot be read", file.display()))?;
        let source = source.strip_prefix('\u{feff}').unwrap_or(&source);
        let parsed =
            syn::parse_file(source).map_err(|error| format!("{}: {error}", file.display()))?;
        if test_only(&parsed.attrs) {
            continue;
        }
        let mut items = FileItems::default();
        collect_items(&parsed.items, &[], &mut items);
        let dir = child_dir(&file, is_root);
        for child in &items.children {
            let mut module = path.clone();
            module.extend(child.inline.iter().cloned());
            module.push(child.name.clone());
            if is_root && child.inline.is_empty() && child.public {
                target.root_pub_mods.push((child.name.clone(), child.line));
            }
            let mut base = dir.clone();
            base.extend(child.inline.iter());
            let candidates = match &child.path_attr {
                Some(p) if child.inline.is_empty() => {
                    vec![file.parent().unwrap_or(Path::new("")).join(p)]
                }
                Some(p) => vec![base.join(p)],
                None => vec![
                    base.join(format!("{}.rs", child.name)),
                    base.join(&child.name).join("mod.rs"),
                ],
            };
            let Some(found) = candidates.iter().find(|c| read(c).is_some()) else {
                return Err(format!(
                    "{}:{}: module `{}` has no file",
                    file.display(),
                    child.line,
                    child.name
                ));
            };
            if !is_test_path(found) {
                pending.push((found.clone(), module, false));
            }
        }
        target.modules.push(Module {
            path: path.clone(),
            file: file.clone(),
        });
        for (inline, _, _) in &items.inline {
            let mut module = path.clone();
            module.extend(inline.iter().cloned());
            target.modules.push(Module {
                path: module,
                file: file.clone(),
            });
        }
        let tokens: TokenStream = source
            .parse()
            .map_err(|error| format!("{}: {error}", file.display()))?;
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        let mut found = Vec::new();
        scan_paths(&tokens, &mut found);
        for (at, supers, rooted, tail) in found {
            if items.skipped.iter().any(|(s, e)| (*s..=*e).contains(&at)) {
                continue;
            }
            // The innermost inline module holding the line, else the file's module.
            let mut from = path.clone();
            if let Some((inline, _, _)) = items
                .inline
                .iter()
                .filter(|(_, s, e)| (*s..=*e).contains(&at))
                .max_by_key(|(inline, _, _)| inline.len())
            {
                from.extend(inline.iter().cloned());
            }
            let to = if rooted {
                tail
            } else if supers > from.len() {
                continue;
            } else {
                let mut to = from[..from.len() - supers].to_vec();
                to.extend(tail);
                to
            };
            target.references.push(Reference {
                from,
                file: file.clone(),
                line: at,
                to,
            });
        }
        if is_root {
            collect_root_uses(&parsed.items, &mut target.root_uses);
        }
    }
    target.modules.sort_by(|a, b| a.path.cmp(&b.path));
    target
        .references
        .sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    let tops = target.top_level();
    // A root `use a::b` names `a` from the root as well as `crate::a`.
    for path in target.root_uses.values_mut() {
        if path.first().is_some_and(|first| first == "crate") {
            path.remove(0);
        }
    }
    target
        .root_uses
        .retain(|_, path| path.first().is_some_and(|first| tops.contains(first)));
    Ok(target)
}

fn collect_root_uses(items: &[syn::Item], out: &mut BTreeMap<String, Vec<String>>) {
    fn walk(tree: &syn::UseTree, prefix: &[String], out: &mut BTreeMap<String, Vec<String>>) {
        match tree {
            syn::UseTree::Path(path) => {
                let mut next = prefix.to_vec();
                next.push(path.ident.to_string());
                walk(&path.tree, &next, out);
            }
            syn::UseTree::Name(name) => {
                let mut full = prefix.to_vec();
                if name.ident != "self" {
                    full.push(name.ident.to_string());
                }
                if let Some(last) = full.last().cloned() {
                    out.insert(last, full);
                }
            }
            syn::UseTree::Rename(rename) => {
                let mut full = prefix.to_vec();
                full.push(rename.ident.to_string());
                out.insert(rename.rename.to_string(), full);
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    walk(tree, prefix, out);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }
    for item in items {
        if let syn::Item::Use(item) = item {
            if !test_only(&item.attrs) {
                walk(&item.tree, &[], out);
            }
        }
    }
}

impl Target {
    /// The top-level modules (children of the root).
    fn top_level(&self) -> BTreeSet<String> {
        self.modules
            .iter()
            .filter(|m| m.path.len() == 1)
            .map(|m| m.path[0].clone())
            .collect()
    }

    /// The module path a reference reaches, through the root's `use` when it names a root item;
    /// `None` for an item defined in the root itself.
    fn resolve(&self, to: &[String]) -> Option<Vec<String>> {
        let first = to.first()?;
        if self.top_level().contains(first) {
            return Some(to.to_vec());
        }
        let mut via = self.root_uses.get(first)?.clone();
        via.extend(to[1..].iter().cloned());
        Some(via)
    }

    /// Top-level module dependencies, `from -> {to}`, of every module below the root.
    fn graph(&self) -> BTreeMap<String, BTreeSet<String>> {
        let tops = self.top_level();
        let mut graph: BTreeMap<String, BTreeSet<String>> =
            tops.iter().map(|t| (t.clone(), BTreeSet::new())).collect();
        for reference in &self.references {
            let Some(from) = reference.from.first() else {
                continue;
            };
            let Some(to) = self.resolve(&reference.to) else {
                continue;
            };
            if let Some(to) = to.first().filter(|to| *to != from && tops.contains(*to)) {
                graph.entry(from.clone()).or_default().insert(to.clone());
            }
        }
        graph
    }
}

/// Tarjan's strongly connected components with two or more members, each sorted.
fn cycles(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    struct State<'a> {
        graph: &'a BTreeMap<String, BTreeSet<String>>,
        index: BTreeMap<&'a str, usize>,
        low: BTreeMap<&'a str, usize>,
        stack: Vec<&'a str>,
        on_stack: BTreeSet<&'a str>,
        next: usize,
        found: Vec<Vec<String>>,
    }
    fn connect<'a>(state: &mut State<'a>, node: &'a str) {
        state.index.insert(node, state.next);
        state.low.insert(node, state.next);
        state.next += 1;
        state.stack.push(node);
        state.on_stack.insert(node);
        let graph = state.graph;
        for next in graph.get(node).into_iter().flatten() {
            let next = next.as_str();
            if !state.index.contains_key(next) {
                connect(state, next);
                let low = state.low[node].min(state.low[next]);
                state.low.insert(node, low);
            } else if state.on_stack.contains(next) {
                let low = state.low[node].min(state.index[next]);
                state.low.insert(node, low);
            }
        }
        if state.low[node] == state.index[node] {
            let mut component = Vec::new();
            while let Some(member) = state.stack.pop() {
                state.on_stack.remove(member);
                component.push(member.to_owned());
                if member == node {
                    break;
                }
            }
            if component.len() > 1 {
                component.sort();
                state.found.push(component);
            }
        }
    }
    let mut state = State {
        graph,
        index: BTreeMap::new(),
        low: BTreeMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        next: 0,
        found: Vec::new(),
    };
    for node in graph.keys() {
        if !state.index.contains_key(node.as_str()) {
            connect(&mut state, node);
        }
    }
    state.found.sort();
    state.found
}

// ---------------------------------------------------------------------------------------------
// The checks

fn at(reference: &Reference) -> String {
    format!(
        "{}:{} (`{}` names `{}`)",
        reference.file.display(),
        reference.line,
        display_path(&reference.from),
        display_path(&reference.to)
    )
}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "crate".to_owned()
    } else {
        format!("crate::{}", path.join("::"))
    }
}

/// Invariant 1: `crates/org-agent-cli` (`files` are its source files under `src/`).
fn agent_cli_violations(target: &Target, files: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if AGENT_CLI_LAYER_SUFFIXES.iter().any(|s| name.ends_with(s))
            || AGENT_CLI_LAYER_FILES.contains(&name.as_str())
        {
            out.push(format!(
                "{}: a horizontal layer file; put each command's row, guidance and handler in its `features/<domain>/` slice",
                file.display()
            ));
        }
    }
    for (name, line) in &target.root_pub_mods {
        if AGENT_CLI_PRIVATE.contains(&name.as_str()) {
            out.push(format!(
                "{}:{line}: `pub mod {name}` makes an internal tier public API; re-export the narrow items instead",
                target.root.display()
            ));
        }
    }
    for reference in &target.references {
        let Some(tier) = reference.from.first() else {
            continue;
        };
        let to = target.resolve(&reference.to).unwrap_or_default();
        let to_tier = to.first().map(String::as_str);
        match tier.as_str() {
            "runtime" if to_tier != Some("runtime") => out.push(format!(
                "{}: `runtime` is the leaf kernel and names nothing outside `runtime`",
                at(reference)
            )),
            "features" => {
                let own = reference.from.get(1);
                let allowed = to_tier == Some("runtime")
                    || (to_tier == Some("features")
                        && (own.is_none() || to.get(1).is_none() || to.get(1) == own));
                if !allowed {
                    out.push(format!(
                        "{}: a feature slice names only itself and `runtime` (no other slice, `registry` or crate-root item)",
                        at(reference)
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// Invariant 2: `apps/agentic-org`.
fn operator_cli_violations(target: &Target) -> Vec<String> {
    let mut out = Vec::new();
    for reference in &target.references {
        let Some(to) = target.resolve(&reference.to) else {
            continue;
        };
        let from = &reference.from;
        let from_top = from.first().map(String::as_str);
        if to.first().map(String::as_str) != Some("features") {
            continue;
        }
        if from_top.is_some_and(|top| OPERATOR_SHELL.contains(&top)) {
            out.push(format!(
                "{}: a root shell module never names `features`",
                at(reference)
            ));
        } else if from_top == Some("features")
            && from.len() >= 2
            && to.len() >= 2
            && from[1] != to[1]
        {
            out.push(format!(
                "{}: a feature never names another feature",
                at(reference)
            ));
        }
    }
    out
}

/// The top-level module that owns an `org-office` file for invariant 4, if any.
fn office_owner(target: &Target, file: &Path) -> Option<String> {
    let src = target.root.parent()?;
    let relative = file.strip_prefix(src).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let tops = target.top_level();
    let families = office_families(target);
    if parts.len() >= 2 && families.contains(&parts[0]) {
        return Some(parts[0].clone());
    }
    if parts.len() >= 3 && parts[0] == "store" && parts[1] == "open" {
        let stem = parts[2].strip_suffix(".rs").unwrap_or(&parts[2]);
        let name = stem.strip_suffix("_ops")?;
        return [name.to_owned(), format!("{name}s"), format!("{name}es")]
            .into_iter()
            .find(|candidate| tops.contains(candidate));
    }
    None
}

/// The journaled families: top-level modules with a `features` module.
fn office_families(target: &Target) -> BTreeSet<String> {
    target
        .modules
        .iter()
        .filter(|m| m.path.len() == 2 && m.path[1] == "features")
        .map(|m| m.path[0].clone())
        .collect()
}

/// Invariant 4: every `(file, domain)` reach into another family's `features`.
fn office_reach_ins(target: &Target) -> BTreeSet<(String, String)> {
    let families = office_families(target);
    let mut out = BTreeSet::new();
    for reference in &target.references {
        let Some(owner) = office_owner(target, &reference.file) else {
            continue;
        };
        let Some(to) = target.resolve(&reference.to) else {
            continue;
        };
        if to.len() >= 2 && to[1] == "features" && families.contains(&to[0]) && to[0] != owner {
            out.insert((reference.file.display().to_string(), to[0].clone()));
        }
    }
    out
}

/// Invariant 3 for one target: what differs from [`KNOWN_CYCLES`].
fn cycle_violations(target: &Target, known: &[(&str, &[&str])]) -> Vec<String> {
    let root = target.root.display().to_string();
    let found = cycles(&target.graph());
    let listed: Vec<Vec<String>> = known
        .iter()
        .filter(|(r, _)| *r == root)
        .map(|(_, members)| members.iter().map(|m| (*m).to_owned()).collect())
        .collect();
    let mut out = Vec::new();
    for component in &found {
        if !listed.contains(component) {
            out.push(format!(
                "{root}: the top-level modules {{{}}} depend on each other in a cycle; break it (a listed cycle never grows, and none is added)",
                component.join(", ")
            ));
        }
    }
    for component in &listed {
        if !found.contains(component) {
            out.push(format!(
                "{root}: the listed cycle {{{}}} is gone or changed; update or remove its KNOWN_CYCLES entry",
                component.join(", ")
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The workspace

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/org-platform is two levels below the repository root")
        .to_path_buf()
}

/// Every crate target root of the workspace's packages, relative to the repository.
fn target_roots(repo: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for group in ["apps", "crates"] {
        let Ok(entries) = fs::read_dir(repo.join(group)) else {
            continue;
        };
        let mut packages: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.join("Cargo.toml").is_file())
            .collect();
        packages.sort();
        for package in packages {
            let relative = package.strip_prefix(repo).expect("under the repository");
            for root in ["src/lib.rs", "src/main.rs"] {
                if package.join(root).is_file() {
                    roots.push(relative.join(root));
                }
            }
            if let Ok(bins) = fs::read_dir(package.join("src/bin")) {
                let mut bins: Vec<PathBuf> = bins
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "rs"))
                    .collect();
                bins.sort();
                roots.extend(
                    bins.iter()
                        .map(|b| b.strip_prefix(repo).unwrap().to_path_buf()),
                );
            }
        }
    }
    roots
}

fn files_under(repo: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![repo.join(dir)];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("a readable source directory") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                found.push(path.strip_prefix(repo).unwrap().to_path_buf());
            }
        }
    }
    found.sort();
    found
}

fn print_requested() -> bool {
    std::env::var_os("VSA_STRUCTURE_PRINT").is_some()
}

#[test]
fn the_workspace_keeps_its_vsa_boundaries_and_only_shrinks_its_module_cycles() {
    let repo = repository();
    let read = |path: &Path| fs::read_to_string(repo.join(path)).ok();
    let mut failures = Vec::new();
    let mut targets = BTreeMap::new();
    for root in target_roots(&repo) {
        match load_target(&root, &read) {
            Ok(target) => {
                if print_requested() {
                    for cycle in cycles(&target.graph()) {
                        println!("cycle {} {:?}", root.display(), cycle);
                    }
                }
                failures.extend(cycle_violations(&target, KNOWN_CYCLES));
                targets.insert(root.display().to_string(), target);
            }
            Err(error) => failures.push(error),
        }
    }
    for (root, _) in KNOWN_CYCLES {
        if !targets.contains_key(*root) {
            failures.push(format!(
                "{root}: listed in KNOWN_CYCLES but not a crate target"
            ));
        }
    }

    let agent_cli = targets
        .get(AGENT_CLI)
        .expect("the agent CLI is a crate target");
    let agent_files = files_under(&repo, Path::new("crates/org-agent-cli/src"));
    failures.extend(agent_cli_violations(agent_cli, &agent_files));

    let operator = targets
        .get(OPERATOR_CLI)
        .expect("the operator CLI is a crate target");
    failures.extend(operator_cli_violations(operator));

    let office = targets.get(OFFICE).expect("org-office is a crate target");
    let found = office_reach_ins(office);
    if print_requested() {
        for (file, domain) in &found {
            println!("reach-in {file} -> {domain}::features");
        }
    }
    let listed: BTreeSet<(String, String)> = OFFICE_REACH_INS
        .iter()
        .map(|(f, d)| ((*f).to_owned(), (*d).to_owned()))
        .collect();
    for (file, domain) in found.difference(&listed) {
        failures.push(format!(
            "{file}: names `{domain}::features` from another domain; share a narrow capability or a domain-neutral helper instead"
        ));
    }
    for (file, domain) in listed.difference(&found) {
        failures.push(format!(
            "{file}: no longer names `{domain}::features`; remove its OFFICE_REACH_INS entry"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[cfg(test)]
mod synthetic {
    //! Each check catches the violation it exists for, on sources held in memory.

    use super::*;

    fn target(root: &str, files: &[(&str, &str)]) -> Target {
        let files: BTreeMap<PathBuf, String> = files
            .iter()
            .map(|(p, s)| (PathBuf::from(p), (*s).to_owned()))
            .collect();
        let read = |path: &Path| files.get(path).cloned();
        load_target(Path::new(root), &read).expect("the synthetic crate loads")
    }

    fn agent(files: &[(&str, &str)]) -> Vec<String> {
        let t = target("cli/src/lib.rs", files);
        let paths: Vec<PathBuf> = files.iter().map(|(p, _)| PathBuf::from(p)).collect();
        agent_cli_violations(&t, &paths)
    }

    const CLEAN_LIB: &str =
        "mod features; mod registry; mod runtime; pub use runtime::input::Thing;";
    const FEATURES: &str = "pub(crate) mod auth; pub(crate) mod work;";

    #[test]
    fn a_clean_two_tier_layout_passes() {
        let found = agent(&[
            ("cli/src/lib.rs", CLEAN_LIB),
            (
                "cli/src/registry.rs",
                "use crate::features::{auth, work}; fn f() { crate::runtime::input::go(); }",
            ),
            ("cli/src/runtime/mod.rs", "pub(crate) mod input;"),
            (
                "cli/src/runtime/input.rs",
                "pub struct Thing; pub fn go() { self::helper(); } fn helper() {}",
            ),
            ("cli/src/features/mod.rs", FEATURES),
            (
                "cli/src/features/auth/mod.rs",
                "use crate::runtime::input::Thing; use super::auth::x; mod x;",
            ),
            (
                "cli/src/features/auth/x.rs",
                "use super::super::auth::y; fn f() { super::z(); }",
            ),
            (
                "cli/src/features/work/mod.rs",
                "use crate::Thing; #[cfg(test)] mod tests;",
            ),
            (
                "cli/src/features/work/tests.rs",
                "use crate::features::auth::Anything;",
            ),
        ]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_cross_feature_import_is_caught_in_use_trees_and_inline_paths() {
        let found = agent(&[
            ("cli/src/lib.rs", CLEAN_LIB),
            ("cli/src/registry.rs", ""),
            ("cli/src/runtime/mod.rs", "pub(crate) mod input;"),
            ("cli/src/runtime/input.rs", "pub struct Thing;"),
            ("cli/src/features/mod.rs", FEATURES),
            (
                "cli/src/features/auth/mod.rs",
                "use crate::{runtime::input, features::{work::Row}};",
            ),
            (
                "cli/src/features/work/mod.rs",
                "fn f() { super::auth::go(); crate::registry::table(); }",
            ),
        ]);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].contains("features::work::Row"), "{found:?}");
        assert!(found[1].contains("features::auth::go"), "{found:?}");
        assert!(found[2].contains("registry::table"), "{found:?}");
    }

    #[test]
    fn runtime_naming_a_feature_the_registry_or_the_root_is_caught() {
        let found = agent(&[
            (
                "cli/src/lib.rs",
                "mod features; mod registry; mod runtime; pub fn run() {}",
            ),
            ("cli/src/registry.rs", ""),
            (
                "cli/src/runtime/mod.rs",
                "use crate::features::auth; fn f() { crate::registry::x(); crate::run(); }",
            ),
            ("cli/src/features/mod.rs", "pub(crate) mod auth;"),
            (
                "cli/src/features/auth.rs",
                "#[cfg(test)] fn t() { crate::registry::x(); }",
            ),
        ]);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found.iter().all(|f| f.contains("leaf kernel")), "{found:?}");
    }

    #[test]
    fn a_public_internal_tier_and_a_layer_file_are_caught() {
        let found = agent(&[
            (
                "cli/src/lib.rs",
                "pub mod features; mod registry; pub(crate) mod runtime; pub mod msg_table;",
            ),
            ("cli/src/registry.rs", ""),
            ("cli/src/runtime.rs", ""),
            ("cli/src/features.rs", ""),
            ("cli/src/msg_table.rs", ""),
        ]);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("msg_table.rs"), "{found:?}");
        assert!(found[1].contains("pub mod features"), "{found:?}");
    }

    #[test]
    fn a_module_cycle_is_found_and_the_ratchet_holds_it_exactly() {
        let t = target(
            "x/src/lib.rs",
            &[
                (
                    "x/src/lib.rs",
                    "mod a; mod b; mod c; mod d; pub use c::Root;",
                ),
                ("x/src/a.rs", "use crate::b::B; pub struct A;"),
                ("x/src/b.rs", "pub struct B; fn f() { crate::Root::new(); }"),
                (
                    "x/src/c/mod.rs",
                    "pub struct Root; mod inner { fn g() { super::super::a::A; } }",
                ),
                (
                    "x/src/d.rs",
                    "#[cfg(test)] use crate::a::A; pub(in crate::a) fn f() {}",
                ),
            ],
        );
        assert_eq!(
            cycles(&t.graph()),
            vec![vec!["a".to_owned(), "b".into(), "c".into()]]
        );
        let new = cycle_violations(&t, &[]);
        assert_eq!(new.len(), 1, "{new:?}");
        assert!(new[0].contains("{a, b, c}"), "{new:?}");
        assert!(cycle_violations(&t, &[("x/src/lib.rs", &["a", "b", "c"])]).is_empty());
        let stale = cycle_violations(&t, &[("x/src/lib.rs", &["a", "b"])]);
        assert_eq!(
            stale.len(),
            2,
            "a shrunk or grown entry fails both ways: {stale:?}"
        );
    }

    #[test]
    fn the_operator_cli_rules_catch_cross_features_and_shell_reach_ins() {
        let t = target(
            "op/src/lib.rs",
            &[
                ("op/src/lib.rs", "mod features; mod view; mod cli;"),
                ("op/src/cli.rs", "use crate::features::org::run;"),
                ("op/src/view.rs", "use crate::features::org::Card;"),
                (
                    "op/src/features/mod.rs",
                    "pub(crate) mod org; pub(crate) mod project;",
                ),
                (
                    "op/src/features/org.rs",
                    "use crate::view::Table; use super::project::Row;",
                ),
                (
                    "op/src/features/project/mod.rs",
                    "use crate::features::project::Own;",
                ),
            ],
        );
        let found = operator_cli_violations(&t);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|f| f.contains("root shell")), "{found:?}");
        assert!(
            found.iter().any(|f| f.contains("another feature")),
            "{found:?}"
        );
    }

    #[test]
    fn an_office_domain_reaching_into_another_domains_features_is_caught() {
        let t = target(
            "o/src/lib.rs",
            &[
                (
                    "o/src/lib.rs",
                    "mod tasks; mod projects; mod processes; mod store; mod office;",
                ),
                ("o/src/tasks/mod.rs", "pub mod features;"),
                ("o/src/tasks/features.rs", "pub fn helper() {}"),
                (
                    "o/src/projects/mod.rs",
                    "pub mod features; use crate::tasks::features::helper;",
                ),
                (
                    "o/src/projects/features.rs",
                    "use crate::projects::features::x;",
                ),
                ("o/src/processes/mod.rs", ""),
                ("o/src/store/mod.rs", "mod open;"),
                (
                    "o/src/store/open/mod.rs",
                    "mod process_ops; mod project_ops; mod operations;",
                ),
                (
                    "o/src/store/open/process_ops.rs",
                    "use crate::{tasks::{features::{helper}}};",
                ),
                (
                    "o/src/store/open/project_ops.rs",
                    "use crate::projects::features::x;",
                ),
                (
                    "o/src/store/open/operations.rs",
                    "use crate::tasks::features::helper;",
                ),
                ("o/src/office.rs", "use crate::tasks::features::helper;"),
            ],
        );
        let found: Vec<(String, String)> = office_reach_ins(&t).into_iter().collect();
        assert_eq!(
            found,
            vec![
                ("o/src/projects/mod.rs".to_owned(), "tasks".to_owned()),
                (
                    "o/src/store/open/process_ops.rs".to_owned(),
                    "tasks".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn test_only_code_is_never_production() {
        assert!(requires_test("test".parse().unwrap()));
        assert!(requires_test("all(test, unix)".parse().unwrap()));
        assert!(requires_test("any(test, all(test, unix))".parse().unwrap()));
        assert!(!requires_test(
            "any(test, feature = \"testkit\")".parse().unwrap()
        ));
        assert!(!requires_test("not(test)".parse().unwrap()));
        assert!(is_test_path(Path::new("crates/x/src/a/tests.rs")));
        assert!(is_test_path(Path::new("crates/x/src/wake_tests.rs")));
        assert!(is_test_path(Path::new("crates/x/src/testkit/mod.rs")));
        assert!(!is_test_path(Path::new("crates/x/src/testing_ground.rs")));
        let t = target(
            "t/src/lib.rs",
            &[
                ("t/src/lib.rs", "mod a; mod b; #[cfg(test)] mod c;"),
                (
                    "t/src/a.rs",
                    "use crate::b::B;\nimpl X {\n    #[cfg(test)]\n    fn t() { crate::b::y(); }\n}\n",
                ),
                ("t/src/b.rs", "#![cfg(test)]\nuse crate::a::A;"),
            ],
        );
        assert!(cycles(&t.graph()).is_empty());
        assert_eq!(t.references.len(), 1, "{:?}", t.references);
    }
}
