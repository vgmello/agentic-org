//! Dynamic `<TAB>` candidates for `COMPLETE=<shell> agentic-org`: instance names, organization
//! IDs and slugs, office IDs and project keys, read from the completion cache the commands
//! fill when they list them. Completion never connects to a host: it reads one small file.
//! A leaf module beside `host`; the grammar (`args`, the noun slices) attaches the
//! completers to their flags.

use std::ffi::OsStr;

use clap_complete::engine::CompletionCandidate;
use org_cli_shell::cache::{scan_flag, CachedCandidate, CompletionCache};
use org_cli_shell::config::{INSTANCE_FLAG, INSTANCE_FLAG_ALIAS};

use crate::host;

/// The cache kinds.
pub(crate) const INSTANCES: &str = "instances";
pub(crate) const ORGS: &str = "orgs";
pub(crate) const OFFICES: &str = "offices";
pub(crate) const PROJECTS: &str = "projects";

/// The partition of an instance: its name, `default` when none is typed.
pub(crate) fn partition(instance: Option<&str>) -> String {
    instance.unwrap_or("default").to_owned()
}

fn cache() -> Option<CompletionCache> {
    host::cli_dirs().map(|dirs| CompletionCache::new(dirs.cache))
}

/// The instance typed on the command line being completed (`--name X`, `--name=X`,
/// `--instance X`), if any. A completion request is `agentic-org -- <the words typed>`: the
/// words after that first `--` are the ones read.
fn typed_instance() -> Option<String> {
    let args: Vec<_> = std::env::args_os().collect();
    let typed = match args.iter().position(|arg| arg == "--") {
        Some(at) => &args[at + 1..],
        None => &args[..],
    };
    scan_flag(typed, &[INSTANCE_FLAG, INSTANCE_FLAG_ALIAS])
}

fn candidates(stored: Vec<CachedCandidate>, current: &OsStr) -> Vec<CompletionCandidate> {
    let current = current.to_string_lossy();
    stored
        .into_iter()
        .filter(|candidate| candidate.name.starts_with(current.as_ref()))
        .map(|candidate| {
            CompletionCandidate::new(candidate.name).help(candidate.help.map(Into::into))
        })
        .collect()
}

fn of_instance(kind: &str, current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(cache) = cache() else {
        return Vec::new();
    };
    let partition = partition(typed_instance().as_deref());
    candidates(cache.load(&[&partition], kind), current)
}

pub(crate) fn instances(current: &OsStr) -> Vec<CompletionCandidate> {
    cache().map_or_else(Vec::new, |cache| {
        candidates(cache.load(&[], INSTANCES), current)
    })
}

pub(crate) fn orgs(current: &OsStr) -> Vec<CompletionCandidate> {
    of_instance(ORGS, current)
}

pub(crate) fn offices(current: &OsStr) -> Vec<CompletionCandidate> {
    of_instance(OFFICES, current)
}

pub(crate) fn projects(current: &OsStr) -> Vec<CompletionCandidate> {
    of_instance(PROJECTS, current)
}

/// Remembers candidates a command just listed, best effort: a cache that cannot be written
/// never fails the command, and a dry run writes nothing.
pub(crate) fn remember(instance: Option<&str>, kind: &str, found: Vec<CachedCandidate>) {
    let Some(cache) = cache() else {
        return;
    };
    let partition = partition(instance);
    let _ = cache.store(&[&partition], kind, &found);
    let mut known = cache.load(&[], INSTANCES);
    if !known.iter().any(|candidate| candidate.name == partition) {
        known.push(CachedCandidate {
            name: partition,
            help: Some("instance".to_owned()),
        });
        let _ = cache.store(&[], INSTANCES, &known);
    }
}
