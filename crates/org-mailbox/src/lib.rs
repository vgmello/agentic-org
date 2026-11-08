//! Office messaging domain over the ADR-0003 storage, directory and session ports.
//! Database implementations and socket authorization stay outside this crate.
#![deny(unsafe_code)]

mod error;
mod hash;
mod model;
mod ports;

pub use error::{AuthError, MailboxError, StoreError};
pub use hash::content_hash;
pub use model::*;
pub use ports::{MemberDirectory, MessageEvents, MessageStore, NoMessageEvents, SessionAuth};
