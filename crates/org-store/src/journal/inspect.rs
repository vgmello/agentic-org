//! A read-only look at one journal incarnation (`TASK-104`): the identity its anchor binds
//! and the last whole, chained entry, for a backup's manifest and a restore's custody guard.
//! Nothing on the drive is changed and no custody is asked: a torn tail is reported, never
//! cut, and a segment a release left behind is skipped, never unlinked.

use std::fs;
use std::path::Path;

use super::open::read_recovery_base;
use super::segment::{ends_at_or_below, read_small, scan, Bad};
use super::{io_error, segment_name, Anchor, JournalPoint, FORMAT};
use crate::change::Sha256;
use crate::custody::OfficeIdentity;
use crate::error::JournalError;

/// What [`super::Journal::inspect`] found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Inspection {
    /// The identity the anchor binds.
    pub identity: OfficeIdentity,
    /// Where the retained chain starts: the anchor (ordinal zero) or the checkpoint.
    pub base: JournalPoint,
    /// The last whole entry that continues the chain (the base when there is none).
    pub head: JournalPoint,
    /// The ordinal of the first entry past the head that is cut or missing, if any: a torn
    /// tail an opening would truncate.
    pub torn: Option<u64>,
    /// The hash at the probed ordinal, when the retained chain holds it.
    pub probe: Option<Sha256>,
}

impl super::Journal {
    /// Verifies the chain of the incarnation in `dir` (a
    /// `.../journals/acceptance/<incarnation>` folder) without changing a byte, and reports
    /// its head. `probe` asks for the hash of one more ordinal on the retained chain.
    pub fn inspect(dir: &Path, probe: Option<u64>) -> Result<Inspection, JournalError> {
        let anchor_bytes = read_small(&dir.join("anchor.json"))?
            .ok_or(JournalError::MissingPredecessor { ordinal: 0 })?;
        let anchor: Anchor = serde_json::from_slice(&anchor_bytes)
            .map_err(|e| JournalError::UnknownFormat(format!("anchor: {e}")))?;
        if anchor.format != FORMAT {
            return Err(JournalError::UnknownFormat(anchor.format));
        }
        let identity = anchor.identity;
        let named = dir.file_name().and_then(|name| name.to_str());
        if named != Some(identity.journal_incarnation.as_str()) {
            return Err(JournalError::IdentityMismatch);
        }
        let (base, _) = read_recovery_base(dir, &identity)?;
        let firsts = retained_segments(dir, &base)?;
        let mut point = base.clone();
        let mut found = probe
            .filter(|ordinal| *ordinal == base.ordinal)
            .map(|_| base.sha256);
        let mut torn = None;
        for first in firsts {
            if first != point.ordinal + 1 {
                if first <= point.ordinal {
                    return Err(JournalError::MissingPredecessor { ordinal: first });
                }
                torn = Some(point.ordinal + 1);
                break;
            }
            let path = dir.join("segments").join(segment_name(first));
            let bytes = fs::read(&path).map_err(|e| io_error(&path, "read", &e))?;
            let mut entries = Vec::new();
            let outcome = scan(&bytes, &mut point, &identity, &mut entries);
            if let Some(wanted) = probe {
                if let Some(entry) = entries.iter().find(|e| e.ordinal() == wanted) {
                    found = Some(entry.sha256);
                }
            }
            match outcome {
                Ok(_) => {}
                Err(Bad::Chain { ordinal, reason }) => {
                    return Err(JournalError::Corrupt { ordinal, reason })
                }
                Err(Bad::Torn { ordinal, .. }) => {
                    torn = Some(ordinal);
                    break;
                }
            }
        }
        Ok(Inspection {
            identity,
            base,
            head: point,
            torn,
            probe: found,
        })
    }
}

/// The segments' first ordinals, sorted, less those a release had wholly passed (the
/// same rule an opening applies, but they are only skipped here).
fn retained_segments(dir: &Path, base: &JournalPoint) -> Result<Vec<u64>, JournalError> {
    let segments_dir = dir.join("segments");
    let mut firsts = Vec::new();
    let listing = match fs::read_dir(&segments_dir) {
        Ok(listing) => listing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(firsts),
        Err(e) => return Err(io_error(&segments_dir, "list", &e)),
    };
    for entry in listing {
        let entry = entry.map_err(|e| io_error(&segments_dir, "list", &e))?;
        let name = entry.file_name();
        let first = name
            .to_str()
            .and_then(|n| n.strip_suffix(".log"))
            .filter(|n| n.len() == 20)
            .and_then(|n| n.parse::<u64>().ok());
        if let Some(first) = first {
            firsts.push(first);
        }
    }
    firsts.sort_unstable();
    while firsts.len() > 1 && firsts[1] <= base.ordinal + 1 {
        firsts.remove(0);
    }
    if let Some(first) = firsts.first().copied().filter(|f| *f <= base.ordinal) {
        let path = segments_dir.join(segment_name(first));
        let bytes = fs::read(&path).map_err(|e| io_error(&path, "read", &e))?;
        if ends_at_or_below(&bytes, first, base)? {
            firsts.remove(0);
        }
    }
    Ok(firsts)
}
