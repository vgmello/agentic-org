//! Rules 6 to 9: the project's version, turned-off keys, private processes and drift.

use super::chains::Fold;
use super::{
    place_name, Conflict, Drift, EffectiveSet, Outcome, PrivateReason, Resolver, TurnedOff,
};
use crate::file::{ProcessFile, Status};
use crate::place::{Level, Place, ProcessKey};

impl Resolver<'_> {
    /// Rule 6, with rule 3's project check, which runs after rule 5: the project's version wins
    /// and settles any conflict, unless the organization sealed the key, or a team seal binds
    /// it and there is no team-level conflict to settle. A seal here includes one that can't be
    /// shown not to be (rule 2: a file whose key can't be read), which refuses with "can't tell".
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
            .map(|work| Place::Project(work.project().clone()));
        let Some(j) = place.and_then(|place| self.live(&place, key)) else {
            return (winner, conflict);
        };
        let org_sealed = self.sealed(&Place::Organization, key);
        // The smallest sealing team by team id, whatever the chains' order.
        let team_seal = folds.iter().filter_map(|fold| fold.seal.clone()).min();
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
            name: place_name(&self.inputs.names, &file.place),
            reason: file.reason.clone().unwrap_or_default(),
        });
        Some(off)
    }

    /// Rule 8: a private file isn't applied, and is flagged, if it sets `overrides` or is an
    /// `off`, or if an organization, team or project file of this work uses its key in any state,
    /// or might (one whose key couldn't be read, failing closed: "can't tell"). Otherwise it
    /// applies: nothing above it has the key.
    pub(super) fn private(&mut self, key: &ProcessKey) -> Option<usize> {
        let g = self.live(&Place::Agent(self.inputs.agent.clone()), key)?;
        let file = self.file(g);
        let above = || {
            self.items
                .iter()
                .filter(|i| i.input.place.level() != Level::Agent)
        };
        let uses = above().any(|i| i.key.as_ref() == Some(key));
        let unread = above().find(|i| i.key.is_none());
        let outcome = if file.overrides.is_some() {
            Outcome::PrivateOnlyAdds(PrivateReason::Overrides)
        } else if file.status == Status::Off {
            Outcome::PrivateOnlyAdds(PrivateReason::TurnsOff)
        } else if uses {
            Outcome::PrivateOnlyAdds(PrivateReason::Inherited)
        } else if let Some(unread) = unread {
            let path = unread.input.path.clone();
            Outcome::RefusedUnknown { path }
        } else {
            return Some(g);
        };
        self.items[g].outcome = Some(outcome);
        None
    }

    /// Rule 9: an override written against a version below its parent's current one. One
    /// written against a version above it matches no version of the parent: unknown.
    pub(super) fn drift(&self, file: &ProcessFile, parent: Option<usize>) -> Drift {
        match (&file.overrides, parent.map(|p| self.file(p))) {
            (Some((_, written)), Some(parent)) if *written < parent.version => {
                Drift::ParentChanged {
                    parent: parent.place.clone(),
                    version: parent.version,
                }
            }
            (Some((_, written)), Some(parent)) if *written == parent.version => Drift::Current,
            _ => Drift::Unknown,
        }
    }
}
