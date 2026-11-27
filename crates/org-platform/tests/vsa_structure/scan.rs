//! What a file's tokens hold: every path they name, where a `mod` keyword or an `include!` is,
//! and the `use` items `syn` never reads (in macro bodies and macro calls).

use std::collections::BTreeSet;

use proc_macro2::{Delimiter, Group, LineColumn, Spacing, TokenStream, TokenTree};

use super::parse::{is_keyword, split_commas, unraw, Building, Written};

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

/// `$crate` at `at`, which a macro expands to `crate`.
fn dollar_crate(tokens: &[TokenTree], at: usize) -> bool {
    punct(tokens.get(at), '$').is_some()
        && matches!(tokens.get(at + 1), Some(TokenTree::Ident(word)) if word == "crate")
}

/// Whether the ident at `at` starts a path: a name (or `crate`, `self`, `super`) before `::`,
/// not after `::`, a lifetime's `'`, a method's `.` or a macro variable's `$` (except `$crate`).
fn path_start(tokens: &[TokenTree], at: usize) -> bool {
    let TokenTree::Ident(word) = &tokens[at] else {
        return false;
    };
    if is_keyword(word) || !colons(tokens, at + 1) || (at >= 2 && colons(tokens, at - 2)) {
        return false;
    }
    let before = at.checked_sub(1).and_then(|i| tokens.get(i));
    if punct(before, '\'').is_some() {
        return false;
    }
    if punct(before, '.').is_some() {
        // `..crate::X` is a range; `x.name::<T>()` is a method.
        let range = at.checked_sub(2).and_then(|i| punct(tokens.get(i), '.'));
        return range.is_some_and(|dot| dot.spacing() == Spacing::Joint);
    }
    punct(before, '$').is_none() || word == "crate"
}

/// Whether the `::` at `at` starts a path from the extern prelude (`::name::..`, or the
/// `use ::{..};` group): it does not follow a name. After anything else (`(`, `,`, `=`, `->`,
/// `=>`, `>`, a block, a keyword such as `use`, `return` or `in`, a lifetime) it starts one: a
/// `::name` read after a generic `>` names the crate only when `name` is an `extern crate self`
/// alias, never another path.
fn extern_start(tokens: &[TokenTree], at: usize) -> bool {
    let next = match tokens.get(at + 2) {
        Some(TokenTree::Ident(_)) => true,
        Some(TokenTree::Group(group)) => {
            group.delimiter() == Delimiter::Brace && after_use(tokens, at)
        }
        _ => false,
    };
    if !colons(tokens, at) || !next {
        return false;
    }
    match at.checked_sub(1).and_then(|i| tokens.get(i)) {
        Some(TokenTree::Ident(word)) => {
            let lifetime = at.checked_sub(2).and_then(|i| punct(tokens.get(i), '\''));
            lifetime.is_some() || is_keyword(word)
        }
        _ => true,
    }
}

/// The paths of `pub(..)` as a visibility: `Some([])` for `crate`, `self` and `super`, the path
/// of `in path` when it is a simple path read to its last token, `None` for anything else (a
/// macro's argument), whose tokens are then scanned like any others.
fn restriction(group: &Group) -> Option<Vec<Written>> {
    if group.delimiter() != Delimiter::Parenthesis {
        return None;
    }
    let tokens: Vec<TokenTree> = group.stream().into_iter().collect();
    match tokens.as_slice() {
        [TokenTree::Ident(word)] if word == "crate" || word == "self" || word == "super" => {
            Some(Vec::new())
        }
        [TokenTree::Ident(word), rest @ ..] if word == "in" => simple_path(rest),
        _ => None,
    }
}

/// `tokens` as one `SimplePath` (`::`? name (`::` name)*), or `None`.
fn simple_path(tokens: &[TokenTree]) -> Option<Vec<Written>> {
    let (start, path) = if colons(tokens, 0) {
        (2, Building::extern_prelude())
    } else {
        (0, Building::default())
    };
    let mut at = start;
    loop {
        match tokens.get(at) {
            Some(TokenTree::Ident(word)) if !is_keyword(word) => at += 1,
            _ => return None,
        }
        if at == tokens.len() {
            break;
        }
        if !colons(tokens, at) {
            return None;
        }
        at += 2;
    }
    let (paths, end) = read_path(tokens, start, path, Tree::No);
    (end == tokens.len()).then_some(paths)
}

/// Whether the path at `at` is the tree of a `use` item: right after `use` (`use $crate::..` in
/// a macro too).
fn after_use(tokens: &[TokenTree], at: usize) -> bool {
    let before = |back: usize| at.checked_sub(back).and_then(|i| tokens.get(i));
    let back = if punct(before(1), '$').is_some() {
        2
    } else {
        1
    };
    matches!(before(back), Some(TokenTree::Ident(word)) if word == "use")
}

/// What a path is read as, which says whether a `{..}` after its `name::` belongs to it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tree {
    /// Any path: `name::{..}` is no Rust path, so the group is left to the scan.
    No,
    /// The tree of a `use` item: a group of `use` trees right before its `;` ends it.
    Use,
    /// A tree in such a group, which [`use_group`] checked whole.
    Part,
}

/// Whether every part of the `{..}` after `path::` is a `use` tree (names, `*`, a nested group,
/// `.. as name`), so that reading it with the prefix accounts for each of its tokens.
fn use_group(group: &Group, path: &Building) -> bool {
    split_commas(group.stream()).into_iter().all(|part| {
        let part: Vec<TokenTree> = part.into_iter().collect();
        use_tree(&part, path.clone())
    })
}

/// Whether `tokens` are one `use` tree after `path::` (or a whole one when `path` is empty):
/// `crate` (`$crate` in a macro) never follows a prefix, `super` only climbs, `self` only climbs
/// or ends the tree.
fn use_tree(tokens: &[TokenTree], mut path: Building) -> bool {
    let mut at = usize::from(dollar_crate(tokens, 0));
    loop {
        match tokens.get(at) {
            Some(TokenTree::Punct(star)) if star.as_char() == '*' => return at + 1 == tokens.len(),
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                return at + 1 == tokens.len() && use_group(group, &path);
            }
            Some(TokenTree::Ident(word)) => {
                let (first, last) = (path.head.is_none(), !colons(tokens, at + 1));
                let valid = match word.to_string().as_str() {
                    "crate" => first,
                    "super" => first || path.climbing(),
                    "self" => first || path.climbing() || last,
                    _ => !is_keyword(word),
                };
                if !valid {
                    return false;
                }
                path.push(&unraw(word));
                if !last {
                    at += 3;
                    continue;
                }
                return match &tokens[at + 1..] {
                    [] => true,
                    [TokenTree::Ident(r#as), TokenTree::Ident(name)] => {
                        r#as == "as" && (name == "_" || !is_keyword(name))
                    }
                    _ => false,
                };
            }
            _ => return false,
        }
    }
}

/// The paths a path or `use` tree starting at `at` names, and the index after it: in a `use`
/// item, a `{..}` group of `use` trees is read here with the prefix (and its `super`s) it
/// extends, never on its own; any other group after `name::` is left to the scan.
fn read_path(
    tokens: &[TokenTree],
    mut at: usize,
    mut path: Building,
    tree: Tree,
) -> (Vec<Written>, usize) {
    at += usize::from(dollar_crate(tokens, at));
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
                if group.delimiter() == Delimiter::Brace
                    && match tree {
                        Tree::No => false,
                        Tree::Use => punct(tokens.get(at + 1), ';').is_some(),
                        Tree::Part => true,
                    }
                    && use_group(group, &path) =>
            {
                let mut out = Vec::new();
                for part in split_commas(group.stream()) {
                    let part: Vec<TokenTree> = part.into_iter().collect();
                    out.extend(read_path(&part, 0, path.clone(), Tree::Part).0);
                }
                return (out, at + 1);
            }
            _ => return (path.done().into_iter().collect(), at),
        }
    }
}

/// Every path `tokens` name, in `use` trees, signatures, bodies and macro definitions alike.
pub fn scan(tokens: &[TokenTree], out: &mut Vec<Named>) {
    scan_in(tokens, out, false);
}

/// [`scan`], in a macro call when `call`: what the macro makes of a `use` item is unknown, so
/// the trees of its `{..}` group are also read on their own.
fn scan_in(tokens: &[TokenTree], out: &mut Vec<Named>, call: bool) {
    let mut at = 0;
    while at < tokens.len() {
        let tree = if after_use(tokens, at) {
            Tree::Use
        } else {
            Tree::No
        };
        let (start, read) = match &tokens[at] {
            TokenTree::Group(group) => {
                // `use {..};`, a group of whole `use` trees.
                let (paths, end) = match tree {
                    Tree::Use => read_path(tokens, at, Building::default(), tree),
                    _ => (Vec::new(), at),
                };
                if end > at {
                    (Some(group.span_open().start()), Some((paths, end, false)))
                } else {
                    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                    let call = call || matches!(macro_body(tokens, at), Some(Macro::Call(_)));
                    scan_in(&inner, out, call);
                    (None, None)
                }
            }
            TokenTree::Ident(word) if word == "pub" => {
                let restricted = match tokens.get(at + 1) {
                    Some(TokenTree::Group(group)) => restriction(group),
                    _ => None,
                };
                let start = word.span().start();
                (Some(start), restricted.map(|paths| (paths, at + 2, true)))
            }
            TokenTree::Ident(word) if path_start(tokens, at) => {
                let (paths, end) = read_path(tokens, at, Building::default(), tree);
                (Some(word.span().start()), Some((paths, end, false)))
            }
            TokenTree::Punct(colon) if extern_start(tokens, at) => {
                let (paths, end) = read_path(tokens, at + 2, Building::extern_prelude(), tree);
                (Some(colon.span().start()), Some((paths, end, false)))
            }
            _ => (None, None),
        };
        match (start, read) {
            (Some(start), Some((paths, end, restriction))) => {
                out.extend(paths.into_iter().map(|path| Named {
                    at: start,
                    path,
                    restriction,
                }));
                let groups = tokens[at..end].iter().filter_map(|token| match token {
                    TokenTree::Group(group) if call && tree == Tree::Use => Some(group),
                    _ => None,
                });
                for group in groups {
                    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                    scan_in(&inner, out, call);
                }
                at = end;
            }
            _ => at += 1,
        }
    }
}

/// Where `tokens` hold a `mod` keyword, and where an `include!` (`r#include!` too).
pub fn sites(tokens: &[TokenTree], mods: &mut Vec<LineColumn>, includes: &mut Vec<LineColumn>) {
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => {
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                sites(&inner, mods, includes);
            }
            TokenTree::Ident(word) if word == "mod" => mods.push(word.span().start()),
            TokenTree::Ident(word)
                if unraw(word) == "include" && punct(tokens.get(index + 1), '!').is_some() =>
            {
                includes.push(word.span().start());
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// `use` items in macro bodies and calls

/// A `use` item in the tokens.
pub struct FoundUse {
    /// Where its `use` keyword is.
    pub at: LineColumn,
    pub item: syn::ItemUse,
    /// The group it is in: where it opens and where it closes.
    pub group: (LineColumn, LineColumn),
    /// Where the outermost macro call it is an argument of starts; `None` outside any call and
    /// in a `macro_rules!` definition (which expands wherever it is called).
    pub call: Option<LineColumn>,
    /// An attribute before it may leave it out (a `cfg`, a `cfg_attr` or a macro variable), or it
    /// is in a `macro_rules!` definition.
    pub conditional: bool,
}

/// Where a walk is: the group it is in, the outermost macro call around it, and whether it is in
/// a `macro_rules!` definition.
#[derive(Clone, Copy)]
struct Within {
    group: Option<(LineColumn, LineColumn)>,
    call: Option<LineColumn>,
    defining: bool,
}

/// Every `use` item in a group of `tokens` that `skip` does not reject, and every name an
/// `extern crate self as <name>` binds.
pub fn found_uses(
    tokens: &[TokenTree],
    skip: &dyn Fn(LineColumn) -> bool,
    uses: &mut Vec<FoundUse>,
    aliases: &mut BTreeSet<String>,
) {
    let within = Within {
        group: None,
        call: None,
        defining: false,
    };
    walk_uses(tokens, within, skip, uses, aliases);
}

fn walk_uses(
    tokens: &[TokenTree],
    within: Within,
    skip: &dyn Fn(LineColumn) -> bool,
    uses: &mut Vec<FoundUse>,
    aliases: &mut BTreeSet<String>,
) {
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => {
                let (call, defining) = match macro_body(tokens, index) {
                    Some(Macro::Definition) => (None, true),
                    Some(Macro::Call(at)) if !within.defining => (within.call.or(Some(at)), false),
                    _ => (within.call, within.defining),
                };
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                let group = Some((group.span_open().start(), group.span_close().end()));
                let within = Within {
                    group,
                    call,
                    defining,
                };
                walk_uses(&inner, within, skip, uses, aliases);
            }
            TokenTree::Ident(word) if word == "use" => {
                let at = word.span().start();
                let item = use_item(tokens, index);
                if let (Some(group), Some((item, conditional)), false) =
                    (within.group, item, skip(at))
                {
                    uses.push(FoundUse {
                        at,
                        item,
                        group,
                        call: within.call,
                        conditional: within.defining || conditional,
                    });
                }
            }
            TokenTree::Ident(word) if word == "extern" && !skip(word.span().start()) => {
                if let [TokenTree::Ident(krate), TokenTree::Ident(this), TokenTree::Ident(r#as), TokenTree::Ident(name), ..] =
                    &tokens[index + 1..]
                {
                    if krate == "crate" && this == "self" && r#as == "as" {
                        aliases.insert(unraw(name));
                    }
                }
            }
            _ => {}
        }
    }
}

enum Macro {
    /// `macro_rules! name { .. }`.
    Definition,
    /// `name!(..)`, starting at `name`.
    Call(LineColumn),
}

/// Whether the group at `index` is the body of a macro definition or call.
fn macro_body(tokens: &[TokenTree], index: usize) -> Option<Macro> {
    let before = |back: usize| index.checked_sub(back).and_then(|i| tokens.get(i));
    match (before(3), before(2), before(1)) {
        (Some(TokenTree::Ident(rules)), Some(bang), Some(TokenTree::Ident(_)))
            if rules == "macro_rules" && punct(Some(bang), '!').is_some() =>
        {
            Some(Macro::Definition)
        }
        (_, Some(TokenTree::Ident(name)), Some(bang))
            if punct(Some(bang), '!').is_some() && !is_keyword(name) =>
        {
            Some(Macro::Call(name.span().start()))
        }
        _ => None,
    }
}

/// The `use` item whose `use` keyword is at `index` (with a `pub` or `pub(..)` before it), and
/// whether an outer attribute before it may leave it out (a `cfg`, a `cfg_attr`, a macro
/// variable); `None` when the tokens to its `;` are no `use` item (a macro variable in its
/// path). `$crate` reads as `crate`.
fn use_item(tokens: &[TokenTree], index: usize) -> Option<(syn::ItemUse, bool)> {
    let before = |back: usize| index.checked_sub(back).and_then(|i| tokens.get(i));
    let start = match (before(2), before(1)) {
        (Some(TokenTree::Ident(public)), Some(TokenTree::Group(scope)))
            if public == "pub" && scope.delimiter() == Delimiter::Parenthesis =>
        {
            index - 2
        }
        (_, Some(TokenTree::Ident(public))) if public == "pub" => index - 1,
        _ => index,
    };
    let (mut attr, mut conditional) = (start, false);
    while let (Some(_), Some(TokenTree::Group(group))) = (
        punct(attr.checked_sub(2).and_then(|i| tokens.get(i)), '#'),
        attr.checked_sub(1).and_then(|i| tokens.get(i)),
    ) {
        if group.delimiter() != Delimiter::Bracket {
            break;
        }
        conditional |= match group.stream().into_iter().next() {
            Some(TokenTree::Ident(word)) => ["cfg", "cfg_attr"].contains(&unraw(&word).as_str()),
            Some(TokenTree::Punct(dollar)) => dollar.as_char() == '$',
            _ => false,
        };
        attr -= 2;
    }
    let end = index
        + tokens[index..]
            .iter()
            .position(|token| punct(Some(token), ';').is_some())?;
    let item = syn::parse2(without_dollar(&tokens[start..=end])).ok()?;
    Some((item, conditional))
}

/// `tokens` with the `$` of each `$crate` left out, in groups too.
fn without_dollar(tokens: &[TokenTree]) -> TokenStream {
    let kept = tokens
        .iter()
        .enumerate()
        .filter(|(i, _)| !dollar_crate(tokens, *i));
    kept.map(|(_, token)| match token {
        TokenTree::Group(group) => {
            let inner: Vec<TokenTree> = group.stream().into_iter().collect();
            let mut stripped = Group::new(group.delimiter(), without_dollar(&inner));
            stripped.set_span(group.span());
            TokenTree::Group(stripped)
        }
        other => other.clone(),
    })
    .collect()
}
