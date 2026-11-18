//! The office store's engine-neutral foundation (ADR-0021, `docs/design/office-store.md`).
//!
//! An operation is accepted when its entry is appended to the acceptance journal. The
//! projection (an [`Engine`]) is a function of the files those entries leave behind, and
//! can be deleted and rebuilt. This crate depends on no record family and names no
//! database: a family supplies a [`Projector`] and decisions that return a [`Change`].
#![deny(unsafe_code)]

mod change;
mod engine;
mod error;
mod sequences;

pub use change::{
    Ack, Actor, Bytes, Change, Clock, DurablePath, FileChange, Id, Operation, Outcome, Receipt,
    Refusal, Sha256, SystemClock, Timestamp, Tree,
};
pub use engine::{CorePort, CoreRead, Cursor, Engine, Mark, Projector};
pub use error::{EngineError, ProjectionError, StoreError};
pub use sequences::{FeedPosition, HighWaters, Sequences, Stream, MAX_POSITION};
