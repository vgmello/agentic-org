//! Rebuilding the projection from the files and the journal, deterministically.

use std::sync::Arc;

use crate::{
    change::{DurablePath, Id, Sha256, Tree},
    engine::{CorePort, Cursor, Engine, Projector},
    error::{ProjectionError, StoreError},
    files::{FileVersion, Files},
    hooks::{Hooks, NoHooks, Site},
    journal::{Entry, Journal},
    receipts::{ReceiptProjector, RECEIPTS},
};

/// The families of one store, with the core's receipt family in front.
pub(crate) struct Projectors<E: Engine> {
    families: Vec<Arc<dyn Projector<E>>>,
    /// The prefixes a rebuild walks. A file outside them would never be projected again.
    prefixes: Vec<DurablePath>,
    /// Prefixes owned by independent companion stores outside `office.db`: pruned before
    /// directory descent and file reads on rebuild, and rejected on journal writes (`TASK-97`).
    excluded: Vec<DurablePath>,
}

impl<E: Engine> Projectors<E> {
    pub(crate) fn new(families: Vec<Arc<dyn Projector<E>>>, office: &Id) -> Self {
        Self::new_with_exclusions(families, office, Vec::new())
    }

    pub(crate) fn new_with_exclusions(
        families: Vec<Arc<dyn Projector<E>>>,
        office: &Id,
        excluded: Vec<DurablePath>,
    ) -> Self {
        Self {
            families,
            prefixes: projected(office.as_str()),
            excluded,
        }
    }

    /// Whether exactly one projector owns `path`.
    pub(crate) fn owned(&self, path: &DurablePath) -> Result<(), ProjectionError> {
        if self
            .excluded
            .iter()
            .any(|ex| path == ex || path.is_under(ex))
        {
            return Err(ProjectionError::new(
                path,
                "owned by an excluded companion store",
            ));
        }
        if !self.prefixes.iter().any(|prefix| path.is_under(prefix)) {
            return Err(ProjectionError::new(
                path,
                "outside the trees a rebuild projects",
            ));
        }
        let receipts = usize::from(ReceiptProjector::owns_path(path));
        match receipts + self.families.iter().filter(|f| f.owns(path)).count() {
            1 => Ok(()),
            0 => Err(ProjectionError::new(path, "no family owns this path")),
            _ => Err(ProjectionError::new(
                path,
                "more than one family owns this path",
            )),
        }
    }

    /// Sends one file to its owner.
    pub(crate) fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        self.owned(path)?;
        if ReceiptProjector::owns_path(path) {
            return <ReceiptProjector as Projector<E>>::apply(&ReceiptProjector, tx, path, bytes);
        }
        let owner = self.families.iter().find(|f| f.owns(path));
        owner
            .expect("one owner, checked above")
            .apply(tx, path, bytes)
    }

    /// Applies an entry as the write path, replay and recovery all do: each file to its
    /// owner, then its source hash.
    pub(crate) fn apply_entry(&self, tx: &mut E::Tx, entry: &Entry) -> Result<(), StoreError> {
        for version in entry.versions() {
            self.apply(tx, version.path, version.bytes)?;
            tx.set_source(version.path, version.sha256)?;
        }
        Ok(())
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        self.families.iter().try_for_each(|f| f.validate(tx))
    }
}

/// Writes the files of every flushed entry after the materialized mark and advances it.
pub(crate) fn materialize(files: &Files, journal: &Journal) -> Result<Option<u64>, StoreError> {
    let entries = journal.after(journal.materialized());
    let Some(last) = entries.last().map(|e| e.ordinal()) else {
        return Ok(None);
    };
    let versions: Vec<FileVersion<'_>> = entries.iter().flat_map(|e| e.versions()).collect();
    files.materialize(&versions)?;
    Ok(Some(last))
}

/// What a rebuild did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RebuildReport {
    /// Files projected.
    pub files: usize,
    /// Records left out because their projector failed without refusing the rebuild.
    pub skipped: Vec<ProjectionError>,
    /// Where the new projection stands: the journal's head.
    pub cursor: Cursor,
}

/// Every prefix a rebuild walks: all of the projected trees, the receipts, and this
/// office's own directory under `local/`. The journal's own files are never projected.
fn projected(office: &str) -> Vec<DurablePath> {
    let mut prefixes: Vec<String> = Tree::ALL
        .into_iter()
        .filter(|tree| !matches!(tree, Tree::Recovery | Tree::Local))
        .map(|tree| tree.name().to_owned())
        .collect();
    prefixes.push(RECEIPTS.to_owned());
    prefixes.push(format!("local/offices/{office}"));
    prefixes.sort();
    let parsed = prefixes.iter().map(|p| DurablePath::parse(p));
    parsed.map(|p| p.expect("built from known trees")).collect()
}

/// Rebuilds the projection from the files and the journal and swaps it in.
///
/// It first writes the files of every flushed entry not yet materialized, so the files
/// hold everything accepted. Then, into a fresh projection beside the live one, it walks
/// every projected tree in byte order of path, gives each file to the one projector that
/// owns it and records its hash; a file no family owns is a failure that names the path.
/// It runs every family's `validate`, sets the cursor to the journal's head and installs
/// the new projection. If anything fails the old projection stays.
pub fn rebuild<E: Engine>(
    files: &Files,
    journal: &Journal,
    engine: &E,
    families: &[Arc<dyn Projector<E>>],
) -> Result<RebuildReport, StoreError> {
    let projectors = Projectors::new(families.to_vec(), &journal.identity().office_id);
    rebuild_with(files, journal, engine, &projectors, &NoHooks)
}

pub(crate) fn rebuild_with<E: Engine>(
    files: &Files,
    journal: &Journal,
    engine: &E,
    projectors: &Projectors<E>,
    hooks: &dyn Hooks,
) -> Result<RebuildReport, StoreError> {
    if let Some(through) = materialize(files, journal)? {
        journal.set_materialized(through)?;
    }
    let head = journal.head();
    if journal.flushed() < head.ordinal {
        return Err(StoreError::Internal(
            "a rebuild needs every journal entry flushed and materialized".into(),
        ));
    }
    let prefixes = &projectors.prefixes;
    let mut report = RebuildReport {
        files: 0,
        skipped: Vec::new(),
        cursor: head.cursor(),
    };
    engine.replace(&mut |tx| {
        report.files = 0;
        report.skipped.clear();
        for prefix in prefixes {
            for path in files.walk_excluding(prefix, &projectors.excluded)? {
                let Some(bytes) = files.read(&path)? else {
                    return Err(ProjectionError::new(
                        &path,
                        "the file vanished during the rebuild",
                    )
                    .into());
                };
                let mark = tx.mark()?;
                match projectors.apply(tx, &path, Some(&bytes)) {
                    Ok(()) => {
                        tx.set_source(&path, Some(Sha256::of(&bytes)))?;
                        report.files += 1;
                    }
                    Err(e) if !e.refuses_rebuild && !e.transaction_ended => {
                        tx.undo_to(mark)?;
                        report.skipped.push(e);
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            if hooks.crash_at(Site::DuringRebuild) {
                return Err(StoreError::stopped("the process died during the rebuild"));
            }
        }
        projectors.validate(tx)?;
        tx.check()?;
        Ok(report.cursor.clone())
    })?;
    Ok(report)
}

#[cfg(test)]
mod tests;
