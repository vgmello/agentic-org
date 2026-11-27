//! A crate target: its module tree, what each module binds, and every crate-internal path its
//! production code names, resolved through `use` bindings to the module it reaches.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use proc_macro2::{LineColumn, TokenStream, TokenTree};

use super::items::{ChildDecl, FileItems, Use};
use super::parse::{self, is_marker, module_of, Head, Written};
use super::scan::{self, Named};

/// How deep a chain of `use` bindings and globs is followed (`rustc` refuses cyclic ones).
const BINDING_DEPTH: usize = 32;

/// One production module of a crate target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// From the crate root; empty for the root itself.
    pub path: Vec<String>,
    /// The file it is written in, relative to the repository.
    pub file: PathBuf,
}

/// A crate-internal path a module names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The naming module's path (a module declared in a body has its block's segment).
    pub from: Vec<String>,
    pub file: PathBuf,
    pub at: LineColumn,
    /// What it reaches, from the crate root, through every `use` on the way (`[]` is the root).
    pub to: Vec<String>,
}

/// A `use` of a module and the file it is written in.
#[derive(Debug, Clone)]
pub struct Binding {
    pub path: Written,
    pub public: bool,
    pub file: PathBuf,
    pub at: LineColumn,
}

/// What one module or block defines and binds.
#[derive(Debug, Default)]
pub struct Scope {
    pub items: BTreeSet<String>,
    /// Its loaded child modules (a block's are the modules declared in it).
    pub children: BTreeSet<String>,
    /// The children declared `pub`.
    pub public_children: BTreeSet<String>,
    /// Every path each name may stand for (`cfg` alternatives bind a name more than once).
    pub uses: BTreeMap<String, Vec<Binding>>,
    pub globs: Vec<Binding>,
}

/// What the checks read of one crate target.
#[derive(Debug, Default)]
pub struct Target {
    pub root: PathBuf,
    /// Every module of an item list (not one declared in a body).
    pub modules: Vec<Module>,
    /// Every loaded module (file, inline or in a body) and every block that binds names, by
    /// scope path.
    pub scopes: BTreeMap<Vec<String>, Scope>,
    pub references: Vec<Reference>,
    /// `pub mod` items of the root file (inline or not).
    pub root_pub_mods: Vec<(String, LineColumn)>,
    /// Layout rules broken while reading: test file names, `#[path]`, `include!`, a name bound
    /// twice in one scope.
    pub problems: Vec<String>,
    /// Names bound by `extern crate self as <name>`: the crate root.
    aliases: BTreeSet<String>,
}

/// The source of a file, by path relative to the repository.
pub type Read<'a> = &'a dyn Fn(&Path) -> Option<String>;

/// `file:line:column`.
pub fn site(file: &Path, at: LineColumn) -> String {
    format!("{}:{}:{}", file.display(), at.line, at.column + 1)
}

/// Whether `path` is dedicated test code (the rule of `file_size_ratchet`).
pub fn is_test_path(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        let stem = name.strip_suffix(".rs").unwrap_or(&name);
        matches!(stem, "tests" | "testing" | "testkit" | "test_support")
            || stem.ends_with("_tests")
            || stem.ends_with("-tests")
    })
}

/// `path` with `.` and `..` worked out lexically (a `..` above the start is kept).
fn normalize(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.last(), Some(Component::Normal(_))) => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.iter().collect()
}

/// The directory a file's `mod x;` children are in.
fn child_dir(file: &Path, root_like: bool) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("")).to_path_buf();
    if root_like || file.file_name().is_some_and(|n| n == "mod.rs") {
        dir
    } else {
        dir.join(file.file_stem().unwrap_or_default())
    }
}

/// A production file read in the first pass, scanned in the second.
struct Parsed {
    file: PathBuf,
    module: Vec<String>,
    tokens: Vec<TokenTree>,
    items: FileItems,
}

/// Reads one crate target from its root file.
pub fn load_target(root: &Path, read: Read<'_>) -> Result<Target, String> {
    let mut target = Target {
        root: root.to_path_buf(),
        ..Target::default()
    };
    let mut parsed = Vec::new();
    let mut pending: Vec<(PathBuf, Vec<String>)> = vec![(root.to_path_buf(), Vec::new())];
    while let Some((file, module)) = pending.pop() {
        let source = read(&file).ok_or_else(|| format!("{}: cannot be read", file.display()))?;
        let source = source
            .strip_prefix('\u{feff}')
            .unwrap_or(&source)
            .to_owned();
        let syntax =
            syn::parse_file(&source).map_err(|error| format!("{}: {error}", file.display()))?;
        if parse::test_only(&syntax.attrs) {
            continue;
        }
        let tokens: TokenStream = source
            .parse()
            .map_err(|error| format!("{}: {error}", file.display()))?;
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        let items = FileItems::read(&syntax, &tokens)
            .map_err(|error| format!("{}{error}", file.display()))?;
        for child in &items.children {
            if let Some(next) = target.child(&file, &module, child, read)? {
                pending.push(next);
            }
        }
        target.add_file(&file, &module, &items);
        parsed.push(Parsed {
            file,
            module,
            tokens,
            items,
        });
    }
    for file in &parsed {
        target.scan(file)?;
    }
    for (module, scope) in &target.scopes {
        for binding in scope.uses.values().flatten().chain(&scope.globs) {
            target
                .absolute(module, &binding.path)
                .map_err(|error| format!("{}: {error}", site(&binding.file, binding.at)))?;
        }
    }
    target.modules.sort_by(|a, b| a.path.cmp(&b.path));
    target.modules.dedup();
    target
        .references
        .sort_by(|a, b| (&a.file, a.at).cmp(&(&b.file, b.at)));
    Ok(target)
}

impl Target {
    /// Finds the file of `mod name;`, checking the layout rules on the way; `None` for a test
    /// module that is not production code.
    fn child(
        &mut self,
        file: &Path,
        module: &[String],
        child: &ChildDecl,
        read: Read<'_>,
    ) -> Result<Option<(PathBuf, Vec<String>)>, String> {
        // A block is no directory: a `mod x;` in a body needs a `#[path]` from the file's own.
        let inline: Vec<&String> = child.scope.iter().filter(|s| !is_marker(s)).collect();
        let mut base = child_dir(file, module.is_empty());
        base.extend(inline.iter());
        let candidates = match &child.path_attr {
            Some(p) if inline.is_empty() => {
                vec![file.parent().unwrap_or(Path::new("")).join(p)]
            }
            Some(p) => vec![base.join(p)],
            None => vec![
                base.join(format!("{}.rs", child.name)),
                base.join(&child.name).join("mod.rs"),
            ],
        };
        let Some(found) = candidates
            .iter()
            .map(|c| normalize(c))
            .find(|c| read(c).is_some())
        else {
            return Err(format!(
                "{}: module `{}` has no file",
                site(file, child.at),
                child.name
            ));
        };
        let src = self.root.parent().unwrap_or(Path::new(""));
        let test_named = is_test_path(found.strip_prefix(src).unwrap_or(&found));
        if test_named && child.test_gated {
            return Ok(None);
        }
        if test_named {
            self.problems.push(format!(
                "{}: `mod {}` loads the test-reserved file {} without a test `cfg`; gate it on `test` or a test-only feature, or rename it",
                site(file, child.at),
                child.name,
                found.display()
            ));
        }
        if child.path_attr.is_some() && !found.starts_with(&base) {
            self.problems.push(format!(
                "{}: `#[path]` loads {} from outside {}, the declaring module's own directory",
                site(file, child.at),
                found.display(),
                base.display()
            ));
        }
        let mut path = module.to_vec();
        path.extend(child.scope.iter().cloned());
        path.push(child.name.clone());
        Ok(Some((found, path)))
    }

    /// Records a file's modules and what each of its scopes binds.
    fn add_file(&mut self, file: &Path, module: &[String], items: &FileItems) {
        let key = |inner: &[String]| {
            let mut path = module.to_vec();
            path.extend(inner.iter().cloned());
            path
        };
        let inline = items.inline.iter().map(|m| (key(&m.key), m.public));
        for (path, public) in std::iter::once((module.to_vec(), false)).chain(inline) {
            if let Some((name, parent)) = path.split_last() {
                let parent = self.scopes.entry(parent.to_vec()).or_default();
                parent.children.insert(name.clone());
                if public {
                    parent.public_children.insert(name.clone());
                }
            }
            self.scopes.entry(path.clone()).or_default();
            // A module declared in a body is no module of the tree: nothing outside names it.
            if !path.iter().any(|segment| is_marker(segment)) {
                self.modules.push(Module {
                    path,
                    file: file.to_path_buf(),
                });
            }
        }
        for child in items.children.iter().filter(|child| child.public) {
            let parent = self.scopes.entry(key(&child.scope)).or_default();
            parent.public_children.insert(child.name.clone());
        }
        let binding = |found: &Use| Binding {
            path: found.path.clone(),
            public: found.public,
            file: file.to_path_buf(),
            at: found.at,
        };
        for (inner, names) in &items.names {
            let scope = self.scopes.entry(key(inner)).or_default();
            scope.items.extend(names.items.iter().cloned());
            for (name, found) in &names.uses {
                let bound = scope.uses.entry(name.clone()).or_default();
                bound.extend(found.iter().map(binding));
            }
            scope.globs.extend(names.globs.iter().map(binding));
        }
        for (at, problem) in &items.problems {
            self.problems
                .push(format!("{}: {problem}", site(file, *at)));
        }
        if module.is_empty() {
            self.root_pub_mods.extend(items.pub_mods.iter().cloned());
        }
        self.aliases.extend(items.aliases.iter().cloned());
    }

    /// Records every crate-internal path a production file names.
    fn scan(&mut self, file: &Parsed) -> Result<(), String> {
        let (tokens, items) = (&file.tokens, &file.items);
        let (mut mods, mut includes) = (Vec::new(), Vec::new());
        scan::sites(tokens, &mut mods, &mut includes);
        if let Some(at) = mods
            .iter()
            .find(|at| !items.placed(**at) && !items.in_test(**at))
        {
            return Err(format!(
                "{}: a `mod` the guard cannot place (inside a macro or an unusual expression); declare modules as items",
                site(&file.file, *at)
            ));
        }
        for at in includes.iter().filter(|at| !items.in_test(**at)) {
            self.problems.push(format!(
                "{}: `include!` pastes in code no module owns; declare a module instead",
                site(&file.file, *at)
            ));
        }
        let mut named = Vec::new();
        scan::scan(tokens, &mut named);
        for Named {
            at,
            path,
            restriction,
        } in named
        {
            if items.in_test(at) {
                continue;
            }
            let mut scope = file.module.clone();
            scope.extend(items.scope_at(at).iter().cloned());
            let error = |error: String| format!("{}: {error}", site(&file.file, at));
            let from = module_of(&scope).to_vec();
            for absolute in self.absolute(&scope, &path).map_err(error)? {
                // `pub(in path)` names an ancestor, which `rustc` checks; anything else is a
                // path.
                if restriction && from.starts_with(&absolute) {
                    continue;
                }
                for to in self.resolve(&absolute).map_err(error)? {
                    self.references.push(Reference {
                        from: from.clone(),
                        file: file.file.clone(),
                        at,
                        to,
                    });
                }
            }
        }
        Ok(())
    }

    /// Every crate path `path` may stand for when written in `scope` (a module or a block):
    /// one, or one per `cfg` alternative of a name on the way; none when it leaves the crate
    /// (another crate, the prelude, a generic parameter).
    pub fn absolute(&self, scope: &[String], path: &Written) -> Result<Vec<Vec<String>>, String> {
        self.absolute_at(scope, path, 0, true)
    }

    /// Every path `to` reaches: each segment after a module is followed through that module's
    /// `use` bindings and globs (re-exports, inline facades); the first item ends it.
    pub fn resolve(&self, to: &[String]) -> Result<Vec<Vec<String>>, String> {
        self.resolve_at(to, 0, true)
    }

    /// [`Self::absolute`] `depth` bindings deep; `globs` is whether globs are followed (one
    /// level: what a glob reaches never consults globs again).
    fn absolute_at(
        &self,
        scope: &[String],
        path: &Written,
        depth: usize,
        globs: bool,
    ) -> Result<Vec<Vec<String>>, String> {
        match path.head {
            Head::Root => Ok(vec![path.segments.clone()]),
            // `::name` is a crate of the extern prelude: this one only through an alias.
            Head::Extern => Ok(match path.segments.split_first() {
                Some((first, rest)) if self.aliases.contains(first) => vec![rest.to_vec()],
                _ => Vec::new(),
            }),
            Head::Up(supers) => {
                let mut to = climb(scope, supers).ok_or_else(|| {
                    format!(
                        "a path climbs {supers} modules from `{}`, past the crate root",
                        display_path(module_of(scope))
                    )
                })?;
                to.extend(path.segments.iter().cloned());
                Ok(vec![to])
            }
            Head::Local => {
                let Some((first, rest)) = path.segments.split_first() else {
                    return Ok(Vec::new());
                };
                let found = self.lookup(scope, first, depth, globs)?;
                Ok(extend(found, rest))
            }
        }
    }

    /// What a plain `name` may stand for in `scope`: what the scope binds, else what each block
    /// around it binds out to its module (a module never sees the blocks around it), else the
    /// crate itself (`extern crate self as name`).
    fn lookup(
        &self,
        scope: &[String],
        name: &str,
        depth: usize,
        globs: bool,
    ) -> Result<Vec<Vec<String>>, String> {
        if depth > BINDING_DEPTH {
            return Ok(Vec::new());
        }
        let mut key = scope.to_vec();
        loop {
            if let Some(found) = self.lookup_in(&key, name, depth, globs)? {
                return Ok(found);
            }
            match key.last() {
                Some(last) if is_marker(last) => key.pop(),
                _ => break,
            };
        }
        Ok(if self.aliases.contains(name) {
            vec![Vec::new()]
        } else {
            Vec::new()
        })
    }

    /// `name` in the one scope `key`: its `use` bindings and its own item or child module (a
    /// type and a value may share a name), else a name its globs import. `None` when the scope
    /// does not bind it; an empty list when it binds it only to something outside the crate.
    fn lookup_in(
        &self,
        key: &[String],
        name: &str,
        depth: usize,
        globs: bool,
    ) -> Result<Option<Vec<Vec<String>>>, String> {
        let Some(scope) = self.scopes.get(key) else {
            return Ok(None);
        };
        let around = key.split_last().filter(|(last, _)| is_marker(last));
        let (mut found, mut bound) = (Vec::new(), false);
        for binding in scope.uses.get(name).into_iter().flatten() {
            let path = &binding.path;
            let first = path.segments.first().map(String::as_str);
            if path.head != Head::Local || first != Some(name) {
                bound = true;
                found.extend(self.absolute_at(key, path, depth + 1, globs)?);
            } else if let Some((_, around)) = around {
                // `use x;` (or `use x::y as x;`) never resolves through itself: in a block it
                // names the `x` around the block; in a module, another crate.
                bound = true;
                let outer = self.lookup(around, name, depth + 1, globs)?;
                found.extend(extend(outer, &path.segments[1..]));
            }
        }
        if scope.items.contains(name) || scope.children.contains(name) {
            bound = true;
            found.push(key.iter().cloned().chain([name.to_owned()]).collect());
        }
        if bound {
            return Ok(Some(found));
        }
        for glob in scope.globs.iter().filter(|_| globs) {
            for source in self.absolute_at(key, &glob.path, depth + 1, false)? {
                for source in self.resolve_at(&source, depth + 1, false)? {
                    found.extend(
                        self.lookup_in(&source, name, depth + 1, false)?
                            .into_iter()
                            .flatten(),
                    );
                }
            }
        }
        Ok((!found.is_empty()).then_some(found))
    }

    fn resolve_at(
        &self,
        to: &[String],
        depth: usize,
        globs: bool,
    ) -> Result<Vec<Vec<String>>, String> {
        let mut done = Vec::new();
        let mut pending = vec![(to.to_vec(), 0)];
        while let Some((path, expansions)) = pending.pop() {
            let mut module: Vec<String> = Vec::new();
            let (mut ends, mut next) = (true, Vec::new());
            for (index, segment) in path.iter().enumerate() {
                let child = self.scopes.get(&module);
                // Only the modules of item lists are children; a block's segment is in a path
                // found through a name the block binds.
                let block = is_marker(segment) && {
                    let mut block = module.clone();
                    block.push(segment.clone());
                    self.scopes.contains_key(&block)
                };
                if block || child.is_some_and(|scope| scope.children.contains(segment)) {
                    module.push(segment.clone());
                    continue;
                }
                let own = |found: &Vec<String>| {
                    found.len() == module.len() + 1
                        && found.starts_with(&module)
                        && found.last() == Some(segment)
                };
                let found = if expansions < BINDING_DEPTH {
                    self.lookup(&module, segment, depth + 1, globs)?
                } else {
                    Vec::new()
                };
                let (own, others): (Vec<_>, Vec<_>) = found.into_iter().partition(own);
                ends = !own.is_empty() || others.is_empty();
                next = extend(others, &path[index + 1..]);
                break;
            }
            pending.extend(next.into_iter().map(|path| (path, expansions + 1)));
            if ends {
                done.push(path);
            }
        }
        done.sort();
        done.dedup();
        Ok(done)
    }

    /// Whether `path` is a loaded module (or block).
    pub fn is_module(&self, path: &[String]) -> bool {
        self.scopes.contains_key(path)
    }

    /// The top-level modules (children of the root).
    pub fn top_level(&self) -> BTreeSet<String> {
        self.modules
            .iter()
            .filter(|m| m.path.len() == 1)
            .map(|m| m.path[0].clone())
            .collect()
    }

    /// Top-level module dependencies, `from -> {to}`, of every module below the root.
    pub fn graph(&self) -> BTreeMap<String, BTreeSet<String>> {
        let tops = self.top_level();
        let mut graph: BTreeMap<String, BTreeSet<String>> =
            tops.iter().map(|t| (t.clone(), BTreeSet::new())).collect();
        for reference in &self.references {
            let (Some(from), Some(to)) = (reference.from.first(), reference.to.first()) else {
                continue;
            };
            if to != from && tops.contains(from) && tops.contains(to) {
                graph.entry(from.clone()).or_default().insert(to.clone());
            }
        }
        graph
    }
}

/// Each of `found` followed by `rest`.
fn extend(found: Vec<Vec<String>>, rest: &[String]) -> Vec<Vec<String>> {
    found
        .into_iter()
        .map(|mut path| {
            path.extend(rest.iter().cloned());
            path
        })
        .collect()
}

/// The module of `scope` and `supers` modules up: a block is no module, so `self` and `super`
/// in a block name the module it is in (and its parent).
fn climb(scope: &[String], supers: usize) -> Option<Vec<String>> {
    let mut module = module_of(scope).to_vec();
    for _ in 0..supers {
        module.pop()?;
        module.truncate(module_of(&module).len());
    }
    Some(module)
}

/// `crate::a::b` (`crate` for the root; a block's segment is left out).
pub fn display_path(path: &[String]) -> String {
    let named: Vec<&str> = path
        .iter()
        .map(String::as_str)
        .filter(|segment| !is_marker(segment))
        .collect();
    if named.is_empty() {
        "crate".to_owned()
    } else {
        format!("crate::{}", named.join("::"))
    }
}

/// Tarjan's strongly connected components with two or more members, each sorted.
pub fn cycles(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
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
