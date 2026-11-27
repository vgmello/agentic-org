//! The reader's shared vocabulary: how a written path starts, how a scope is named, and what an
//! item's attributes say (`cfg`, `#[path]`).
//!
//! Every extent is a pair of `proc_macro2` line/column positions, so two items on one line are
//! told apart. An extent the reader cannot work out is an error, never "to the end of the file".

use proc_macro2::{LineColumn, TokenStream, TokenTree};

/// Cargo features that exist only for tests: a module gated on one may use a test file name.
const TEST_FEATURES: [&str; 2] = ["testkit", "test-support"];

/// Strict and reserved keywords of Rust 2021 that never start or continue a path (`crate`,
/// `self`, `super` and `Self` do).
const KEYWORDS: [&str; 47] = [
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where", "while",
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try", "typeof",
    "unsized", "virtual", "yield",
];

/// How a written path starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    /// `crate::` or `$crate::`.
    Root,
    /// `self::` (zero) or as many `super::` (after an optional `self::`).
    Up(usize),
    /// A plain name (the first segment), which the scope's own names resolve.
    Local,
    /// `::name::`, a crate of the extern prelude (the crate itself when `name` is an
    /// `extern crate self` alias).
    Extern,
}

/// A path as written in a scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub head: Head,
    pub segments: Vec<String>,
}

/// A path being read segment by segment.
#[derive(Debug, Clone, Default)]
pub struct Building {
    pub head: Option<Head>,
    pub segments: Vec<String>,
}

impl Building {
    /// A path after a leading `::`.
    pub fn extern_prelude() -> Self {
        Self {
            head: Some(Head::Extern),
            segments: Vec::new(),
        }
    }

    /// A `use` path: from the extern prelude after a leading `::`.
    pub fn of_use(item: &syn::ItemUse) -> Self {
        if item.leading_colon.is_some() {
            Self::extern_prelude()
        } else {
            Self::default()
        }
    }

    /// Only `self` and `super` read so far: another `super` still climbs.
    pub fn climbing(&self) -> bool {
        matches!(self.head, Some(Head::Up(_))) && self.segments.is_empty()
    }

    pub fn push(&mut self, word: &str) {
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

    pub fn done(self) -> Option<Written> {
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

/// Whether `word` is a keyword that is never part of a path (a raw `r#use` is a name).
pub fn is_keyword(word: &proc_macro2::Ident) -> bool {
    KEYWORDS.contains(&word.to_string().as_str())
}

// ---------------------------------------------------------------------------------------------
// Scopes

/// The scope-path segment of a block that binds names (a body, a `const` block, a group of a
/// macro): `{line:column}` of its opening brace, which no Rust name can be. A block's scope path
/// is the scope it is in plus its segment; a module declared in a block is that plus its name.
pub fn marker(at: LineColumn) -> String {
    format!("{{{}:{}}}", at.line, at.column)
}

pub fn is_marker(segment: &str) -> bool {
    segment.starts_with('{')
}

/// The module a scope is: the scope path without its trailing block segments.
pub fn module_of(scope: &[String]) -> &[String] {
    let end = scope
        .iter()
        .rposition(|segment| !is_marker(segment))
        .map_or(0, |last| last + 1);
    &scope[..end]
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

/// Whether an attribute path is the single name `name` (`r#name` included).
fn named(path: &syn::Path, name: &str) -> bool {
    path.leading_colon.is_none()
        && path.segments.len() == 1
        && unraw(&path.segments[0].ident) == name
}

fn cfg_requires(attrs: &[syn::Attribute], features: &[&str]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        syn::Meta::List(list) if named(&list.path, "cfg") => {
            requires(list.tokens.clone(), features)
        }
        _ => false,
    })
}

/// Whether the attributes make an item conditional: a `cfg` (in a `cfg_attr` too) may leave it
/// out, so it never hides a name bound around it.
pub fn conditional(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        syn::Meta::List(list) if named(&list.path, "cfg_attr") => wraps(list.tokens.clone(), "cfg"),
        meta => named(meta.path(), "cfg"),
    })
}

/// Whether the attributes make an item test-only (`#[cfg(test)]`, `#![cfg(test)]`).
pub fn test_only(attrs: &[syn::Attribute]) -> bool {
    cfg_requires(attrs, &[])
}

/// Whether a `mod` is compiled only for tests or a test-only feature.
pub fn gated_for_tests(attrs: &[syn::Attribute]) -> bool {
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

/// What the attributes of a `mod` say of where its file is.
#[derive(Debug, Default)]
pub struct PathAttrs {
    /// Each `#[path]`: its text, or `None` when it is not a string literal.
    pub direct: Vec<Option<String>>,
    /// A `cfg_attr` (at any depth) adds a `path`.
    pub wrapped: bool,
}

pub fn path_attrs(attrs: &[syn::Attribute]) -> PathAttrs {
    let mut out = PathAttrs::default();
    for attr in attrs {
        if named(attr.path(), "path") {
            out.direct.push(match &attr.meta {
                syn::Meta::NameValue(syn::MetaNameValue {
                    value:
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(text),
                            ..
                        }),
                    ..
                }) => Some(text.value()),
                _ => None,
            });
        } else if let syn::Meta::List(list) = &attr.meta {
            if named(&list.path, "cfg_attr") {
                out.wrapped |= wraps(list.tokens.clone(), "path");
            }
        }
    }
    out
}

/// Whether the attributes of `cfg_attr(predicate, attribute, ..)` hold a `name` (`path`,
/// `cfg`), nested `cfg_attr`s included.
fn wraps(tokens: TokenStream, name: &str) -> bool {
    split_commas(tokens).into_iter().skip(1).any(|attr| {
        let attr: Vec<TokenTree> = attr.into_iter().collect();
        match attr.as_slice() {
            [TokenTree::Ident(word), ..] if unraw(word) == name => true,
            [TokenTree::Ident(word), TokenTree::Group(inner)] if unraw(word) == "cfg_attr" => {
                wraps(inner.stream(), name)
            }
            _ => false,
        }
    })
}

pub fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
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

/// The name an item defines in its scope (child modules included), and whether it is in the
/// type namespace (a module, a type, a trait), the only one a path's `name::` prefix resolves
/// in: a function, constant, static or macro of that name never hides it.
pub fn item_name(item: &syn::Item) -> Option<(String, bool)> {
    use syn::Item;
    let (ident, ty) = match item {
        Item::Const(i) => (&i.ident, false),
        Item::Enum(i) => (&i.ident, true),
        Item::Fn(i) => (&i.sig.ident, false),
        Item::Macro(i) => (i.ident.as_ref()?, false),
        Item::Mod(i) => (&i.ident, true),
        Item::Static(i) => (&i.ident, false),
        Item::Struct(i) => (&i.ident, true),
        Item::Trait(i) => (&i.ident, true),
        Item::TraitAlias(i) => (&i.ident, true),
        Item::Type(i) => (&i.ident, true),
        Item::Union(i) => (&i.ident, true),
        _ => return None,
    };
    Some((unraw(ident), ty))
}

/// Where an item ends: the end of its closing brace or semicolon.
pub fn item_end(item: &syn::Item) -> Option<LineColumn> {
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
