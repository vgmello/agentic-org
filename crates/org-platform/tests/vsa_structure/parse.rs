//! Reading one source file: what `syn` says of its structure, and the paths its tokens name.
//!
//! Every extent is a pair of `proc_macro2` line/column positions, so two items on one line are
//! told apart. An extent the reader cannot work out is an error, never "to the end of the file".

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, LineColumn, Spacing, TokenStream, TokenTree};

/// Cargo features that exist only for tests: a module gated on one may use a test file name.
const TEST_FEATURES: [&str; 2] = ["testkit", "test-support"];

/// How a written path starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    /// `crate::` or `$crate::`.
    Root,
    /// `self::` (zero) or as many `super::` (after an optional `self::`).
    Up(usize),
    /// A plain name (the first segment), which the module's own names resolve.
    Local,
}

/// A path as written in a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub head: Head,
    pub segments: Vec<String>,
}

/// A path being read segment by segment.
#[derive(Debug, Clone, Default)]
struct Building {
    head: Option<Head>,
    segments: Vec<String>,
}

impl Building {
    fn push(&mut self, word: &str) {
        match (self.head, word) {
            (None, "crate") => self.head = Some(Head::Root),
            (None, "self") => self.head = Some(Head::Up(0)),
            (None, "super") => self.head = Some(Head::Up(1)),
            // `super::{super::x}` and `self::super::x` climb once more.
            (Some(Head::Up(supers)), "super") if self.segments.is_empty() => {
                self.head = Some(Head::Up(supers + 1));
            }
            // `{self}` names the prefix; `super` after a name is not Rust.
            (Some(_), "self" | "super") => {}
            (None, _) => {
                self.head = Some(Head::Local);
                self.segments.push(word.to_owned());
            }
            (Some(_), _) => self.segments.push(word.to_owned()),
        }
    }

    fn done(self) -> Option<Written> {
        self.head.map(|head| Written {
            head,
            segments: self.segments,
        })
    }
}

/// A name without its `r#`.
pub fn unraw(word: &impl ToString) -> String {
    let word = word.to_string();
    match word.strip_prefix("r#") {
        Some(bare) => bare.to_owned(),
        None => word,
    }
}

// ---------------------------------------------------------------------------------------------
// `cfg` predicates and attributes

/// Whether a `cfg` predicate holds only in a test build, or with one of `features` on.
fn requires(predicate: TokenStream, features: &[&str]) -> bool {
    let tokens: Vec<TokenTree> = predicate.into_iter().collect();
    match tokens.as_slice() {
        [TokenTree::Ident(word)] => word == "test",
        [TokenTree::Ident(word), TokenTree::Punct(eq), TokenTree::Literal(value)]
            if word == "feature" && eq.as_char() == '=' =>
        {
            let value = value.to_string();
            features.iter().any(|f| value == format!("\"{f}\""))
        }
        [TokenTree::Ident(word), TokenTree::Group(args)] => {
            let parts = split_commas(args.stream());
            if word == "all" {
                parts.into_iter().any(|part| requires(part, features))
            } else if word == "any" {
                !parts.is_empty() && parts.into_iter().all(|part| requires(part, features))
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Whether a `cfg` predicate holds only in a test build.
pub fn requires_test(predicate: TokenStream) -> bool {
    requires(predicate, &[])
}

fn cfg_requires(attrs: &[syn::Attribute], features: &[&str]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        syn::Meta::List(list) if list.path.is_ident("cfg") => {
            requires(list.tokens.clone(), features)
        }
        _ => false,
    })
}

/// Whether the attributes make an item test-only (`#[cfg(test)]`, `#![cfg(test)]`).
pub fn test_only(attrs: &[syn::Attribute]) -> bool {
    cfg_requires(attrs, &[])
}

/// Whether a `mod` is compiled only for tests or a test-only feature.
fn gated_for_tests(attrs: &[syn::Attribute]) -> bool {
    cfg_requires(attrs, &TEST_FEATURES)
}

pub fn split_commas(stream: TokenStream) -> Vec<TokenStream> {
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

/// The name an item defines in its module's namespace (child modules included).
fn item_name(item: &syn::Item) -> Option<String> {
    use syn::Item;
    let ident = match item {
        Item::Const(i) => &i.ident,
        Item::Enum(i) => &i.ident,
        Item::Fn(i) => &i.sig.ident,
        Item::Macro(i) => i.ident.as_ref()?,
        Item::Mod(i) => &i.ident,
        Item::Static(i) => &i.ident,
        Item::Struct(i) => &i.ident,
        Item::Trait(i) => &i.ident,
        Item::TraitAlias(i) => &i.ident,
        Item::Type(i) => &i.ident,
        Item::Union(i) => &i.ident,
        _ => return None,
    };
    Some(unraw(ident))
}

/// Where an item ends: the end of its closing brace or semicolon.
fn item_end(item: &syn::Item) -> Option<LineColumn> {
    use syn::Item;
    let semi_end = |semi: &syn::token::Semi| semi.span.end();
    let fields_end = |fields: &syn::Fields, semi: &Option<syn::token::Semi>| match (fields, semi) {
        (_, Some(semi)) => Some(semi_end(semi)),
        (syn::Fields::Named(named), None) => Some(named.brace_token.span.close().end()),
        (syn::Fields::Unnamed(_) | syn::Fields::Unit, None) => None,
    };
    let macro_end = |mac: &syn::Macro, semi: &Option<syn::token::Semi>| match semi {
        Some(semi) => semi_end(semi),
        None => match &mac.delimiter {
            syn::MacroDelimiter::Paren(d) => d.span.close().end(),
            syn::MacroDelimiter::Brace(d) => d.span.close().end(),
            syn::MacroDelimiter::Bracket(d) => d.span.close().end(),
        },
    };
    match item {
        Item::Const(i) => Some(semi_end(&i.semi_token)),
        Item::Enum(i) => Some(i.brace_token.span.close().end()),
        Item::ExternCrate(i) => Some(semi_end(&i.semi_token)),
        Item::Fn(i) => Some(i.block.brace_token.span.close().end()),
        Item::ForeignMod(i) => Some(i.brace_token.span.close().end()),
        Item::Impl(i) => Some(i.brace_token.span.close().end()),
        Item::Macro(i) => Some(macro_end(&i.mac, &i.semi_token)),
        Item::Mod(i) => match (&i.content, &i.semi) {
            (Some((brace, _)), _) => Some(brace.span.close().end()),
            (None, Some(semi)) => Some(semi_end(semi)),
            (None, None) => None,
        },
        Item::Static(i) => Some(semi_end(&i.semi_token)),
        Item::Struct(i) => fields_end(&i.fields, &i.semi_token),
        Item::Trait(i) => Some(i.brace_token.span.close().end()),
        Item::TraitAlias(i) => Some(semi_end(&i.semi_token)),
        Item::Type(i) => Some(semi_end(&i.semi_token)),
        Item::Union(i) => Some(i.fields.brace_token.span.close().end()),
        Item::Use(i) => Some(semi_end(&i.semi_token)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// The structure of a file

/// A `use` in a module: what it binds or glob-imports.
#[derive(Debug, Clone)]
pub struct Use {
    pub path: Written,
    pub public: bool,
    pub at: LineColumn,
}

/// What one module of a file defines and binds.
#[derive(Debug, Default)]
pub struct Names {
    /// Names of the items it defines, child modules included.
    pub items: BTreeSet<String>,
    /// `use` bindings (in item lists and in bodies) by the name they bind.
    pub uses: BTreeMap<String, Use>,
    /// `use path::*`.
    pub globs: Vec<Use>,
}

/// A `mod name;` whose body is another file.
#[derive(Debug)]
pub struct ChildDecl {
    /// The inline modules it is declared in, within the file.
    pub inline: Vec<String>,
    pub name: String,
    pub path_attr: Option<String>,
    /// Its `cfg` holds only for tests or a test-only feature.
    pub test_gated: bool,
    pub at: LineColumn,
}

/// What `syn` gives of one file.
#[derive(Debug, Default)]
pub struct FileItems {
    pub children: Vec<ChildDecl>,
    /// Inline modules (in item lists and in bodies): path within the file and brace extent.
    pub inline: Vec<(Vec<String>, LineColumn, LineColumn)>,
    /// Extents of test-only items.
    tests: Vec<(LineColumn, LineColumn)>,
    /// What each module of the file binds, by its path within the file.
    pub names: BTreeMap<Vec<String>, Names>,
    /// `pub mod` items (inline or not) of the file's own module.
    pub pub_mods: Vec<(String, LineColumn)>,
    /// Names bound by `extern crate self as <name>`.
    pub aliases: BTreeSet<String>,
    /// Every `mod` keyword the walk accounts for.
    placed: BTreeSet<LineColumn>,
}

impl FileItems {
    pub fn read(file: &syn::File) -> Result<Self, String> {
        let mut out = Self::default();
        out.names.insert(Vec::new(), Names::default());
        for item in &file.items {
            out.item(item, &[], false)?;
        }
        Ok(out)
    }

    /// Whether `at` is in test-only code.
    pub fn in_test(&self, at: LineColumn) -> bool {
        self.tests
            .iter()
            .any(|(start, end)| *start <= at && at < *end)
    }

    /// The innermost inline module holding `at`, as a path within the file.
    pub fn module_at(&self, at: LineColumn) -> &[String] {
        self.inline
            .iter()
            .filter(|(_, start, end)| *start <= at && at < *end)
            .max_by_key(|(_, start, _)| *start)
            .map_or(&[], |(path, _, _)| path.as_slice())
    }

    /// Whether the `mod` keyword at `at` was accounted for.
    pub fn placed(&self, at: LineColumn) -> bool {
        self.placed.contains(&at)
    }

    fn names_mut(&mut self, module: &[String]) -> &mut Names {
        self.names.entry(module.to_vec()).or_default()
    }

    fn test_item(
        &mut self,
        attrs: &[syn::Attribute],
        end: Option<LineColumn>,
    ) -> Result<(), String> {
        let start = attrs[0].pound_token.span.start();
        let end = end.ok_or_else(|| {
            format!(
                ":{}:{}: a test-only item whose end cannot be found",
                start.line,
                start.column + 1
            )
        })?;
        self.tests.push((start, end));
        Ok(())
    }

    fn item(&mut self, item: &syn::Item, module: &[String], in_body: bool) -> Result<(), String> {
        use syn::Item;
        let attrs = item_attrs(item);
        if test_only(attrs) {
            if let Item::Mod(m) = item {
                self.placed.insert(m.mod_token.span.start());
            }
            return self.test_item(attrs, item_end(item));
        }
        if !in_body {
            if let Some(name) = item_name(item) {
                self.names_mut(module).items.insert(name);
            }
        }
        match item {
            Item::Mod(m) => return self.module(m, module, in_body),
            Item::Fn(f) => return self.block(&f.block, module),
            Item::Const(c) => return self.expr(&c.expr, module),
            Item::Static(s) => return self.expr(&s.expr, module),
            Item::Impl(i) => {
                for member in &i.items {
                    self.impl_item(member, module)?;
                }
            }
            Item::Trait(t) => {
                for member in &t.items {
                    self.trait_item(member, module)?;
                }
            }
            Item::Use(u) if u.leading_colon.is_none() => {
                let public = matches!(u.vis, syn::Visibility::Public(_));
                let at = u.use_token.span.start();
                self.use_tree(&u.tree, Building::default(), module, public, at);
            }
            Item::ExternCrate(e) if e.ident == "self" => {
                if let Some((_, alias)) = &e.rename {
                    self.aliases.insert(unraw(alias));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn module(&mut self, m: &syn::ItemMod, module: &[String], in_body: bool) -> Result<(), String> {
        let name = unraw(&m.ident);
        let at = m.mod_token.span.start();
        self.placed.insert(at);
        if module.is_empty() && !in_body && matches!(m.vis, syn::Visibility::Public(_)) {
            self.pub_mods.push((name.clone(), at));
        }
        match &m.content {
            Some((brace, content)) => {
                let mut path = module.to_vec();
                path.push(name);
                let extent = (brace.span.open().start(), brace.span.close().end());
                self.inline.push((path.clone(), extent.0, extent.1));
                self.names_mut(&path);
                for item in content {
                    self.item(item, &path, false)?;
                }
            }
            None => self.children.push(ChildDecl {
                inline: module.to_vec(),
                name,
                path_attr: path_attr(&m.attrs),
                test_gated: gated_for_tests(&m.attrs),
                at,
            }),
        }
        Ok(())
    }

    fn impl_item(&mut self, member: &syn::ImplItem, module: &[String]) -> Result<(), String> {
        match member {
            syn::ImplItem::Fn(f) if test_only(&f.attrs) => {
                self.test_item(&f.attrs, Some(f.block.brace_token.span.close().end()))
            }
            syn::ImplItem::Fn(f) => self.block(&f.block, module),
            syn::ImplItem::Const(c) if test_only(&c.attrs) => {
                self.test_item(&c.attrs, Some(c.semi_token.span.end()))
            }
            syn::ImplItem::Const(c) => self.expr(&c.expr, module),
            syn::ImplItem::Type(t) if test_only(&t.attrs) => {
                self.test_item(&t.attrs, Some(t.semi_token.span.end()))
            }
            _ => Ok(()),
        }
    }

    fn trait_item(&mut self, member: &syn::TraitItem, module: &[String]) -> Result<(), String> {
        match member {
            syn::TraitItem::Fn(f) if test_only(&f.attrs) => {
                let end = match (&f.default, &f.semi_token) {
                    (Some(block), _) => Some(block.brace_token.span.close().end()),
                    (None, Some(semi)) => Some(semi.span.end()),
                    (None, None) => None,
                };
                self.test_item(&f.attrs, end)
            }
            syn::TraitItem::Fn(syn::TraitItemFn {
                default: Some(block),
                ..
            }) => self.block(block, module),
            syn::TraitItem::Const(syn::TraitItemConst {
                attrs,
                default: Some((_, expr)),
                ..
            }) if !test_only(attrs) => self.expr(expr, module),
            _ => Ok(()),
        }
    }

    fn block(&mut self, block: &syn::Block, module: &[String]) -> Result<(), String> {
        for stmt in &block.stmts {
            match stmt {
                syn::Stmt::Item(item) => self.item(item, module, true)?,
                syn::Stmt::Local(local) => {
                    if let Some(init) = &local.init {
                        self.expr(&init.expr, module)?;
                        if let Some((_, diverge)) = &init.diverge {
                            self.expr(diverge, module)?;
                        }
                    }
                }
                syn::Stmt::Expr(expr, _) => self.expr(expr, module)?,
                syn::Stmt::Macro(_) => {}
            }
        }
        Ok(())
    }

    /// Walks the blocks of an expression for items (a `mod` or `use` in a closure, a branch..).
    /// A `mod` this misses is caught as unplaced, so the walk need not be complete.
    fn expr(&mut self, expr: &syn::Expr, module: &[String]) -> Result<(), String> {
        use std::iter::once;
        use syn::Expr as E;
        fn boxed(e: &Option<Box<syn::Expr>>) -> Vec<&syn::Expr> {
            e.as_deref().into_iter().collect()
        }
        let (blocks, exprs): (Vec<&syn::Block>, Vec<&syn::Expr>) = match expr {
            E::Array(e) => (vec![], e.elems.iter().collect()),
            E::Assign(e) => (vec![], vec![&*e.left, &*e.right]),
            E::Async(e) => (vec![&e.block], vec![]),
            E::Await(e) => (vec![], vec![&*e.base]),
            E::Binary(e) => (vec![], vec![&*e.left, &*e.right]),
            E::Block(e) => (vec![&e.block], vec![]),
            E::Break(e) => (vec![], boxed(&e.expr)),
            E::Call(e) => (vec![], once(&*e.func).chain(&e.args).collect()),
            E::Cast(e) => (vec![], vec![&*e.expr]),
            E::Closure(e) => (vec![], vec![&*e.body]),
            E::Const(e) => (vec![&e.block], vec![]),
            E::Field(e) => (vec![], vec![&*e.base]),
            E::ForLoop(e) => (vec![&e.body], vec![&*e.expr]),
            E::Group(e) => (vec![], vec![&*e.expr]),
            E::If(e) => {
                let otherwise = e.else_branch.iter().map(|(_, x)| &**x);
                (
                    vec![&e.then_branch],
                    once(&*e.cond).chain(otherwise).collect(),
                )
            }
            E::Index(e) => (vec![], vec![&*e.expr, &*e.index]),
            E::Let(e) => (vec![], vec![&*e.expr]),
            E::Loop(e) => (vec![&e.body], vec![]),
            E::Match(e) => {
                let arms = e.arms.iter().flat_map(|arm| {
                    let guard = arm.guard.iter().map(|(_, g)| &**g);
                    guard.chain(once(&*arm.body))
                });
                (vec![], once(&*e.expr).chain(arms).collect())
            }
            E::MethodCall(e) => (vec![], once(&*e.receiver).chain(&e.args).collect()),
            E::Paren(e) => (vec![], vec![&*e.expr]),
            E::Range(e) => (
                vec![],
                [&e.start, &e.end].into_iter().flat_map(boxed).collect(),
            ),
            E::Reference(e) => (vec![], vec![&*e.expr]),
            E::Repeat(e) => (vec![], vec![&*e.expr, &*e.len]),
            E::Return(e) => (vec![], boxed(&e.expr)),
            E::Struct(e) => {
                let fields = e.fields.iter().map(|f| &f.expr);
                (vec![], fields.chain(e.rest.as_deref()).collect())
            }
            E::Try(e) => (vec![], vec![&*e.expr]),
            E::TryBlock(e) => (vec![&e.block], vec![]),
            E::Tuple(e) => (vec![], e.elems.iter().collect()),
            E::Unary(e) => (vec![], vec![&*e.expr]),
            E::Unsafe(e) => (vec![&e.block], vec![]),
            E::While(e) => (vec![&e.body], vec![&*e.cond]),
            E::Yield(e) => (vec![], boxed(&e.expr)),
            _ => (vec![], vec![]),
        };
        for block in blocks {
            self.block(block, module)?;
        }
        exprs.into_iter().try_for_each(|e| self.expr(e, module))
    }

    fn use_tree(
        &mut self,
        tree: &syn::UseTree,
        mut path: Building,
        module: &[String],
        public: bool,
        at: LineColumn,
    ) {
        let bind = |name: String, path: Building, names: &mut Names| {
            if let Some(path) = path.done() {
                names.uses.insert(name, Use { path, public, at });
            }
        };
        match tree {
            syn::UseTree::Path(p) => {
                path.push(&unraw(&p.ident));
                self.use_tree(&p.tree, path, module, public, at);
            }
            syn::UseTree::Name(n) => {
                let word = unraw(&n.ident);
                // `x::{self}` binds `x`.
                let name = if word == "self" {
                    path.segments.last().cloned()
                } else {
                    path.push(&word);
                    Some(word)
                };
                if let Some(name) = name {
                    bind(name, path, self.names_mut(module));
                }
            }
            syn::UseTree::Rename(r) => {
                let word = unraw(&r.ident);
                if word != "self" {
                    path.push(&word);
                }
                let name = unraw(&r.rename);
                if name != "_" {
                    bind(name, path, self.names_mut(module));
                }
            }
            syn::UseTree::Glob(_) => {
                if let Some(path) = path.done() {
                    let glob = Use { path, public, at };
                    self.names_mut(module).globs.push(glob);
                }
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.use_tree(tree, path.clone(), module, public, at);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The paths the tokens name

/// A path the tokens name and where it starts; `restriction` for the path of `pub(in path)`.
#[derive(Debug)]
pub struct Named {
    pub at: LineColumn,
    pub path: Written,
    pub restriction: bool,
}

/// `::` at `index`.
fn colons(tokens: &[TokenTree], index: usize) -> bool {
    matches!(
        (tokens.get(index), tokens.get(index + 1)),
        (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
            if a.as_char() == ':' && a.spacing() == Spacing::Joint && b.as_char() == ':'
    )
}

fn punct(token: Option<&TokenTree>, c: char) -> Option<&proc_macro2::Punct> {
    match token {
        Some(TokenTree::Punct(p)) if p.as_char() == c => Some(p),
        _ => None,
    }
}

/// Whether the ident at `at` starts a path: `name::` not after `::`, a method's `.` or a macro
/// variable's `$` (except `$crate`).
fn path_start(tokens: &[TokenTree], at: usize) -> bool {
    let TokenTree::Ident(word) = &tokens[at] else {
        return false;
    };
    if !colons(tokens, at + 1) || (at >= 2 && colons(tokens, at - 2)) {
        return false;
    }
    let before = at.checked_sub(1).and_then(|i| tokens.get(i));
    if punct(before, '.').is_some() {
        // `..crate::X` is a range; `x.name::<T>()` is a method.
        let range = at.checked_sub(2).and_then(|i| punct(tokens.get(i), '.'));
        return range.is_some_and(|dot| dot.spacing() == Spacing::Joint);
    }
    punct(before, '$').is_none() || word == "crate"
}

/// The group of `pub(..)` as a visibility restriction: `Some(path tokens)` for `in path`,
/// `Some([])` for `crate`, `self` and `super`, `None` for anything else (a macro's argument).
fn restriction(group: &proc_macro2::Group) -> Option<Vec<TokenTree>> {
    if group.delimiter() != Delimiter::Parenthesis {
        return None;
    }
    let tokens: Vec<TokenTree> = group.stream().into_iter().collect();
    match tokens.as_slice() {
        [TokenTree::Ident(word)] if word == "crate" || word == "self" || word == "super" => {
            Some(Vec::new())
        }
        [TokenTree::Ident(word), rest @ ..] if word == "in" && !rest.is_empty() => {
            let simple = rest.iter().all(|token| match token {
                TokenTree::Ident(_) => true,
                TokenTree::Punct(p) => p.as_char() == ':',
                _ => false,
            });
            simple.then(|| rest.to_vec())
        }
        _ => None,
    }
}

/// The paths a path or `use` tree starting at `at` names, and the index after it: a `{..}`
/// group is read here with the prefix (and its `super`s) it extends, never on its own.
fn read_path(tokens: &[TokenTree], mut at: usize, mut path: Building) -> (Vec<Written>, usize) {
    loop {
        match tokens.get(at) {
            Some(TokenTree::Ident(word)) => {
                path.push(&unraw(word));
                if !colons(tokens, at + 1) {
                    return (path.done().into_iter().collect(), at + 1);
                }
                at += 3;
            }
            Some(TokenTree::Group(group))
                if group.delimiter() == Delimiter::Brace && path.head.is_some() =>
            {
                let mut out = Vec::new();
                for part in split_commas(group.stream()) {
                    let part: Vec<TokenTree> = part.into_iter().collect();
                    out.extend(read_path(&part, 0, path.clone()).0);
                }
                return (out, at + 1);
            }
            _ => return (path.done().into_iter().collect(), at),
        }
    }
}

/// Every path `tokens` name, in `use` trees, signatures, bodies and macro definitions alike.
pub fn scan(tokens: &[TokenTree], out: &mut Vec<Named>) {
    let mut at = 0;
    while at < tokens.len() {
        match &tokens[at] {
            TokenTree::Group(group) => {
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                scan(&inner, out);
            }
            TokenTree::Ident(word) if word == "pub" => {
                let group = match tokens.get(at + 1) {
                    Some(TokenTree::Group(group)) => restriction(group),
                    _ => None,
                };
                if let Some(path) = group {
                    let start = word.span().start();
                    let (paths, _) = read_path(&path, 0, Building::default());
                    out.extend(paths.into_iter().map(|path| Named {
                        at: start,
                        path,
                        restriction: true,
                    }));
                    at += 2;
                    continue;
                }
            }
            TokenTree::Ident(word) if path_start(tokens, at) => {
                let start = word.span().start();
                let (paths, end) = read_path(tokens, at, Building::default());
                out.extend(paths.into_iter().map(|path| Named {
                    at: start,
                    path,
                    restriction: false,
                }));
                at = end;
                continue;
            }
            _ => {}
        }
        at += 1;
    }
}

/// Where `tokens` hold a `mod` keyword, and where an `include!`.
pub fn sites(tokens: &[TokenTree], mods: &mut Vec<LineColumn>, includes: &mut Vec<LineColumn>) {
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => {
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                sites(&inner, mods, includes);
            }
            TokenTree::Ident(word) if word == "mod" => mods.push(word.span().start()),
            TokenTree::Ident(word)
                if word == "include" && punct(tokens.get(index + 1), '!').is_some() =>
            {
                includes.push(word.span().start());
            }
            _ => {}
        }
    }
}
