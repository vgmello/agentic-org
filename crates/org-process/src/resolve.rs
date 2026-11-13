//! Which processes apply to an agent's work (processes.md §3): [`resolve`] applies §3.2's rules
//! 1-9, in order, to plain data, and says what happened to every file (PROC-2 to PROC-9). This
//! slice applies rules 1 to 5 to the organization's and the teams' files; the project, private
//! files and drift come next.

use std::collections::{BTreeMap, BTreeSet};

use crate::file::{Problem, ProblemKind, ProcessFile, Status};
use crate::place::{Place, ProcessKey, TeamId};

mod chains;

/// One file at one of §2.2's places, as read: the valid file, or its problem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    /// Where it lives.
    pub place: Place,
    /// Its path in its repository, to name it.
    pub path: String,
    /// What [`crate::Location::read`] gave.
    pub read: Result<ProcessFile, Problem>,
}

/// The agent's current work: its project's teams (§3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Work {
    /// For each team linked to the project, its chain from the outermost parent down to it. A
    /// chain that is a prefix of another (a linked ancestor of another linked team) or a copy of
    /// one is dropped.
    pub chains: Vec<Vec<TeamId>>,
}

/// What the set is computed from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inputs {
    /// The current work; with none only the organization's files take part.
    pub work: Option<Work>,
    /// The files: O and the teams'. A file of a place that takes no part (for now, a project,
    /// an agent, or a team not linked to the work) is left out.
    pub files: Vec<Input>,
}

/// The effective set (§3.2 Output).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectiveSet {
    /// The places that took part, in resolution order: the organization, then the chains'
    /// teams outermost first.
    pub inputs: Vec<Place>,
    /// One per key a taking-part file has (drafts and retired files aside), by key.
    pub entries: Vec<Entry>,
    /// Team versions that disagree, which the project hasn't settled (rule 5).
    pub conflicts: Vec<Conflict>,
    /// Every file left out with something to fix: invalid, clashed or refused (sealed).
    pub problems: Vec<Layer>,
}

/// A key: the file that applies, if any, and every layer, as `process why` prints it (PROC-8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The key.
    pub key: ProcessKey,
    /// What applies; none when nothing does (refused, a conflict with no fallback).
    pub winner: Option<Winner>,
    /// Each file with the key, organization first; drafts and retired files aside.
    pub chain: Vec<Layer>,
}

/// The file that applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Winner {
    /// Its path.
    pub path: String,
    /// The file.
    pub file: ProcessFile,
    /// How it came to apply.
    pub kind: Kind,
}

/// How the winning file relates to the levels above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The organization's own version.
    Inherited,
    /// A team's or the project's version of a key that applied above it, or of a conflict.
    Override,
    /// A key nothing above it has.
    Addition,
}

/// One file's part in resolving its key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// Where it lives.
    pub place: Place,
    /// Its path.
    pub path: String,
    /// Its key, if it could be read.
    pub key: Option<ProcessKey>,
    /// Its version, if it is valid.
    pub version: Option<u32>,
    /// What happened to it.
    pub outcome: Outcome,
}

/// What happened to a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It applies.
    Applied,
    /// A nearer or lower version won.
    Overridden,
    /// A team version in a conflict that doesn't apply (rule 5).
    Conflict,
    /// "Details not recognized (<field>)": front matter, place or file name (rule 1).
    Invalid(Problem),
    /// Another file uses its key at its level and scope (rule 2).
    Clashed,
    /// It overrides or turns off a sealed key (rule 3).
    Refused {
        /// The sealing level.
        sealed_by: Place,
    },
}

/// Teams that disagree on a key, which the project hasn't settled (decision 4, PROC-6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The key.
    pub key: ProcessKey,
    /// The disagreeing versions' places (a team that sealed what it inherited stands for it).
    pub teams: Vec<Place>,
    /// The teams that sealed the key.
    pub sealed_by: Vec<Place>,
    /// What applies until the project decides: the organization's, a sealed team's, or nothing.
    pub applies: Option<Place>,
}

impl EffectiveSet {
    /// The entry for `key`, which `process why` prints.
    pub fn why(&self, key: &ProcessKey) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.key == *key)
    }
}

/// A taking-part file, with what the rules found.
struct Item<'a> {
    /// Its place's position in the resolution order.
    rank: usize,
    input: &'a Input,
    key: Option<ProcessKey>,
    /// The file, if it is valid.
    file: Option<&'a ProcessFile>,
    /// A valid draft or retired file: left out silently.
    quiet: bool,
    /// It seals its key, or can't be shown not to (rule 2).
    seals: bool,
    /// Set when a rule leaves it out.
    outcome: Option<Outcome>,
}

struct Resolver<'a> {
    chains: Vec<Vec<Place>>,
    items: Vec<Item<'a>>,
    /// What each override replaced: a version (or, for a settled conflict, none).
    parents: BTreeMap<usize, Option<usize>>,
}

/// The effective set for the work: §3.2's rules, in order.
pub fn resolve(inputs: &Inputs) -> EffectiveSet {
    let mut places = vec![Place::Organization];
    let mut chains = Vec::new();
    if let Some(work) = &inputs.work {
        chains = chains::kept(&work.chains);
        for place in chains.iter().flatten() {
            if !places.contains(place) {
                places.push(place.clone());
            }
        }
    }
    let mut r = Resolver {
        chains,
        items: read_files(inputs, &places),
        parents: BTreeMap::new(),
    };
    let keys: BTreeSet<ProcessKey> = r
        .items
        .iter()
        .filter(|i| !i.quiet)
        .filter_map(|i| i.key.clone())
        .collect();
    let mut set = EffectiveSet {
        inputs: places,
        ..EffectiveSet::default()
    };
    for key in &keys {
        r.key(key, &mut set);
    }
    let left_out = r
        .items
        .iter()
        .filter_map(|item| Some(layer(item, item.outcome.clone()?)));
    set.problems = left_out.collect();
    set
}

/// Rules 1 and 2 for every taking-part file, sorted in resolution order, then by path.
fn read_files<'a>(inputs: &'a Inputs, places: &[Place]) -> Vec<Item<'a>> {
    let mut items = Vec::new();
    for input in &inputs.files {
        let Some(rank) = places.iter().position(|p| *p == input.place) else {
            continue;
        };
        // Rule 1: valid files only. A file read at another place than its input's is out of
        // place (failing closed); a draft or retired file is left out silently.
        let read: Result<&ProcessFile, Problem> = match &input.read {
            Ok(file) if file.place == input.place => Ok(file),
            Ok(file) => Err(Problem {
                field: "level".into(),
                kind: ProblemKind::WrongPlace,
                key: Some(file.key.clone()),
                sealed: Some(file.seals()),
            }),
            Err(problem) => Err(problem.clone()),
        };
        let (key, seals) = match &read {
            Ok(file) => (Some(file.key.clone()), file.seals()),
            Err(problem) => (problem.key.clone(), problem.sealed != Some(false)),
        };
        let file = read.as_ref().ok().copied();
        items.push(Item {
            rank,
            input,
            key,
            quiet: file.is_some_and(|f| matches!(f.status, Status::Draft | Status::Retired)),
            file,
            seals,
            outcome: read.err().map(Outcome::Invalid),
        });
    }
    items.sort_by(|a, b| (a.rank, &a.input.path).cmp(&(b.rank, &b.input.path)));
    // Rule 2: two files with one key at one place are both left out. An invalid file whose key
    // could be read is one of them (failing closed: PROC-1's "two files use the key").
    let clashed: Vec<bool> = items
        .iter()
        .map(|a| {
            let twin = |b: &&Item| !b.quiet && b.rank == a.rank && b.key == a.key;
            !a.quiet && a.key.is_some() && items.iter().filter(twin).count() > 1
        })
        .collect();
    for (item, _) in items
        .iter_mut()
        .zip(clashed)
        .filter(|(_, clashed)| *clashed)
    {
        item.outcome.get_or_insert(Outcome::Clashed);
    }
    items
}

impl<'a> Resolver<'a> {
    fn key(&mut self, key: &ProcessKey, set: &mut EffectiveSet) {
        let org = self.live(&Place::Organization, key);
        let (folds, winner, conflict) = self.teams(key, org);
        let conflicting =
            |n: &usize| conflict.is_some() && folds.iter().any(|f| f.winner == Some(*n));
        let mut chain = Vec::new();
        for (n, item) in self.items.iter().enumerate() {
            if item.quiet || item.key.as_ref() != Some(key) {
                continue;
            }
            let outcome = item.outcome.clone().unwrap_or(match Some(n) {
                n if n == winner => Outcome::Applied,
                Some(n) if conflicting(&n) => Outcome::Conflict,
                _ => Outcome::Overridden,
            });
            chain.push(layer(item, outcome));
        }
        let winner = winner.map(|w| self.winner(w));
        set.entries.push(Entry {
            key: key.clone(),
            winner,
            chain,
        });
        set.conflicts.extend(conflict);
    }

    fn winner(&self, w: usize) -> Winner {
        let file = self.file(w);
        let parent = self.parents.get(&w).copied();
        let kind = match (&file.place, parent) {
            (Place::Organization, _) => Kind::Inherited,
            (_, Some(_)) => Kind::Override,
            (_, None) => Kind::Addition,
        };
        Winner {
            path: self.items[w].input.path.clone(),
            file: file.clone(),
            kind,
        }
    }

    /// The live file with `key` at `place`: valid, active or off, not left out by a rule.
    fn live(&self, place: &Place, key: &ProcessKey) -> Option<usize> {
        self.items.iter().position(|item| {
            item.input.place == *place
                && item.outcome.is_none()
                && !item.quiet
                && item.key.as_ref() == Some(key)
        })
    }

    fn file(&self, n: usize) -> &'a ProcessFile {
        self.items[n].file.expect("a live item is valid")
    }
}

fn layer(item: &Item<'_>, outcome: Outcome) -> Layer {
    Layer {
        place: item.input.place.clone(),
        path: item.input.path.clone(),
        key: item.key.clone(),
        version: item.file.map(|file| file.version),
        outcome,
    }
}

#[cfg(test)]
mod tests;
