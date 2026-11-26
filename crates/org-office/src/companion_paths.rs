//! The stores an office keeps beside its acceptance journal (companion stores): written to
//! their own files under `durable/`, never through the journal, so the office store's file
//! scan leaves them out. The store's exclusions and the agent sandbox's read denies
//! (`org-office-sessions`) are both made from this one list, so a companion store added here
//! is masked from every agent session from the day it exists.

/// What one companion store is on disk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompanionKind {
    /// A folder the store owns, with everything below it.
    Folder,
    /// One file.
    File,
}

/// One companion store of an office.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CompanionPath {
    /// The store's path below `durable/`, `/`-separated.
    pub path: String,
    pub kind: CompanionKind,
}

/// The folder below `durable/local/offices/` an office's local records live in, its own.
pub const OFFICE_LOCAL_DIR: &str = "local/offices";

/// The folder below `durable/local/offices/<office-id>/` that holds each agent's own home
/// and runtime; every other entry there is office-private.
pub const AGENTS_DIR_NAME: &str = "agents";

/// Companion stores below `durable/`, outside any one office's folder.
const DURABLE_COMPANIONS: [(&str, CompanionKind); 3] = [
    ("authority/agent-execution", CompanionKind::Folder),
    ("accounting", CompanionKind::Folder),
    ("budgets", CompanionKind::Folder),
];

/// The folder below `durable/local/offices/<office-id>/` that holds unattached uploads
/// (`TASK-98.1`): each uploader's bytes before they are bound, never journaled, masked from
/// every agent session.
pub const ATTACHMENT_STAGING_DIR_NAME: &str = "attachment-staging";

/// Companion stores below `durable/local/offices/<office-id>/`.
const OFFICE_LOCAL_COMPANIONS: [(&str, CompanionKind); 18] = [
    ("connections", CompanionKind::Folder),
    ("auth", CompanionKind::Folder),
    ("subscription-usage", CompanionKind::Folder),
    (AGENTS_DIR_NAME, CompanionKind::Folder),
    ("sessions", CompanionKind::Folder),
    ("enroll-tmp", CompanionKind::Folder),
    ("replication", CompanionKind::Folder),
    ("hq-owner.v1.json", CompanionKind::File),
    ("watcher", CompanionKind::Folder),
    ("repositories", CompanionKind::Folder),
    ("close-recovery", CompanionKind::Folder),
    ("local-execution", CompanionKind::Folder),
    ("invitations", CompanionKind::Folder),
    ("tls", CompanionKind::Folder),
    ("security/git-https-helper", CompanionKind::File),
    ("security/git-ssh-key", CompanionKind::File),
    ("security/git-known-hosts", CompanionKind::File),
    (ATTACHMENT_STAGING_DIR_NAME, CompanionKind::Folder),
];

/// Every companion store of office `office_id`, in a fixed order.
pub fn companion_store_paths(office_id: &str) -> Vec<CompanionPath> {
    let durable = DURABLE_COMPANIONS.iter().map(|(path, kind)| CompanionPath {
        path: (*path).to_owned(),
        kind: *kind,
    });
    let local = OFFICE_LOCAL_COMPANIONS
        .iter()
        .map(|(path, kind)| CompanionPath {
            path: format!("{OFFICE_LOCAL_DIR}/{office_id}/{path}"),
            kind: *kind,
        });
    durable.chain(local).collect()
}

/// The top-level entries of `durable/local/offices/<office-id>/` a companion store occupies
/// (each folder once, `security` for the stores inside it), with what each entry is.
pub fn office_local_entries() -> Vec<(&'static str, CompanionKind)> {
    let mut entries: Vec<(&'static str, CompanionKind)> = Vec::new();
    for (path, kind) in OFFICE_LOCAL_COMPANIONS {
        let entry = match path.split_once('/') {
            Some((top, _)) => (top, CompanionKind::Folder),
            None => (path, kind),
        };
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    entries
}
