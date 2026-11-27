//! What `syn` says of one file: its modules, what each module and each block binds, the
//! extents of its test-only items, and the layout rules it breaks.
//!
//! A block (a body, a `const` block, an enum discriminant, a group of a macro) that binds names
//! is a scope of its own (see [`parse::marker`]): a `use` or `mod` in a body never shadows the
//! module's names, and a path in a body sees the blocks around it before its module.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{LineColumn, TokenTree};

use super::parse::{self, is_marker, unraw, Building, Written};
use super::scan;

/// A `use` in a scope: what it binds or glob-imports.
#[derive(Debug, Clone)]
pub struct Use {
    pub path: Written,
    pub public: bool,
    pub at: LineColumn,
}

/// What one scope of a file defines and binds.
#[derive(Debug, Default)]
pub struct Names {
    /// Names of the items it defines, modules included.
    pub items: BTreeSet<String>,
    /// `use` bindings by the name they bind: more than one for `cfg` alternatives
    /// (`#[cfg(..)] use linux as native;`), each a path the name may stand for.
    pub uses: BTreeMap<String, Vec<Use>>,
    /// `use path::*`.
    pub globs: Vec<Use>,
}

/// A `mod name;` whose body is another file.
#[derive(Debug)]
pub struct ChildDecl {
    /// The scope it is declared in, within the file (inline modules and blocks).
    pub scope: Vec<String>,
    pub name: String,
    pub path_attr: Option<String>,
    pub public: bool,
    /// Its `cfg` holds only for tests or a test-only feature.
    pub test_gated: bool,
    pub at: LineColumn,
}

/// An inline `mod name { .. }`.
#[derive(Debug)]
pub struct Inline {
    /// Its scope path within the file.
    pub key: Vec<String>,
    /// Its brace extent.
    pub start: LineColumn,
    pub end: LineColumn,
    pub public: bool,
}

/// A `use` item's bindings: the name each binds (`None` for a glob).
type Bindings = Vec<(Option<String>, Use)>;

/// What `syn` gives of one file.
#[derive(Debug, Default)]
pub struct FileItems {
    pub children: Vec<ChildDecl>,
    pub inline: Vec<Inline>,
    /// Extents of test-only items.
    tests: Vec<(LineColumn, LineColumn)>,
    /// Block scopes: scope path within the file and brace extent.
    blocks: Vec<(Vec<String>, LineColumn, LineColumn)>,
    /// What each scope of the file binds, by its path within the file.
    pub names: BTreeMap<Vec<String>, Names>,
    /// `pub mod` items (inline or not) of the file's own module.
    pub pub_mods: Vec<(String, LineColumn)>,
    /// Names bound by `extern crate self as <name>`.
    pub aliases: BTreeSet<String>,
    /// Layout rules broken in the file, by where.
    pub problems: Vec<(LineColumn, String)>,
    /// Every `mod` keyword the walk accounts for.
    placed: BTreeSet<LineColumn>,
    /// Every `use` keyword the walk read.
    placed_uses: BTreeSet<LineColumn>,
}

impl FileItems {
    pub fn read(file: &syn::File, tokens: &[TokenTree]) -> Result<Self, String> {
        let mut out = Self::default();
        out.names.insert(Vec::new(), Names::default());
        for item in &file.items {
            out.item(item, &[])?;
        }
        out.macro_uses(tokens);
        let names = &out.names;
        out.blocks.retain(|(key, _, _)| names.contains_key(key));
        Ok(out)
    }

    /// Whether `at` is in test-only code.
    pub fn in_test(&self, at: LineColumn) -> bool {
        self.tests
            .iter()
            .any(|(start, end)| *start <= at && at < *end)
    }

    /// The innermost scope holding `at` (an inline module or a block that binds names), as a
    /// path within the file.
    pub fn scope_at(&self, at: LineColumn) -> &[String] {
        let inline = self.inline.iter().map(|m| (&m.key, m.start, m.end));
        let blocks = self
            .blocks
            .iter()
            .map(|(key, start, end)| (key, *start, *end));
        inline
            .chain(blocks)
            .filter(|(_, start, end)| *start <= at && at < *end)
            .max_by_key(|(_, start, _)| *start)
            .map_or(&[], |(key, _, _)| key.as_slice())
    }

    /// Whether the `mod` keyword at `at` was accounted for.
    pub fn placed(&self, at: LineColumn) -> bool {
        self.placed.contains(&at)
    }

    fn names_mut(&mut self, scope: &[String]) -> &mut Names {
        self.names.entry(scope.to_vec()).or_default()
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

    fn item(&mut self, item: &syn::Item, scope: &[String]) -> Result<(), String> {
        use syn::Item;
        let attrs = parse::item_attrs(item);
        if parse::test_only(attrs) {
            if let Item::Mod(m) = item {
                self.placed.insert(m.mod_token.span.start());
            }
            return self.test_item(attrs, parse::item_end(item));
        }
        if let Some(name) = parse::item_name(item) {
            self.names_mut(scope).items.insert(name);
        }
        match item {
            Item::Mod(m) => self.module(m, scope),
            Item::Fn(f) => {
                self.signature(&f.sig, scope)?;
                self.block(&f.block, scope)
            }
            Item::Const(c) => {
                self.ty(&c.ty, scope)?;
                self.expr(&c.expr, scope)
            }
            Item::Static(s) => {
                self.ty(&s.ty, scope)?;
                self.expr(&s.expr, scope)
            }
            Item::Struct(s) => s.fields.iter().try_for_each(|f| self.ty(&f.ty, scope)),
            Item::Union(u) => u
                .fields
                .named
                .iter()
                .try_for_each(|f| self.ty(&f.ty, scope)),
            Item::Enum(e) => e.variants.iter().try_for_each(|variant| {
                variant
                    .fields
                    .iter()
                    .try_for_each(|f| self.ty(&f.ty, scope))?;
                match &variant.discriminant {
                    Some((_, discriminant)) => self.expr(discriminant, scope),
                    None => Ok(()),
                }
            }),
            Item::Type(t) => self.ty(&t.ty, scope),
            Item::Impl(i) => i.items.iter().try_for_each(|m| self.impl_item(m, scope)),
            Item::Trait(t) => t.items.iter().try_for_each(|m| self.trait_item(m, scope)),
            Item::Use(u) => {
                let at = u.use_token.span.start();
                self.placed_uses.insert(at);
                let public = matches!(u.vis, syn::Visibility::Public(_));
                let mut found = Vec::new();
                self.use_tree(&u.tree, Building::of_use(u), public, at, &mut found);
                self.bind(scope, found);
                Ok(())
            }
            Item::ExternCrate(e) if e.ident == "self" => {
                if let Some((_, alias)) = &e.rename {
                    self.aliases.insert(unraw(alias));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn module(&mut self, m: &syn::ItemMod, scope: &[String]) -> Result<(), String> {
        let name = unraw(&m.ident);
        let at = m.mod_token.span.start();
        self.placed.insert(at);
        let public = matches!(m.vis, syn::Visibility::Public(_));
        if scope.is_empty() && public {
            self.pub_mods.push((name.clone(), at));
        }
        let paths = parse::path_attrs(&m.attrs);
        if paths.wrapped {
            self.problems.push((at, format!(
                "`mod {name}` takes a `path` from `cfg_attr`, which can load a file from anywhere; write `#[path]` directly or drop it"
            )));
        }
        let mut key = scope.to_vec();
        key.push(name.clone());
        let Some((brace, content)) = &m.content else {
            if paths.direct.len() > 1 || paths.direct.iter().any(Option::is_none) {
                self.problems.push((
                    at,
                    format!("`mod {name}` has a `#[path]` that is not one string literal"),
                ));
            }
            self.children.push(ChildDecl {
                scope: scope.to_vec(),
                name,
                path_attr: paths.direct.into_iter().flatten().next(),
                public,
                test_gated: parse::gated_for_tests(&m.attrs),
                at,
            });
            return Ok(());
        };
        if !paths.direct.is_empty() {
            self.problems.push((at, format!(
                "`#[path]` on the inline module `{name}` moves where its `mod x;` files load from; drop it"
            )));
        }
        self.inline.push(Inline {
            key: key.clone(),
            start: brace.span.open().start(),
            end: brace.span.close().end(),
            public,
        });
        self.names_mut(&key);
        content.iter().try_for_each(|item| self.item(item, &key))
    }

    fn signature(&mut self, sig: &syn::Signature, scope: &[String]) -> Result<(), String> {
        for input in &sig.inputs {
            if let syn::FnArg::Typed(typed) = input {
                self.ty(&typed.ty, scope)?;
            }
        }
        match &sig.output {
            syn::ReturnType::Type(_, ty) => self.ty(ty, scope),
            syn::ReturnType::Default => Ok(()),
        }
    }

    fn impl_item(&mut self, member: &syn::ImplItem, scope: &[String]) -> Result<(), String> {
        match member {
            syn::ImplItem::Fn(f) if parse::test_only(&f.attrs) => {
                self.test_item(&f.attrs, Some(f.block.brace_token.span.close().end()))
            }
            syn::ImplItem::Fn(f) => {
                self.signature(&f.sig, scope)?;
                self.block(&f.block, scope)
            }
            syn::ImplItem::Const(c) if parse::test_only(&c.attrs) => {
                self.test_item(&c.attrs, Some(c.semi_token.span.end()))
            }
            syn::ImplItem::Const(c) => {
                self.ty(&c.ty, scope)?;
                self.expr(&c.expr, scope)
            }
            syn::ImplItem::Type(t) if parse::test_only(&t.attrs) => {
                self.test_item(&t.attrs, Some(t.semi_token.span.end()))
            }
            syn::ImplItem::Type(t) => self.ty(&t.ty, scope),
            _ => Ok(()),
        }
    }

    fn trait_item(&mut self, member: &syn::TraitItem, scope: &[String]) -> Result<(), String> {
        match member {
            syn::TraitItem::Fn(f) if parse::test_only(&f.attrs) => {
                let end = match (&f.default, &f.semi_token) {
                    (Some(block), _) => Some(block.brace_token.span.close().end()),
                    (None, Some(semi)) => Some(semi.span.end()),
                    (None, None) => None,
                };
                self.test_item(&f.attrs, end)
            }
            syn::TraitItem::Fn(f) => {
                self.signature(&f.sig, scope)?;
                match &f.default {
                    Some(block) => self.block(block, scope),
                    None => Ok(()),
                }
            }
            syn::TraitItem::Const(syn::TraitItemConst {
                attrs,
                default: Some((_, expr)),
                ..
            }) if !parse::test_only(attrs) => self.expr(expr, scope),
            _ => Ok(()),
        }
    }

    /// A block: a scope of its own, in `scope`.
    fn block(&mut self, block: &syn::Block, scope: &[String]) -> Result<(), String> {
        let (start, end) = (
            block.brace_token.span.open().start(),
            block.brace_token.span.close().end(),
        );
        let mut key = scope.to_vec();
        key.push(parse::marker(start));
        self.blocks.push((key.clone(), start, end));
        for stmt in &block.stmts {
            match stmt {
                syn::Stmt::Item(item) => self.item(item, &key)?,
                syn::Stmt::Local(local) => {
                    if let Some(init) = &local.init {
                        self.expr(&init.expr, &key)?;
                        if let Some((_, diverge)) = &init.diverge {
                            self.expr(diverge, &key)?;
                        }
                    }
                }
                syn::Stmt::Expr(expr, _) => self.expr(expr, &key)?,
                // Its `use` items are read from the tokens (`macro_uses`).
                syn::Stmt::Macro(_) => {}
            }
        }
        Ok(())
    }

    /// Walks a type for the expressions in it (an array length, a `const` argument).
    fn ty(&mut self, ty: &syn::Type, scope: &[String]) -> Result<(), String> {
        use syn::Type as T;
        match ty {
            T::Array(t) => {
                self.ty(&t.elem, scope)?;
                self.expr(&t.len, scope)
            }
            T::Group(t) => self.ty(&t.elem, scope),
            T::Paren(t) => self.ty(&t.elem, scope),
            T::Ptr(t) => self.ty(&t.elem, scope),
            T::Reference(t) => self.ty(&t.elem, scope),
            T::Slice(t) => self.ty(&t.elem, scope),
            T::Tuple(t) => t.elems.iter().try_for_each(|elem| self.ty(elem, scope)),
            T::Path(t) => {
                if let Some(qself) = &t.qself {
                    self.ty(&qself.ty, scope)?;
                }
                for segment in &t.path.segments {
                    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                        continue;
                    };
                    for arg in &args.args {
                        match arg {
                            syn::GenericArgument::Type(t) => self.ty(t, scope)?,
                            syn::GenericArgument::Const(e) => self.expr(e, scope)?,
                            _ => {}
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Walks the blocks of an expression for items (a `mod` or `use` in a closure, a branch..).
    /// A `mod` this misses is caught as unplaced and a `use` is read from the tokens, so the
    /// walk need not be complete.
    fn expr(&mut self, expr: &syn::Expr, scope: &[String]) -> Result<(), String> {
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
            self.block(block, scope)?;
        }
        exprs.into_iter().try_for_each(|e| self.expr(e, scope))
    }

    /// What a `use` tree binds; a `use` of `include` is a layout problem.
    fn use_tree(
        &mut self,
        tree: &syn::UseTree,
        mut path: Building,
        public: bool,
        at: LineColumn,
        out: &mut Bindings,
    ) {
        let mut bind = |name: Option<String>, path: Building| {
            if let Some(path) = path.done() {
                out.push((name, Use { path, public, at }));
            }
        };
        let mut include = false;
        match tree {
            syn::UseTree::Path(p) => {
                path.push(&unraw(&p.ident));
                self.use_tree(&p.tree, path, public, at, out);
            }
            syn::UseTree::Name(n) => {
                let word = unraw(&n.ident);
                include = word == "include";
                // `x::{self}` binds `x`.
                let name = if word == "self" {
                    path.segments.last().cloned()
                } else {
                    path.push(&word);
                    Some(word)
                };
                if name.is_some() {
                    bind(name, path);
                }
            }
            syn::UseTree::Rename(r) => {
                let word = unraw(&r.ident);
                include = word == "include";
                if word != "self" {
                    path.push(&word);
                }
                let name = unraw(&r.rename);
                if name != "_" {
                    bind(Some(name), path);
                }
            }
            syn::UseTree::Glob(_) => bind(None, path),
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.use_tree(tree, path.clone(), public, at, out);
                }
            }
        }
        if include {
            self.problems.push((at, "a `use` of `include` lets a macro of another name paste in code no module owns; declare a module instead".to_owned()));
        }
    }

    /// Binds a `use` item's names in `scope`. A name bound again in one scope (`cfg`
    /// alternatives) keeps every path: a later binding never hides an earlier one.
    fn bind(&mut self, scope: &[String], found: Bindings) {
        let names = self.names_mut(scope);
        for (name, found) in found {
            match name {
                None => names.globs.push(found),
                Some(name) => {
                    let bound = names.uses.entry(name).or_default();
                    if bound.iter().all(|bound| bound.path != found.path) {
                        bound.push(found);
                    }
                }
            }
        }
    }

    /// The scope that binds `name` for a path in `scope`: `scope` or a block around it, within
    /// its module.
    fn binder(&self, scope: &[String], name: &str) -> Option<Vec<String>> {
        let mut key = scope.to_vec();
        loop {
            if let Some(names) = self.names.get(&key) {
                if names.uses.contains_key(name) || names.items.contains(name) {
                    return Some(key);
                }
            }
            match key.last() {
                Some(last) if is_marker(last) => key.pop(),
                _ => return None,
            };
        }
    }

    /// Binds the `use` items `syn` did not read: those in macro bodies and calls, and in the
    /// expressions the walk skips. One in a macro call binds where the call expands (the scope
    /// around the outermost call); when a scope there or around it already binds the name, it
    /// binds there as one more alternative, so that neither hides the other. Any other binds in
    /// the group it is written in.
    fn macro_uses(&mut self, tokens: &[TokenTree]) {
        let (mut found, mut aliases) = (Vec::new(), BTreeSet::new());
        let skip = |at: LineColumn| self.placed_uses.contains(&at) || self.in_test(at);
        scan::found_uses(tokens, &skip, &mut found, &mut aliases);
        self.aliases.extend(aliases);
        // Outer groups first, so a group's scope nests in the scope of the group around it.
        let mut groups: Vec<(LineColumn, LineColumn)> = found
            .iter()
            .filter(|f| f.call.is_none())
            .map(|f| f.group)
            .collect();
        groups.sort_by(|a, b| (a.0, b.1).cmp(&(b.0, a.1)));
        groups.dedup();
        for (start, end) in groups {
            if self.blocks.iter().all(|(_, s, _)| *s != start) {
                let mut key = self.scope_at(start).to_vec();
                key.push(parse::marker(start));
                self.blocks.push((key, start, end));
            }
        }
        for found in found {
            let mut bindings = Vec::new();
            let public = matches!(found.item.vis, syn::Visibility::Public(_));
            let path = Building::of_use(&found.item);
            self.use_tree(&found.item.tree, path, public, found.at, &mut bindings);
            let Some(call) = found.call else {
                let group = self.blocks.iter().find(|(_, s, _)| *s == found.group.0);
                let key = group.map(|(key, _, _)| key.clone()).unwrap_or_default();
                self.bind(&key, bindings);
                continue;
            };
            let key = self.scope_at(call).to_vec();
            for (name, binding) in bindings {
                let binder = name.as_deref().and_then(|name| self.binder(&key, name));
                self.bind(binder.as_deref().unwrap_or(&key), vec![(name, binding)]);
            }
        }
    }
}
