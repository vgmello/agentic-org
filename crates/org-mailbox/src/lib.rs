//! Office messaging domain over the ADR-0003 storage, directory and session ports.
//! Database implementations and socket authorization stay outside this crate.
#![deny(unsafe_code)]

mod deadline;
mod error;
mod hash;
mod ids;
mod mailbox;
mod model;
mod ports;
mod snoozes;
mod time;
mod waiters;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

pub use deadline::past_deadline;
pub use error::{AuthError, MailboxError, StoreError};
pub use hash::content_hash;
pub use mailbox::{Mailbox, OfficeIdentity};
pub use model::*;
pub use ports::{
    MemberDirectory, MessageEvents, MessageStore, NoMessageEvents, SessionAuth, SyncStore,
    ThreadStore, TriageStore, MAX_EXPIRED_IDS,
};
pub use time::{format_utc, parse_utc, Clock, SystemClock};
pub use waiters::{cancellation_pair, CancelHandle, Cancellation};
