//! # org-backup: one-command local organization backup, verification and restore
//!
//! `TASK-104`, `doc-91` (recovery and office continuity). A backup is a consistent copy of
//! one stopped office's non-rebuildable `<data-root>/<organization-id>/durable/` root: Org
//! Git `state/`, `messaging/`, `recovery/` (the acceptance journal), `authority/`,
//! `accounting/`, `budgets/`, `checkpoints/`, `blobs/` when present, and
//! `local/offices/<office-id>/`, less what is disposable there (`enroll-tmp/`, each agent's
//! `runtime/` and `tmp/`, and the host toolchain caches the office links into an agent's
//! `home/` again: `.cargo/registry`, `.cargo/git`, a legacy `.rustup`);
//! `private/task-notes.db`, a WAL-mode SQLite database, is carried as a transactionally
//! consistent `VACUUM INTO` copy. The disposable `runtime/` root is never carried. Every
//! other tree under `durable/` is carried as it is: a link, a socket, a FIFO or a device
//! there fails the backup rather than being skipped, except inside an agent's own `home/`,
//! which its runtime writes, where such an entry is left out (never followed). An archive
//! listing a path a backup never carries is refused.
//!
//! The archive is a deterministic `.tar.gz` (sorted members, normalized metadata) whose first
//! member, `manifest.json`, records the format version, the organization, the office, the
//! journal incarnation and head, when it was taken, and every file's length and SHA-256
//! ([`Manifest`]). It may be wrapped in a passphrase envelope (Argon2id and AES-256-GCM,
//! [`envelope`]), which is detected from its first bytes.
//!
//! - [`backup`] refuses while the host or the office holds its lock, and holds both locks
//!   until the archive is atomically in place.
//! - [`verify`] checks every member against the manifest under a byte budget, refuses links,
//!   special members, `..` and absolute paths, runs SQLite's integrity check on the notes copy,
//!   verifies the journal to the recorded head, and rebuilds the office's projections from the
//!   copy on a disposable runtime root.
//! - [`restore_in_place`] does all of that, then refuses an archive the host's writer custody
//!   would have to be reset for (a journal bound to another writer than the host reserved, or
//!   behind or off the live chain), discards the rebuildable projections and swaps a pristine
//!   copy in (keeping the previous root). It also restores an office whose whole
//!   `<organization-id>/` folder was lost.
//!
//! Module graph: `write, verify, restore -> read, journal, locate, collect, output`;
//! `read -> envelope, manifest, tarfmt, paths`; `restore -> verify`; nothing points back.

#![forbid(unsafe_code)]

mod collect;
pub mod envelope;
mod error;
mod journal;
mod locate;
mod manifest;
mod output;
mod paths;
mod read;
mod restore;
mod tarfmt;
mod verify;
mod write;

pub use envelope::{ArchiveKind, Passphrase, MAX_PASSPHRASE_BYTES, MIN_PASSPHRASE_BYTES};
pub use error::BackupError;
pub use journal::LiveJournal;
pub use locate::{locate, OfficeTarget};
pub use manifest::{FileMark, JournalMark, Limits, Manifest, FORMAT, VERSION};
pub use paths::{RelPath, DURABLE_PREFIX, MANIFEST_MEMBER};
pub use read::archive_kind;
pub use restore::{restore_in_place, RestoreReport, RestoreRequest, SCOPE_NOTICE};
pub use verify::{verify, Rebuilt, VerifyReport, VerifyRequest};
pub use write::{backup, BackupReport, BackupRequest};
