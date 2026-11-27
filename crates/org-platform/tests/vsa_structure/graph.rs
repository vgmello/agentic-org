//! A crate target: its module tree, what each module binds, and every crate-internal path its
//! production code names, resolved through `use` bindings to the module it reaches.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use proc_macro2::{LineColumn, TokenStream, TokenTree};

use super::parse::{self, FileItems, Head, Named, Use, Written};

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
    /// The naming module's path.
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

/// What one module defines and binds.
#[derive(Debug, Default)]
pub struct Scope {
    pub items: BTreeSet<String>,
    /// Its loaded child modules.
    pub children: BTreeSet<String>,
    pub uses: BTreeMap<String, Binding>,
    pub globs: Vec<Binding>,
}

/// What the checks read of one crate target.
#[derive(Debug, Default)]
pub struct Target {
    pub root: PathBuf,
    pub modules: Vec<Module>,
    /// Every loaded module (file, inline or in a body) by path.
    pub scopes: BTreeMap<Vec<String>, Scope>,
    pub references: Vec<Reference>,
    /// `pub mod` items of the root file (inline or not).
    pub root_pub_mods: Vec<(String, LineColumn)>,
    /// Layout rules broken while reading: test file names, `#[path]`, `include!`.
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
    source: String,
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
        let items =
            FileItems::read(&syntax).map_err(|error| format!("{}{error}", file.display()))?;
        for child in &items.children {
            if let Some(next) = target.child(&file, &module, child, read)? {
                pending.push(next);
            }
        }
        target.add_file(&file, &module, &items);
        parsed.push(Parsed {
            file,
            module,
            source,
            items,
        });
    }
    for file in &parsed {
        target.scan(file)?;
    }
    for (module, scope) in &target.scopes {
        for binding in scope.uses.values().chain(&scope.globs) {
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
        child: &parse::ChildDecl,
        read: Read<'_>,
    ) -> Result<Option<(PathBuf, Vec<String>)>, String> {
        let mut base = child_dir(file, module.is_empty());
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
        path.extend(child.inline.iter().cloned());
        path.push(child.name.clone());
        Ok(Some((found, path)))
    }

    /// Records a file's modules and what each binds.
    fn add_file(&mut self, file: &Path, module: &[String], items: &FileItems) {
        let inline = items.inline.iter().map(|(inline, _, _)| {
            let mut path = module.to_vec();
            path.extend(inline.iter().cloned());
            path
        });
        for path in std::iter::once(module.to_vec()).chain(inline) {
            if let Some((name, parent)) = path.split_last() {
                let parent = self.scopes.entry(parent.to_vec()).or_default();
                parent.children.insert(name.clone());
            }
            self.scopes.entry(path.clone()).or_default();
            self.modules.push(Module {
                path,
                file: file.to_path_buf(),
            });
        }
        let binding = |found: &Use| Binding {
            path: found.path.clone(),
            public: found.public,
            file: file.to_path_buf(),
            at: found.at,
        };
        for (inline, names) in &items.names {
            let mut path = module.to_vec();
            path.extend(inline.iter().cloned());
            let scope = self.scopes.entry(path).or_default();
            scope.items.extend(names.items.iter().cloned());
            for (name, found) in &names.uses {
                scope.uses.insert(name.clone(), binding(found));
            }
            scope.globs.extend(names.globs.iter().map(binding));
        }
        if module.is_empty() {
            self.root_pub_mods.extend(items.pub_mods.iter().cloned());
        }
        self.aliases.extend(items.aliases.iter().cloned());
    }

    /// Records every crate-internal path a production file names.
    fn scan(&mut self, file: &Parsed) -> Result<(), String> {
        let tokens: TokenStream = file
            .source
            .parse()
            .map_err(|error| format!("{}: {error}", file.file.display()))?;
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        let items = &file.items;
        let (mut mods, mut includes) = (Vec::new(), Vec::new());
        parse::sites(&tokens, &mut mods, &mut includes);
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
        parse::scan(&tokens, &mut named);
        for Named {
            at,
            path,
            restriction,
        } in named
        {
            if items.in_test(at) {
                continue;
            }
            let mut from = file.module.clone();
            from.extend(items.module_at(at).iter().cloned());
            let found = self
                .absolute(&from, &path)
                .map_err(|error| format!("{}: {error}", site(&file.file, at)))?;
            let Some(absolute) = found else {
                continue;
            };
            // `pub(in path)` names an ancestor, which `rustc` checks; anything else is a path.
            if restriction && from.starts_with(&absolute) {
                continue;
            }
            let to = self
                .resolve(&absolute)
                .map_err(|error| format!("{}: {error}", site(&file.file, at)))?;
            self.references.push(Reference {
                from,
                file: file.file.clone(),
                at,
                to,
            });
        }
        Ok(())
    }

    /// The crate path `path` stands for when written in `module`; `None` when it leaves the
    /// crate (another crate, the prelude, a generic parameter).
    pub fn absolute(
        &self,
        module: &[String],
        path: &Written,
    ) -> Result<Option<Vec<String>>, String> {
        self.absolute_at(module, path, 0, true)
    }

    /// The path `to` reaches: each segment after a module is followed through that module's
    /// `use` bindings and globs (re-exports, inline facades); the first item ends it.
    pub fn resolve(&self, to: &[String]) -> Result<Vec<String>, String> {
        self.resolve_at(to, 0, true)
    }

    /// [`Self::absolute`] `depth` bindings deep; `globs` is whether globs are followed (one
    /// level: what a glob reaches never consults globs again).
    fn absolute_at(
        &self,
        module: &[String],
        path: &Written,
        depth: usize,
        globs: bool,
    ) -> Result<Option<Vec<String>>, String> {
        match path.head {
            Head::Root => Ok(Some(path.segments.clone())),
            Head::Up(supers) if supers > module.len() => Err(format!(
                "a path climbs {supers} modules from `{}`, past the crate root",
                display_path(module)
            )),
            Head::Up(supers) => {
                let mut to = module[..module.len() - supers].to_vec();
                to.extend(path.segments.iter().cloned());
                Ok(Some(to))
            }
            Head::Local => {
                let Some((first, rest)) = path.segments.split_first() else {
                    return Ok(None);
                };
                let found = self.lookup(module, first, depth, globs)?;
                Ok(found.map(|mut to| {
                    to.extend(rest.iter().cloned());
                    to
                }))
            }
        }
    }

    /// What a plain `name` stands for in `module`: its `use` binding, its own item or child
    /// module, the crate itself (`extern crate self as name`), or a name its globs import.
    fn lookup(
        &self,
        module: &[String],
        name: &str,
        depth: usize,
        globs: bool,
    ) -> Result<Option<Vec<String>>, String> {
        if depth > BINDING_DEPTH {
            return Ok(None);
        }
        let Some(scope) = self.scopes.get(module) else {
            return Ok(None);
        };
        // A `use` never resolves through itself (`use serde_json;` names the crate).
        let binding = scope.uses.get(name).filter(|binding| {
            binding.path.head != Head::Local
                || binding.path.segments.first().map(String::as_str) != Some(name)
        });
        if let Some(binding) = binding {
            return self.absolute_at(module, &binding.path, depth + 1, globs);
        }
        if scope.items.contains(name) || scope.children.contains(name) {
            let mut own = module.to_vec();
            own.push(name.to_owned());
            return Ok(Some(own));
        }
        if self.aliases.contains(name) {
            return Ok(Some(Vec::new()));
        }
        for glob in scope.globs.iter().filter(|_| globs) {
            let Some(source) = self.absolute_at(module, &glob.path, depth + 1, false)? else {
                continue;
            };
            let source = self.resolve_at(&source, depth + 1, false)?;
            if let Some(found) = self.lookup(&source, name, depth + 1, false)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    fn resolve_at(&self, to: &[String], depth: usize, globs: bool) -> Result<Vec<String>, String> {
        let mut path = to.to_vec();
        'expand: for _ in 0..BINDING_DEPTH {
            let mut module: Vec<String> = Vec::new();
            for (index, segment) in path.iter().enumerate() {
                let child = self.scopes.get(&module);
                if child.is_some_and(|scope| scope.children.contains(segment)) {
                    module.push(segment.clone());
                    continue;
                }
                let own = |found: &[String]| {
                    found.len() == module.len() + 1
                        && found.starts_with(&module)
                        && found.last() == Some(segment)
                };
                match self.lookup(&module, segment, depth + 1, globs)? {
                    Some(found) if !own(&found) => {
                        let mut expanded = found;
                        expanded.extend(path[index + 1..].iter().cloned());
                        path = expanded;
                        continue 'expand;
                    }
                    _ => return Ok(path),
                }
            }
            return Ok(path);
        }
        Ok(path)
    }

    /// Whether `path` is a loaded module.
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
            if to != from && tops.contains(to) {
                graph.entry(from.clone()).or_default().insert(to.clone());
            }
        }
        graph
    }
}

/// `crate::a::b` (`crate` for the root).
pub fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "crate".to_owned()
    } else {
        format!("crate::{}", path.join("::"))
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
