//! Office messaging domain over the ADR-0003 storage, directory and session ports.
//! Database implementations and socket authorization stay outside this crate.
#![deny(unsafe_code)]

mod error;
mod hash;
mod mailbox;
mod model;
mod ports;
mod time;
mod waiters;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

pub use error::{AuthError, MailboxError, StoreError};
pub use hash::content_hash;
pub use mailbox::{Mailbox, OfficeIdentity};
pub use model::*;
pub use ports::{
    MemberDirectory, MessageEvents, MessageStore, NoMessageEvents, SessionAuth, SyncStore,
    ThreadStore, TriageStore,
};
pub use time::{format_utc, parse_utc, Clock, SystemClock};
pub use waiters::{cancellation_pair, CancelHandle, Cancellation};
