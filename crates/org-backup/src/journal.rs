//! The office's acceptance journal as a backup sees it: read-only, through `org-store`'s
//! inspection, which verifies the whole retained chain from its anchor or checkpoint.

use std::path::{Path, PathBuf};

use org_store::{journal_dir, Id, Inspection, Journal, JournalHead, OfficeIdentity};

use crate::error::BackupError;
use crate::manifest::{JournalMark, Manifest};

fn inspect(dir: &Path, probe: Option<u64>) -> Result<Inspection, BackupError> {
    Journal::inspect(dir, probe).map_err(|e| {
        BackupError::Refused(format!(
            "the acceptance journal at {} does not verify: {e:?}",
            dir.display()
        ))
    })
}

/// Where the office's journal incarnation stands, for the manifest.
pub(crate) fn journal_mark(
    durable: &Path,
    identity: &OfficeIdentity,
) -> Result<JournalMark, BackupError> {
    let dir = journal_dir(durable, identity);
    let seen = inspect(&dir, None)?;
    if seen.identity != *identity {
        return Err(BackupError::Refused(format!(
            "the journal at {} belongs to another writer than the office's reservation",
            dir.display()
        )));
    }
    Ok(JournalMark {
        incarnation: identity.journal_incarnation.as_str().to_owned(),
        head_ordinal: seen.head.ordinal,
        head_sha256: seen.head.sha256.to_hex(),
        torn_tail: seen.torn.is_some(),
    })
}

/// The journal folder a manifest names below `durable`.
pub(crate) fn manifest_journal_dir(
    durable: &Path,
    manifest: &Manifest,
) -> Result<PathBuf, BackupError> {
    let office = Id::parse(&manifest.office_id)
        .map_err(|_| BackupError::invalid("its manifest has an invalid office_id"))?;
    let incarnation = Id::parse(&manifest.journal.incarnation)
        .map_err(|_| BackupError::invalid("its manifest has an invalid journal incarnation"))?;
    Ok(durable
        .join("recovery/offices")
        .join(office.as_str())
        .join("journals/acceptance")
        .join(incarnation.as_str()))
}

/// Checks the restored journal against the manifest: the same organization, office and
/// incarnation, and the same head. Returns its identity and the head to pin the disposable
/// custody to.
pub(crate) fn check_restored(
    durable: &Path,
    manifest: &Manifest,
) -> Result<(OfficeIdentity, JournalHead), BackupError> {
    let dir = manifest_journal_dir(durable, manifest)?;
    let seen = Journal::inspect(&dir, None).map_err(|e| {
        BackupError::invalid(format!("its acceptance journal does not verify: {e:?}"))
    })?;
    let identity = &seen.identity;
    if identity.organization_id.as_str() != manifest.org_id
        || identity.office_id.as_str() != manifest.office_id
        || identity.journal_incarnation.as_str() != manifest.journal.incarnation
    {
        return Err(BackupError::invalid(
            "its journal belongs to another organization, office or incarnation than its manifest names",
        ));
    }
    if seen.head.ordinal != manifest.journal.head_ordinal
        || seen.head.sha256.to_hex() != manifest.journal.head_sha256
        || seen.torn.is_some() != manifest.journal.torn_tail
    {
        return Err(BackupError::invalid(format!(
            "its journal head is ordinal {} but its manifest records {}",
            seen.head.ordinal, manifest.journal.head_ordinal
        )));
    }
    let head = JournalHead {
        ordinal: seen.head.ordinal,
        sha256: seen.head.sha256,
        high_waters: seen.head.high_waters,
    };
    Ok((seen.identity, head))
}

/// What the live journal of an office says about restoring an archive over it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LiveJournal {
    /// There is no journal of the office's incarnation: its durable root was lost.
    Missing,
    /// The live journal does not verify; the office will judge the restored one itself.
    Unreadable(String),
    /// The live journal's head.
    Head { ordinal: u64 },
}

/// The custody guard (`doc-91`: missing custody, a changed incarnation or a journal behind
/// its protected checkpoint needs explicit recovery, never a reset). Restoring may never
/// leave the office with a journal the host's writer custody would have to be reset for: the
/// archive must be of the incarnation the host reserved, and hold the live journal's head on
/// its own chain (the same head, or a later one).
pub(crate) fn guard_custody(
    live_durable: &Path,
    reserved: &OfficeIdentity,
    staged_durable: &Path,
    manifest: &Manifest,
) -> Result<LiveJournal, BackupError> {
    if manifest.journal.incarnation != reserved.journal_incarnation.as_str() {
        return Err(BackupError::Refused(format!(
            "the archive's journal incarnation {} is not the one the host reserved for office {} \
             ({}); restoring it would need the host's writer custody reset, which this command \
             never does (doc-91: explicit recovery)",
            manifest.journal.incarnation,
            manifest.office_id,
            reserved.journal_incarnation.as_str()
        )));
    }
    let live_dir = journal_dir(live_durable, reserved);
    if !live_dir.join("anchor.json").exists() {
        return Ok(LiveJournal::Missing);
    }
    let live = match Journal::inspect(&live_dir, None) {
        Ok(live) => live,
        Err(e) => return Ok(LiveJournal::Unreadable(format!("{e:?}"))),
    };
    let staged = inspect(
        &manifest_journal_dir(staged_durable, manifest)?,
        Some(live.head.ordinal),
    )?;
    if staged.head.ordinal < live.head.ordinal {
        return Err(BackupError::Refused(format!(
            "the archive's journal ends at ordinal {} but office {} has already accepted through \
             ordinal {}; restoring it would roll back acknowledged work and need the host's \
             writer custody reset, which this command never does (doc-91: explicit recovery)",
            staged.head.ordinal, manifest.office_id, live.head.ordinal
        )));
    }
    if staged.probe != Some(live.head.sha256) {
        return Err(BackupError::Refused(format!(
            "the archive's journal is not on the chain of office {}'s journal (they differ at \
             ordinal {}); restoring it would fork the office and need the host's writer custody \
             reset, which this command never does",
            manifest.office_id, live.head.ordinal
        )));
    }
    Ok(LiveJournal::Head {
        ordinal: live.head.ordinal,
    })
}
