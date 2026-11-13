//! Rules 3 to 5: seals inside a team chain, folding each chain, and combining the chains.

use std::collections::BTreeSet;

use super::{Conflict, Outcome, Resolver};
use crate::file::Status;
use crate::place::{Place, ProcessKey, TeamId};

/// A team chain folded for one key.
#[derive(Clone)]
pub(super) struct Fold {
    pub(super) winner: Option<usize>,
    /// The outermost team that seals the key.
    pub(super) seal: Option<Place>,
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
    /// seal (the organization's, or a team's above it in this chain) refused before it can win.
    fn fold(&mut self, chain: &[Place], key: &ProcessKey, org: Option<usize>) -> Fold {
        let org_sealed = self.sealed(&Place::Organization, key);
        let mut fold = Fold {
            winner: None,
            seal: None,
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
            }
        }
        fold
    }

    /// Rule 5: one distinct team claim applies; two or more are a conflict. A chain that sealed
    /// the key claims its winner, or what it inherited when it has none (decision 13: a seal is
    /// never silently cancelled by a sibling).
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
        let mut seals: Vec<(&Place, Option<usize>)> = claims
            .iter()
            .filter_map(|f| Some((f.seal.as_ref()?, f.winner)))
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
        let sealed_by: BTreeSet<&Place> = seals.iter().map(|seal| seal.0).collect();
        let conflict = Conflict {
            key: key.clone(),
            teams: teams.into_iter().collect(),
            sealed_by: sealed_by.into_iter().cloned().collect(),
            applies: applies.map(place),
        };
        (applies, Some(conflict))
    }

    /// Whether a file at `place` seals `key` or can't be shown not to (rule 2, failing closed):
    /// a clashed or invalid one included, and one whose key couldn't be read seals every key.
    pub(super) fn sealed(&self, place: &Place, key: &ProcessKey) -> bool {
        self.items.iter().any(|item| {
            item.input.place == *place && item.seals && item.key.as_ref().is_none_or(|k| k == key)
        })
    }

    /// Rule 3: left out, with the sealing level.
    pub(super) fn refuse(&mut self, n: usize, by: Place) {
        self.items[n]
            .outcome
            .get_or_insert(Outcome::Refused { sealed_by: by });
    }
}

/// The chains §3.1 keeps: not empty, not a copy of an earlier one, and not a prefix of another
/// (a linked ancestor of another linked team, whose chain holds it).
pub(super) fn kept(chains: &[Vec<TeamId>]) -> Vec<Vec<Place>> {
    let mut out: Vec<&Vec<TeamId>> = Vec::new();
    for chain in chains {
        let prefix = chains
            .iter()
            .any(|other| other.len() > chain.len() && other.starts_with(chain));
        if !chain.is_empty() && !prefix && !out.contains(&chain) {
            out.push(chain);
        }
    }
    out.iter()
        .map(|chain| chain.iter().cloned().map(Place::Team).collect())
        .collect()
}
