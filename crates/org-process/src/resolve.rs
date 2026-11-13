//! Which processes apply to an agent's work (processes.md §3): [`resolve`] applies §3.2's rules
//! 1-9, in order, to plain data, and says what happened to every file (PROC-2 to PROC-9).

use std::collections::{BTreeMap, BTreeSet};

use crate::file::{Problem, ProblemKind, ProcessFile, Status};
use crate::place::{MemberId, Place, ProcessKey, ProjectKey, TeamId};

mod chains;
mod lower;

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

impl Input {
    /// Whether it seals its key, or can't be shown not to (rule 2): a valid file that
    /// [`ProcessFile::seals`], or an invalid one whose `overridable` isn't read as `true`, unless
    /// it is a draft or retired one. Out of place or not, the same.
    pub(crate) fn seals(&self) -> bool {
        match &self.read {
            Ok(file) => file.seals(),
            Err(problem) => problem.sealed != Some(false),
        }
    }
}

/// The agent's current work: a project and its teams (§3.1). Built only by [`Work::new`], which
/// checks the chains:
///
/// ```compile_fail,E0451
/// use org_process::{ProjectKey, Work};
/// let made = Work { project: ProjectKey::new("atlas").unwrap(), chains: vec![] };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Work {
    project: ProjectKey,
    chains: Vec<Vec<TeamId>>,
}

/// Why [`Work::new`] refused the chains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainError {
    /// A chain names no team.
    Empty,
    /// A chain names this team twice.
    Repeated(TeamId),
    /// This team has different ancestors in two chains: a chain is its last team's parent path,
    /// so a team always has the same ones.
    Ancestors(TeamId),
}

impl Work {
    /// The project's work, with, for each team linked to the project, its chain from the
    /// outermost parent down to it. Refused: an empty chain, a team twice in one chain, and a
    /// team whose ancestors differ between chains. A chain that is a prefix of another (a linked
    /// ancestor of another linked team) or a copy of one is accepted, and dropped when resolving.
    pub fn new(project: ProjectKey, chains: Vec<Vec<TeamId>>) -> Result<Work, ChainError> {
        for chain in &chains {
            if chain.is_empty() {
                return Err(ChainError::Empty);
            }
            if let Some(i) = (1..chain.len()).find(|&i| chain[..i].contains(&chain[i])) {
                return Err(ChainError::Repeated(chain[i].clone()));
            }
        }
        // No chain repeats a team now, so `position` finds its only place in a chain.
        for chain in &chains {
            for (i, team) in chain.iter().enumerate() {
                let differ = |other: &Vec<TeamId>| {
                    let at = other.iter().position(|t| t == team);
                    at.is_some_and(|at| other[..at] != chain[..i])
                };
                if chains.iter().any(differ) {
                    return Err(ChainError::Ancestors(team.clone()));
                }
            }
        }
        Ok(Work { project, chains })
    }

    /// The project.
    pub fn project(&self) -> &ProjectKey {
        &self.project
    }

    /// The chains, as given.
    pub fn chains(&self) -> &[Vec<TeamId>] {
        &self.chains
    }
}

/// What agent A's set is computed from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inputs {
    /// A.
    pub agent: MemberId,
    /// A's current work; with none the set is O ⊕ G (PROC-2).
    pub work: Option<Work>,
    /// The files: O, the teams', J (none without a repository) and G. A file of another place
    /// (another agent, project or team) takes no part.
    pub files: Vec<Input>,
    /// The names the app shows for places, for "Turned off for <name>: <reason>". A place with
    /// none is named by its team id or project key, or "the organization".
    pub names: BTreeMap<Place, String>,
}

/// The effective set (§3.2 Output).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectiveSet {
    /// The places that took part, in resolution order: the organization, the chains' teams
    /// outermost first, the project, the agent.
    pub inputs: Vec<Place>,
    /// One per key a taking-part file has (drafts and retired files aside), by key.
    pub entries: Vec<Entry>,
    /// The keys a surviving `off` removed (rule 7).
    pub turned_off: Vec<TurnedOff>,
    /// Team versions that disagree, which the project hasn't settled (rule 5).
    pub conflicts: Vec<Conflict>,
    /// Every file left out with something to fix: invalid, clashed, refused (sealed, or maybe
    /// sealed by a file whose key can't be read) or a private file that doesn't only add.
    pub problems: Vec<Layer>,
}

/// A key: the file that applies, if any, and every layer, as `process why` prints it (PROC-8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The key.
    pub key: ProcessKey,
    /// What applies; none when nothing does (turned off, refused, a conflict with no fallback).
    pub winner: Option<Winner>,
    /// Each file with the key, organization first, private last; drafts and retired files aside.
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
    /// For an override: whether its parent changed since it was written (rule 9).
    pub drift: Option<Drift>,
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

/// An override's drift (PROC-9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drift {
    /// It was written against the parent's current version.
    Current,
    /// "Parent changed": its `overrides` names a version below the parent's current one.
    ParentChanged {
        /// The parent's place.
        parent: Place,
        /// The parent's current version.
        version: u32,
    },
    /// It has no `overrides`, names a version above the parent's current one, or settles a
    /// conflict with no version applying.
    Unknown,
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
    /// It applies as an `off`: the key is turned off (rule 7).
    TurnedOff,
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
    /// "Can't tell: <path> can't be read": a file above it whose key can't be read might seal its
    /// key, or use it (for a private file), so it is left out (rules 2, 3 and 8).
    RefusedUnknown {
        /// That file's path.
        path: String,
    },
    /// A private file that doesn't only add (rule 8).
    PrivateOnlyAdds(PrivateReason),
}

/// The case a private file is flagged for (processes-ux.md decision 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateReason {
    /// It sets `overrides`.
    Overrides,
    /// It is `status: off`.
    TurnsOff,
    /// An organization, team or project file of this work uses its key, in any state.
    Inherited,
}

/// A key turned off: "Turned off for <name>: <reason>" (PROC-4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnedOff {
    /// The key.
    pub key: ProcessKey,
    /// The place of the `off`.
    pub place: Place,
    /// That place's name, from [`Inputs::names`], else its team id or project key, or "the
    /// organization".
    pub name: String,
    /// Its reason.
    pub reason: String,
}

impl TurnedOff {
    /// The app's line.
    pub fn message(&self) -> String {
        format!("Turned off for {}: {}", self.name, self.reason)
    }
}

/// Teams that disagree on a key, which the project hasn't settled (decision 4, PROC-6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The key.
    pub key: ProcessKey,
    /// The disagreeing versions' places (a team that sealed what it inherited, or can't be shown
    /// not to have, stands for it).
    pub teams: Vec<Place>,
    /// The teams that sealed the key; in a chain whose seal is a "can't tell", the outermost team
    /// below it whose own file seals the key (its file is refused there, but it is named, beside
    /// [`Conflict::unknown`]).
    pub sealed_by: Vec<Place>,
    /// "Can't tell: <path> can't be read": in a disagreeing chain, a file whose key can't be read
    /// might seal the key, so it counts as that chain's seal (rule 2, failing closed), but no
    /// team is named as sealing it. Each such file's path.
    pub unknown: Vec<String>,
    /// What applies until the project decides: the organization's, a sealed team's (one counted
    /// in [`Conflict::unknown`] included), or nothing.
    pub applies: Option<Place>,
}

/// The name the app shows for `place`: its name in `names`, else its team id, project key or
/// member id, or "the organization".
pub(crate) fn place_name(names: &BTreeMap<Place, String>, place: &Place) -> String {
    let scope = || {
        place
            .scope()
            .map_or("the organization".into(), str::to_owned)
    };
    names.get(place).cloned().unwrap_or_else(scope)
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
    /// A draft or retired file, valid or not: it never seals or clashes (rule 2).
    exempt: bool,
    /// It seals its key, or can't be shown not to (rule 2).
    seals: bool,
    /// Set when a rule leaves it out.
    outcome: Option<Outcome>,
}

struct Resolver<'a> {
    inputs: &'a Inputs,
    chains: Vec<Vec<Place>>,
    items: Vec<Item<'a>>,
    /// What each override replaced (rule 9): a version, or none for a settled conflict.
    parents: BTreeMap<usize, Option<usize>>,
}

/// Agent A's effective set for its work: §3.2's rules 1-9, in order.
pub fn resolve(inputs: &Inputs) -> EffectiveSet {
    let mut places = vec![Place::Organization];
    let mut chains = Vec::new();
    if let Some(work) = &inputs.work {
        chains = chains::kept(work.chains());
        for place in chains.iter().flatten() {
            if !places.contains(place) {
                places.push(place.clone());
            }
        }
        places.push(Place::Project(work.project().clone()));
    }
    places.push(Place::Agent(inputs.agent.clone()));
    let mut r = Resolver {
        inputs,
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
        // place (failing closed), naming the field that differs; a valid draft or retired file
        // is left out silently.
        let read: Result<&ProcessFile, Problem> = match &input.read {
            Ok(file) if file.place == input.place => Ok(file),
            Ok(file) => Err(Problem {
                field: match file.place.level() == input.place.level() {
                    true => "scope".into(),
                    false => "level".into(),
                },
                kind: ProblemKind::WrongPlace,
                key: Some(file.key.clone()),
                sealed: Some(file.seals()),
                status: Some(file.status),
            }),
            Err(problem) => Err(problem.clone()),
        };
        let (key, status) = match &read {
            Ok(file) => (Some(file.key.clone()), Some(file.status)),
            Err(problem) => (problem.key.clone(), problem.status),
        };
        let seals = input.seals();
        let file = read.as_ref().ok().copied();
        let exempt = matches!(status, Some(Status::Draft | Status::Retired));
        items.push(Item {
            rank,
            input,
            key,
            quiet: file.is_some() && exempt,
            exempt,
            file,
            seals,
            outcome: read.err().map(Outcome::Invalid),
        });
    }
    items.sort_by(|a, b| (a.rank, &a.input.path).cmp(&(b.rank, &b.input.path)));
    // Rule 2: two files with one key at one place are both left out. An invalid file whose key
    // could be read is one of them (failing closed: PROC-1's "two files use the key"), unless
    // it is a draft or retired one.
    let clashed: Vec<bool> = items
        .iter()
        .map(|a| {
            let twin = |b: &&Item| !b.exempt && b.rank == a.rank && b.key == a.key;
            !a.exempt && a.key.is_some() && items.iter().filter(twin).count() > 1
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
        let (winner, conflict) = self.project(key, &folds, winner, conflict);
        let off = self.turn_off(key, winner, set);
        let winner = self.private(key).or(winner.filter(|_| off.is_none()));
        let conflicting =
            |n: &usize| conflict.is_some() && folds.iter().any(|f| f.winner == Some(*n));
        let mut chain = Vec::new();
        for (n, item) in self.items.iter().enumerate() {
            if item.quiet || item.key.as_ref() != Some(key) {
                continue;
            }
            let outcome = item.outcome.clone().unwrap_or(match Some(n) {
                n if n == winner => Outcome::Applied,
                n if n == off => Outcome::TurnedOff,
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
            drift: parent.map(|parent| self.drift(file, parent)),
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
