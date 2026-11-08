//! # org-office: the office's domain and storage
//!
//! An office is one folder (`<home>/offices/<office-id>/`) with its own encrypted databases
//! (`org.db`: the organization's data; `office.db`: the office's own), one key, a lock that
//! makes the server the only owner, a workspace, and `office.json` marking it finished. A
//! library with no entry point: `apps/org-office-server` runs it.
//!
//! Knows nothing about sockets: callers hand it plain values and get plain values back.
//! `Office::create` / `open` / `discard` are the three ways into a folder; the engine
//! (`store.rs`: SQLCipher, one batching writer, keys, owner-only files) is private.

mod error;
mod key;
mod model;
mod name;
mod office;
mod office_file;
mod private_fs;
mod store;
mod time;
mod ulid;

pub use error::{NameError, OfficeError};
#[cfg(any(test, feature = "dev-keys"))]
pub use key::InMemoryKeys;
pub use key::{Key, KeyError, KeyProvider};
pub use model::{NewOffice, OfficeInfo, OfficeRole};
pub use name::MAX_NAME_CHARS;
pub use office::{Office, LOCK_WAIT};
