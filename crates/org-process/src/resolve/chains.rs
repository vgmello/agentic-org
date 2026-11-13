//! Rules 3 to 5: seals inside a team chain, folding each chain, and combining the chains.

use std::collections::BTreeSet;

use super::{Conflict, Item, Outcome, Resolver};
use crate::file::Status;
use crate::place::{Place, ProcessKey, TeamId};

/// A team chain folded for one key.
#[derive(Clone)]
pub(super) struct Fold {
    pub(super) winner: Option<usize>,
    /// The outermost team that seals the key, or can't be shown not to.
    pub(super) seal: Option<Place>,
    /// When no file there seals the key itself, the path of the first file there whose key can't
    /// be read: that seal is a "can't tell", not a team's.
    pub(super) unknown: Option<String>,
    /// With that "can't tell", the first team below it in the chain whose own file seals the key:
    /// refused there, but the conflict names it as sealing the key too.
    pub(super) real: Option<Place>,
}

impl Resolver<'_> {
    /// Rules 3 to 5 for `key`: each chain folded, then combined. The organization's version is
    /// `org`.
    pub(super) fn teams(
        &mut self,
        key: &ProcessKey,
        org: Option<usize>,
    ) -> (Vec<Fold>, Option<usize>, Option<Conflict>) {
        let chains = self.chains.clone();
        let folds: Vec<Fold> = chains
            .iter()
            .map(|chain| self.fold(chain, key, org))
            .collect();
        let (winner, conflict) = self.combine(key, &folds, org);
        (folds, winner, conflict)
    }

    /// Rules 3 and 4 for one chain: outermost first, the nearer team winning, and a file below a
    /// seal (the organization's, or a team's above it in this chain; or one that can't be shown
    /// not to be) refused before it can win.
    fn fold(&mut self, chain: &[Place], key: &ProcessKey, org: Option<usize>) -> Fold {
        let org_sealed = self.sealed(&Place::Organization, key);
        let mut fold = Fold {
            winner: None,
            seal: None,
            unknown: None,
            real: None,
        };
        for place in chain {
            if let Some(n) = self.live(place, key) {
                match org_sealed
                    .then_some(Place::Organization)
                    .or(fold.seal.clone())
                {
                    Some(by) => self.refuse(n, by),
                    None => {
                        let parent = fold.winner.or(org);
                        self.parents.extend(parent.map(|parent| (n, Some(parent))));
                        fold.winner = Some(n);
                    }
                }
            }
            if fold.seal.is_none() && self.sealed(place, key) {
                fold.seal = Some(place.clone());
                fold.unknown = self.unread_seal(place, key).map(str::to_owned);
            } else if fold.unknown.is_some() && fold.real.is_none() && self.seals_itself(place, key)
            {
                fold.real = Some(place.clone());
            }
        }
        fold
    }

    /// Rule 5: one distinct team claim applies; two or more are a conflict. A chain that sealed
    /// the key claims its winner, or what it inherited when it has none (decision 13: a seal is
    /// never silently cancelled by a sibling). A chain that can't be shown not to seal it (a file
    /// whose key can't be read) counts as sealing it, failing closed, but the conflict names that
    /// file, not its team, as sealing it, and also names the outermost team below it in that
    /// chain whose own file seals the key (the P3 corner: both the can't tell and the real seal are shown).
    fn combine(
        &self,
        key: &ProcessKey,
        folds: &[Fold],
        org: Option<usize>,
    ) -> (Option<usize>, Option<Conflict>) {
        let claims: Vec<&Fold> = folds
            .iter()
            .filter(|f| f.winner.is_some() || f.seal.is_some())
            .collect();
        let distinct: BTreeSet<Option<usize>> = claims.iter().map(|fold| fold.winner).collect();
        if distinct.len() < 2 {
            return (distinct.into_iter().flatten().next().or(org), None);
        }
        let mut seals: Vec<(&Place, Option<usize>, Option<&String>)> = claims
            .iter()
            .filter_map(|f| Some((f.seal.as_ref()?, f.winner, f.unknown.as_ref())))
            .collect();
        seals.sort();
        let sealed: BTreeSet<Option<usize>> = seals.iter().map(|seal| seal.1).collect();
        let applies = match sealed.len() {
            0 => org,
            1 => sealed.into_iter().flatten().next().or(org),
            // A71's interim: the organization's version, else the first seal by team id that
            // isn't an `off`.
            _ => org.or_else(|| {
                let mut versions = seals.iter().filter_map(|seal| seal.1);
                versions.find(|&n| self.file(n).status != Status::Off)
            }),
        };
        let place = |n: usize| self.items[n].input.place.clone();
        let teams: BTreeSet<Place> = claims
            .iter()
            .filter_map(|f| f.winner.map(place).or(f.seal.clone()))
            .collect();
        let real = claims.iter().filter_map(|fold| fold.real.as_ref());
        let sealed_by: BTreeSet<&Place> = seals
            .iter()
            .filter(|seal| seal.2.is_none())
            .map(|seal| seal.0)
            .chain(real)
            .collect();
        let unknown: BTreeSet<&String> = seals.iter().filter_map(|seal| seal.2).collect();
        let conflict = Conflict {
            key: key.clone(),
            teams: teams.into_iter().collect(),
            sealed_by: sealed_by.into_iter().cloned().collect(),
            unknown: unknown.into_iter().cloned().collect(),
            applies: applies.map(place),
        };
        (applies, Some(conflict))
    }

    /// Whether a file at `place` seals `key` or can't be shown not to (rule 2, failing closed):
    /// a clashed or invalid one included, and one whose key couldn't be read seals every key
    /// unless it is shown not to seal (a readable `overridable: true`, or a draft or retired one).
    pub(super) fn sealed(&self, place: &Place, key: &ProcessKey) -> bool {
        self.items.iter().any(|item| {
            item.input.place == *place && item.seals && item.key.as_ref().is_none_or(|k| k == key)
        })
    }

    /// Whether a file at `place` whose key can be read is `key` and seals it.
    fn seals_itself(&self, place: &Place, key: &ProcessKey) -> bool {
        let sealing =
            |i: &Item<'_>| i.input.place == *place && i.seals && i.key.as_ref() == Some(key);
        self.items.iter().any(sealing)
    }

    /// When the only files at `place` that seal `key` are ones whose key can't be read, the
    /// first one's path: it can't be told whether the key is sealed there.
    fn unread_seal(&self, place: &Place, key: &ProcessKey) -> Option<&str> {
        let sealing = self
            .items
            .iter()
            .filter(|i| i.input.place == *place && i.seals);
        let sealing: Vec<_> = sealing.collect();
        match sealing.iter().find(|i| i.key.is_none()) {
            Some(unread) if !sealing.iter().any(|i| i.key.as_ref() == Some(key)) => {
                Some(&unread.input.path)
            }
            _ => None,
        }
    }

    /// Rule 3: left out, with the sealing level; or, when only files whose key can't be read
    /// seal its key there, with the first one's path ([`Self::unread_seal`]).
    pub(super) fn refuse(&mut self, n: usize, by: Place) {
        let key = self.items[n].key.as_ref();
        let outcome = match key.and_then(|key| self.unread_seal(&by, key)) {
            Some(path) => Outcome::RefusedUnknown { path: path.into() },
            None => Outcome::Refused { sealed_by: by },
        };
        self.items[n].outcome.get_or_insert(outcome);
    }
}

/// The chains §3.1 keeps: not a copy of an earlier one, and not a prefix of another (a linked
/// ancestor of another linked team, whose chain holds it). [`super::Work::new`] refused empty ones.
pub(super) fn kept(chains: &[Vec<TeamId>]) -> Vec<Vec<Place>> {
    let mut out: Vec<&Vec<TeamId>> = Vec::new();
    for chain in chains {
        let prefix = chains
            .iter()
            .any(|other| other.len() > chain.len() && other.starts_with(chain));
        if !prefix && !out.contains(&chain) {
            out.push(chain);
        }
    }
    out.iter()
        .map(|chain| chain.iter().cloned().map(Place::Team).collect())
        .collect()
}
