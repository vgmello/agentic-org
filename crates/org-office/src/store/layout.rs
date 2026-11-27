//! The companion stores' durable folders (`TASK-172`). They belong to the office's durable
//! layout, not to an engine (files-first, `ADR-0016`, `ADR-0018` `ENG-3`): the opening seam
//! lays them out for every engine before the engine's store opens over them, and the SQLite
//! adapters use the same paths, so every engine opens on the same durable folders. What a
//! store keeps in them can still differ by engine: the memory engine's accounting, budget,
//! provider-pool and private-notes stores hold their records in memory, while the SQLite
//! ones keep their durable records there (`private/task-notes.db`, the execution
//! authority's `coordinator.json` from the first open on, and the accounting and budget
//! record files, with their sub-folders, as records are written).

use std::path::{Path, PathBuf};

/// `<durable>/accounting/`: the `L2` usage & cost records (`TASK-53`, `doc-87`).
pub fn accounting(durable_root: &Path) -> PathBuf {
    durable_root.join("accounting")
}

/// `<durable>/budgets/`: the `L3` budgets & holds (`TASK-53`, `doc-92`).
pub fn budgets(durable_root: &Path) -> PathBuf {
    durable_root.join("budgets")
}

/// `<durable>/private/`: HQ's per-author private task notes (`TASK-50`, `doc-79`); a
/// satellite has none.
pub fn private_notes(durable_root: &Path) -> PathBuf {
    durable_root.join("private")
}

/// `<durable>/local/offices/<office>/connections/`: the office's own provider bindings
/// (`TASK-54`, `doc-88`).
pub fn provider_bindings(durable_root: &Path, office_id: &str) -> PathBuf {
    office_local(durable_root, office_id).join("connections")
}

/// Where the office keeps its agent-execution permits and cooldowns (`doc-88`, `doc-89`).
/// Only HQ owns `<durable>/authority/agent-execution/`; a satellite keeps its local offline
/// permits and replicated cooldown caches under its own
/// `<durable>/local/offices/<office>/local-execution/` (`TASK-52` / `ARCH-02`).
pub fn execution_authority(durable_root: &Path, office_id: &str, is_hq: bool) -> PathBuf {
    if is_hq {
        durable_root.join("authority").join("agent-execution")
    } else {
        office_local(durable_root, office_id).join("local-execution")
    }
}

fn office_local(durable_root: &Path, office_id: &str) -> PathBuf {
    durable_root.join("local").join("offices").join(office_id)
}

/// `<durable>/blobs/`: shared attachment bytes, content-addressed (`TASK-98.1`). Not a tree of
/// the office store: the journal never carries it.
pub fn shared_blobs(durable_root: &Path) -> PathBuf {
    durable_root.join("blobs")
}

/// `<durable>/private/blobs/`: HQ's human-private attachment bytes (`TASK-98.1`); a satellite
/// has none.
pub fn private_blobs(durable_root: &Path) -> PathBuf {
    private_notes(durable_root).join("blobs")
}
