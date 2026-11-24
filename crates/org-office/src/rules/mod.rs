//! The organization commands the native protocol carries, read from a request.
//!
//! The many-to-many membership rules that lived here are gone: the organization family on
//! the office store (`crate::organization`) decides every command. [`OrgCommand`] stays
//! until the native wire moves to the family's commands (B2.2).

mod command;

pub use command::{OrgCommand, ParsedOrgCommand};
