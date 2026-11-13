//! Rules 6 to 9: the project's version, turned-off keys, private processes and drift.

use super::chains::Fold;
use super::{Conflict, Drift, EffectiveSet, Outcome, PrivateReason, Resolver, TurnedOff};
use crate::file::{ProcessFile, Status};
use crate::place::{Level, Place, ProcessKey};

impl Resolver<'_> {
    /// Rule 6, with rule 3's project check, which runs after rule 5: the project's version wins
    /// and settles any conflict, unless the organization sealed the key, or a team seal binds
    /// it and there is no team-level conflict to settle.
    pub(super) fn project(
        &mut self,
        key: &ProcessKey,
        folds: &[Fold],
        winner: Option<usize>,
        conflict: Option<Conflict>,
    ) -> (Option<usize>, Option<Conflict>) {
        let place = self
            .inputs
            .work
            .as_ref()
            .map(|work| Place::Project(work.project.clone()));
        let Some(j) = place.and_then(|place| self.live(&place, key)) else {
            return (winner, conflict);
        };
        let org_sealed = self.sealed(&Place::Organization, key);
        let team_seal = folds.iter().find_map(|fold| fold.seal.clone());
        match org_sealed
            .then_some(Place::Organization)
            .or(team_seal.filter(|_| conflict.is_none()))
        {
            Some(by) => {
                self.refuse(j, by);
                (winner, conflict)
            }
            None => {
                // Settling a conflict makes it an override even when nothing applied meanwhile.
                if conflict.is_some() || winner.is_some() {
                    self.parents.insert(j, winner);
                }
                (Some(j), None)
            }
        }
    }

    /// Rule 7: a surviving `off` removes the key; returns it.
    pub(super) fn turn_off(
        &self,
        key: &ProcessKey,
        winner: Option<usize>,
        set: &mut EffectiveSet,
    ) -> Option<usize> {
        let off = winner.filter(|&w| self.file(w).status == Status::Off)?;
        let file = self.file(off);
        set.turned_off.push(TurnedOff {
            key: key.clone(),
            place: file.place.clone(),
            name: self
                .inputs
                .names
                .get(&file.place)
                .cloned()
                .unwrap_or_default(),
            reason: file.reason.clone().unwrap_or_default(),
        });
        Some(off)
    }

    /// Rule 8: a private file isn't applied, and is flagged, if it sets `overrides` or is an
    /// `off`, or if an organization, team or project file of this work uses its key in any state,
    /// or might (one whose key couldn't be read, failing closed). Otherwise it applies: nothing
    /// above it has the key.
    pub(super) fn private(&mut self, key: &ProcessKey) -> Option<usize> {
        let g = self.live(&Place::Agent(self.inputs.agent.clone()), key)?;
        let file = self.file(g);
        let inherited = self.items.iter().any(|item| {
            item.input.place.level() != Level::Agent && item.key.as_ref().is_none_or(|k| k == key)
        });
        let reason = if file.overrides.is_some() {
            PrivateReason::Overrides
        } else if file.status == Status::Off {
            PrivateReason::TurnsOff
        } else if inherited {
            PrivateReason::Inherited
        } else {
            return Some(g);
        };
        self.items[g].outcome = Some(Outcome::PrivateOnlyAdds(reason));
        None
    }

    /// Rule 9: an override written against a version below its parent's current one.
    pub(super) fn drift(&self, file: &ProcessFile, parent: Option<usize>) -> Drift {
        match (&file.overrides, parent.map(|p| self.file(p))) {
            (Some((_, written)), Some(parent)) if *written < parent.version => {
                Drift::ParentChanged {
                    parent: parent.place.clone(),
                    version: parent.version,
                }
            }
            (Some(_), Some(_)) => Drift::Current,
            _ => Drift::Unknown,
        }
    }
}
