//! The office store's engine-neutral foundation (ADR-0021, `docs/design/office-store.md`).
//!
//! An operation is accepted when its entry is appended to the acceptance journal. The
//! projection (an [`Engine`]) is a function of the files those entries leave behind, and
//! can be deleted and rebuilt. This crate depends on no record family and names no
//! database: a family supplies a [`Projector`] and decisions that return a [`Change`].
#![deny(unsafe_code)]

mod change;
mod changes;
mod custody;
mod disk;
mod engine;
mod error;
mod files;
mod flusher;
mod journal;
mod rebuild;
mod receipts;
mod sequences;
mod store;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
mod writer;

pub use change::{
    Ack, Actor, Bytes, Change, Clock, DurablePath, FileChange, Id, Operation, Outcome, Receipt,
    Refusal, Sha256, SystemClock, Timestamp, Tree,
};
pub use changes::{ChangeStream, Committed, Resync};
pub use custody::{CustodyError, JournalHead, OfficeIdentity, WriterAuthority};
pub use engine::{CorePort, CoreRead, Cursor, Engine, Mark, Projector};
pub use error::{EngineError, ProjectionError, StoreError};
pub use files::{FileOp, FileVersion, Files, FilesError, MAX_FILE_BYTES};
pub use journal::{
    journal_dir, positions_between, AnchorFacts, Entry, EntryDraft, FileDraft, FileRecord, Header,
    Journal, JournalError, JournalPoint, OperationRecord, Predecessor, Recovered, MAX_ENTRY_BYTES,
    SEGMENT_BYTES,
};
pub use rebuild::{rebuild, RebuildReport};
pub use receipts::{decode_receipt, encode_receipt, receipt_path, ReceiptProjector, RECEIPTS};
pub use sequences::{FeedPosition, HighWaters, Sequences, Stream, MAX_POSITION};
pub use store::{Limits, Opening, Reply, Status, Store, StoreParts, MAX_BATCH};
pub use writer::UnitOfWork;
