//! The set cache key (processes.md §3.3): sets are cached under a fingerprint of the handbook
//! commit, the project's integration commit and the team chains, so a change to any of those
//! drops the sets that used it. Place names ([`crate::Inputs::names`], for "Turned off for
//! <name>") are not in it: they are org data, not files, and a rename must drop the cached sets
//! that name the place.

use crate::place::{ProjectKey, TeamId};
use crate::resolve::Work;

/// A set's cache key, as plain data in a canonical form, for an in-memory cache. Persisting it
/// needs a canonical encoding of these fields, never a `std::hash::Hash` digest (which isn't
/// stable across builds). Its parts are
/// kept as separate fields, never joined into one text, so two different inputs can't run
/// together into one key (`"ab"` and `"c"` against `"a"` and `"bc"`). It names no agent: a
/// cache keyed by it alone holds one agent's sets. Built only by [`fingerprint`] and
/// [`fingerprint_without_work`], which put the chains in their canonical order:
///
/// ```compile_fail,E0451
/// use org_process::Fingerprint;
/// let made = Fingerprint { handbook: String::new(), work: None };
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint {
    handbook: String,
    /// None with no current work (O ⊕ G), which no project's set shares.
    work: Option<WorkPrint>,
}

/// The project's part of a [`Fingerprint`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct WorkPrint {
    project: ProjectKey,
    /// None when the project has no repository or its integration commit can't be found.
    integration: Option<String>,
    /// Every chain as given, each kept in its own order (outermost team first), sorted.
    chains: Vec<Vec<TeamId>>,
}

/// The cache key of a set for `work`, from the handbook commit, the project's integration
/// commit (none: no repository, or a missing integration branch) and the work's project and team
/// chains. Equal inputs give equal keys whatever order the chains come in; a change to either
/// commit, the project, any chain or the order of the teams inside one gives another. Chains
/// that resolving drops (a copy of another, or a prefix of one) still count, so adding or
/// removing one changes the key: a needless recompute, never a stale set. Commits are kept as
/// given, unchecked and untrimmed.
pub fn fingerprint(
    handbook_commit: &str,
    integration_commit: Option<&str>,
    work: &Work,
) -> Fingerprint {
    let mut chains = work.chains().to_vec();
    chains.sort();
    Fingerprint {
        handbook: handbook_commit.to_owned(),
        work: Some(WorkPrint {
            project: work.project().clone(),
            integration: integration_commit.map(str::to_owned),
            chains,
        }),
    }
}

/// The cache key of the set with no current work (O ⊕ G, PROC-2), from the handbook commit only.
/// It differs from every [`fingerprint`] of a work, whatever the commits.
pub fn fingerprint_without_work(handbook_commit: &str) -> Fingerprint {
    Fingerprint {
        handbook: handbook_commit.to_owned(),
        work: None,
    }
}

#[cfg(test)]
mod tests;
