//! The engine boundary (ADR-0018 ENG-1) for the office:
//!
//! - `rusqlite` is a dependency of `org-office` and `org-host` alone;
//! - inside `crates/org-office`, only `src/store/sqlite/` names the engine's crate or
//!   holds SQL. Every other Rust file of the crate (library, tests, benches, examples,
//!   build script) is parsed with `syn`, and a path through `rusqlite`, a string literal
//!   that reads as an SQL statement, or a `.sql` file is a violation. The allowlist is
//!   empty: there is nothing outside the adapter to excuse.
//!
//! The detector's own cases are in `tests/engine_boundary/*.rs.txt`, which this scan
//! does not read (they are not `.rs` files), so the guard never has to excuse itself.

use std::{
    fs,
    path::{Path, PathBuf},
};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

const ENGINE_CRATE: &str = "rusqlite";

/// Folders that hold no source of this workspace.
const SKIPPED: [&str; 4] = ["target", "node_modules", ".git", ".agent-kit"];

/// The only folder of `crates/org-office` that may name the engine or hold SQL.
const ENGINE_ADAPTER: &str = "src/store/sqlite";

/// Files outside the adapter excused from the scan. It stays empty (ENG-1).
const ALLOWED: [&str; 0] = [];

fn workspace() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.parent().and_then(Path::parent);
    root.expect("crates/org-office is two folders below the workspace")
        .to_owned()
}

/// Every file under `root` whose name satisfies `wanted`, sorted.
fn files(root: &Path, wanted: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if root.is_file() {
        return vec![root.to_owned()];
    }
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).expect("a readable folder") {
            let path = entry.expect("an entry").path();
            let file = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if !SKIPPED.contains(&file) {
                    folders.push(path);
                }
                continue;
            }
            if wanted(file) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Whether a manifest line names the crate as a dependency: `rusqlite = ...`,
/// `rusqlite.workspace = true` or a `[dependencies.rusqlite]` table.
fn names_engine(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with('#') {
        return false;
    }
    let key = line.split(['=', '.', ' ']).next().unwrap_or("");
    let table = line.starts_with('[') && line.trim_end_matches(']').ends_with(".rusqlite");
    key == ENGINE_CRATE || table
}

/// The kinds of SQL statement the scan recognizes.
///
/// The guard's own keywords are written in lower case, one word per literal, so the guard
/// passes its own scan without an exception.
#[derive(Debug, Clone, Copy)]
enum Shape {
    SqlFile,
    Transaction,
    PragmaOrSavepoint,
    InsertInto,
    ReplaceInto,
    DeleteFrom,
    UpdateSet,
    CreateObject,
    DropObject,
    AlterTable,
    AttachDatabase,
    BeginTransaction,
    SelectFrom,
    CapitalStatement,
}

const OBJECTS: [&str; 4] = ["table", "index", "view", "trigger"];

/// The SQL statement shape `text` matches, if any.
///
/// The words are compared without case. A statement is recognized by the keywords that
/// open it (`INSERT INTO`, `DELETE FROM`, `UPDATE <table> SET`, `CREATE TABLE`, ...), so
/// prose such as "delete the folder" or "update the record" does not count. `SELECT`
/// needs a later `FROM` and is either written in capitals, or opens the text and is
/// followed by a clause (`WHERE`, `ORDER`, `GROUP`, `LIMIT`, `JOIN`) or a `*` or `?`:
/// "select a team from the list" is prose (a lower-case `select` may also follow an
/// opening `with` or `explain`). `SELECT` in capitals counts anywhere, with or without
/// `FROM`, and so does a statement opened in capitals by `WITH`, `EXPLAIN`, `RELEASE`,
/// `ROLLBACK`, `ANALYZE` or `REINDEX`. A one-word statement (`COMMIT`, `BEGIN`, ...)
/// counts only in capitals. A lower-case `select a, b from t` with no clause, `*` or `?`
/// is not recognized: it reads like prose. A literal naming a `.sql` file counts, so
/// `include_str!("schema.sql")` outside the adapter is caught too.
fn sql_shape(text: &str) -> Option<Shape> {
    let trimmed = text.trim();
    let extension = Path::new(trimmed).extension().and_then(|it| it.to_str());
    if extension.is_some_and(|it| it.eq_ignore_ascii_case("sql")) {
        return Some(Shape::SqlFile);
    }
    let raw: Vec<&str> = trimmed
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .collect();
    let words: Vec<String> = raw.iter().map(|word| word.to_ascii_lowercase()).collect();
    let at = |index: usize| words.get(index).map(String::as_str).unwrap_or("");
    let capitals = |index: usize| raw[index].chars().all(|c| !c.is_ascii_lowercase());
    let statement = trimmed.trim_end_matches(';').trim();
    if raw.len() == 1
        && capitals(0)
        && statement.len() == raw[0].len()
        && matches!(
            at(0),
            "begin" | "commit" | "rollback" | "end" | "vacuum" | "analyze" | "reindex"
        )
    {
        return Some(Shape::Transaction);
    }
    // A statement opened by a keyword in capitals: `SELECT 1`, `WITH t AS (...)`,
    // `RELEASE sp`, `ROLLBACK TO sp`, `EXPLAIN ...`, `ANALYZE t`, `REINDEX t`.
    if raw.len() >= 2
        && capitals(0)
        && matches!(
            at(0),
            "select" | "with" | "explain" | "analyze" | "reindex" | "release" | "rollback"
        )
    {
        return Some(Shape::CapitalStatement);
    }
    if raw.len() >= 2 && matches!(at(0), "pragma" | "savepoint") {
        return Some(Shape::PragmaOrSavepoint);
    }
    for index in 0..words.len() {
        let next = at(index + 1);
        let shape = match at(index) {
            "insert" if next == "into" => Some(Shape::InsertInto),
            "insert" if next == "or" && at(index + 3) == "into" => Some(Shape::InsertInto),
            "replace" if next == "into" => Some(Shape::ReplaceInto),
            "delete" if next == "from" => Some(Shape::DeleteFrom),
            "update" if !next.is_empty() && at(index + 2) == "set" => Some(Shape::UpdateSet),
            "update" if next == "or" && !at(index + 3).is_empty() && at(index + 4) == "set" => {
                Some(Shape::UpdateSet)
            }
            "create"
                if OBJECTS.contains(&next)
                    || (matches!(next, "unique" | "temp" | "temporary" | "virtual")
                        && OBJECTS.contains(&at(index + 2))) =>
            {
                Some(Shape::CreateObject)
            }
            "drop" if OBJECTS.contains(&next) => Some(Shape::DropObject),
            "alter" if next == "table" => Some(Shape::AlterTable),
            "attach" | "detach" if next == "database" => Some(Shape::AttachDatabase),
            "begin" if matches!(next, "immediate" | "exclusive" | "deferred" | "transaction") => {
                Some(Shape::BeginTransaction)
            }
            "select" if capitals(index) => Some(Shape::SelectFrom),
            "select" if words[index + 1..].iter().any(|word| word == "from") => {
                let clause = words[index + 1..].iter().any(|word| {
                    matches!(
                        word.as_str(),
                        "where" | "order" | "group" | "limit" | "join"
                    )
                }) || trimmed.contains(['*', '?']);
                let opens = index == 0 || matches!(at(0), "with" | "explain");
                (opens && clause).then_some(Shape::SelectFrom)
            }
            _ => None,
        };
        if shape.is_some() {
            return shape;
        }
    }
    None
}

/// One source's violations of the boundary.
#[derive(Default)]
struct Scan {
    violations: Vec<String>,
}

impl Scan {
    fn literal(&mut self, text: &str) {
        if let Some(shape) = sql_shape(text) {
            let shown: String = text.trim().chars().take(60).collect();
            let shown = shown.split_whitespace().collect::<Vec<_>>().join(" ");
            self.violations
                .push(format!("SQL string literal ({shape:?}): {shown:?}"));
        }
    }

    fn engine(&mut self, how: &str) {
        self.violations
            .push(format!("the engine's crate named: {how}"));
    }

    /// A macro's tokens are opaque to `syn`'s visitor: read them as tokens, so that
    /// `format!("SELECT ...")` or `m!(rusqlite::Connection)` are seen too.
    fn tokens(&mut self, tokens: TokenStream) {
        for token in tokens {
            match token {
                TokenTree::Group(group) => self.tokens(group.stream()),
                TokenTree::Ident(ident) if ident == ENGINE_CRATE => {
                    self.engine("in a macro's tokens")
                }
                TokenTree::Literal(literal) => match syn::Lit::new(literal) {
                    syn::Lit::Str(text) => self.literal(&text.value()),
                    syn::Lit::ByteStr(bytes) => {
                        self.literal(&String::from_utf8_lossy(&bytes.value()))
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scan {
    // Doc comments are prose about the code, not literals in it.
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if attribute.path().is_ident("doc") {
            return;
        }
        // An attribute's argument list is tokens to `syn`, like a macro's.
        if let syn::Meta::List(list) = &attribute.meta {
            self.tokens(list.tokens.clone());
        }
        syn::visit::visit_attribute(self, attribute);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path
            .segments
            .iter()
            .any(|segment| segment.ident == ENGINE_CRATE)
        {
            self.engine("in a path");
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_use_path(&mut self, tree: &'ast syn::UsePath) {
        if tree.ident == ENGINE_CRATE {
            self.engine("in a `use`");
        }
        syn::visit::visit_use_path(self, tree);
    }

    fn visit_use_name(&mut self, tree: &'ast syn::UseName) {
        if tree.ident == ENGINE_CRATE {
            self.engine("in a `use`");
        }
    }

    fn visit_use_rename(&mut self, tree: &'ast syn::UseRename) {
        if tree.ident == ENGINE_CRATE {
            self.engine("in a `use`");
        }
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        if item.ident == ENGINE_CRATE {
            self.engine("in an `extern crate`");
        }
        syn::visit::visit_item_extern_crate(self, item);
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.literal(&literal.value());
    }

    fn visit_lit_byte_str(&mut self, literal: &'ast syn::LitByteStr) {
        self.literal(&String::from_utf8_lossy(&literal.value()));
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        syn::visit::visit_macro(self, mac);
        self.tokens(mac.tokens.clone());
    }
}

/// The violations in one Rust source text; `name` says where it came from.
fn violations(source: &str, name: &str) -> Vec<String> {
    let file = syn::parse_file(source).unwrap_or_else(|error| panic!("{name}: {error}"));
    let mut scan = Scan::default();
    scan.visit_file(&file);
    scan.violations
}

/// Every violation in `crates/org-office` outside the adapter, as `path: violation`, and
/// the number of Rust files read.
fn crate_violations() -> (Vec<String>, usize) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let adapter = manifest.join(ENGINE_ADAPTER);
    let mut found = Vec::new();
    let mut scanned = 0;
    for file in files(manifest, |_| true) {
        if file.starts_with(&adapter) {
            continue;
        }
        let relative = file.strip_prefix(manifest).expect("under the crate");
        let relative = relative.to_str().expect("a UTF-8 path").replace('\\', "/");
        if ALLOWED.contains(&relative.as_str()) {
            continue;
        }
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.to_ascii_lowercase().ends_with(".sql") {
            found.push(format!("{relative}: an SQL file outside the adapter"));
            continue;
        }
        if !name.ends_with(".rs") {
            continue;
        }
        scanned += 1;
        let text = fs::read_to_string(&file).expect("a source file");
        for violation in violations(&text, &relative) {
            found.push(format!("{relative}: {violation}"));
        }
    }
    (found, scanned)
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/engine_boundary")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn only_the_office_and_the_host_depend_on_the_engines_crate() {
    let root = workspace();
    let mut dependents = Vec::new();
    let manifests = files(&root, |name| name == "Cargo.toml");
    assert!(
        manifests.len() > 2,
        "the workspace's manifests were not found"
    );
    for manifest in manifests {
        // The workspace's own manifest only fixes the version its members use.
        if manifest.parent() == Some(root.as_path()) {
            continue;
        }
        let text = fs::read_to_string(&manifest).expect("a manifest");
        if text.lines().any(names_engine) {
            let folder = manifest.parent().expect("a manifest is in a folder");
            let folder = folder.strip_prefix(&root).expect("under the workspace");
            dependents.push(folder.to_str().expect("a UTF-8 path").replace('\\', "/"));
        }
    }
    dependents.sort();
    assert_eq!(dependents, ["crates/org-host", "crates/org-office"]);
}

#[test]
fn only_the_sqlite_adapter_names_the_engine_or_holds_sql() {
    assert!(
        ALLOWED.is_empty(),
        "ENG-1 allows no file outside the adapter"
    );
    let (found, scanned) = crate_violations();
    // The crate has about sixty Rust files outside the adapter; a scan that read only a
    // handful lost its way.
    assert!(scanned > 40, "only {scanned} files were scanned");
    assert!(
        found.is_empty(),
        "outside {ENGINE_ADAPTER}, the engine is named or SQL is written: {found:#?}"
    );
}

#[test]
fn the_scan_finds_the_engine_and_its_sql_in_the_adapter() {
    // The control: the same scan over the adapter itself must see what it guards against.
    let adapter = Path::new(env!("CARGO_MANIFEST_DIR")).join(ENGINE_ADAPTER);
    let mut named = 0;
    let mut statements = 0;
    for file in files(&adapter, |name| name.ends_with(".rs")) {
        let text = fs::read_to_string(&file).expect("a source file");
        for violation in violations(&text, &file.display().to_string()) {
            if violation.starts_with("SQL string literal") {
                statements += 1;
            } else {
                named += 1;
            }
        }
    }
    assert!(named > 0, "no use of the engine was seen in the adapter");
    assert!(statements > 0, "no SQL was seen in the adapter");
}

#[test]
fn the_scan_reports_each_kind_of_violation() {
    let source = fixture("violations.rs.txt");
    let file = syn::parse_file(&source).expect("the fixture parses");
    let mut checked = 0;
    for item in file.items {
        let syn::Item::Fn(function) = item else {
            continue;
        };
        let mut scan = Scan::default();
        scan.visit_item_fn(&function);
        assert!(
            !scan.violations.is_empty(),
            "the fixture's `{}` was not reported",
            function.sig.ident
        );
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} fixture cases were read");
}

#[test]
fn the_scan_lets_prose_and_ordinary_code_through() {
    let found = violations(&fixture("clean.rs.txt"), "clean.rs.txt");
    assert!(
        found.is_empty(),
        "prose was taken for a violation: {found:#?}"
    );
}

#[test]
fn the_guard_recognizes_a_dependency_line() {
    for line in [
        "rusqlite.workspace = true",
        "rusqlite = \"0.42\"",
        "rusqlite = { version = \"=0.42.0\" }",
        "[dependencies.rusqlite]",
        "[target.'cfg(unix)'.dependencies.rusqlite]",
    ] {
        assert!(names_engine(line), "{line}");
    }
    for line in [
        "# rusqlite = \"1\"",
        "description = \"no rusqlite here\"",
        "rusqlite-extra = \"1\"",
    ] {
        assert!(!names_engine(line), "{line}");
    }
}
