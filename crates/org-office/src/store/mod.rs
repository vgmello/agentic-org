//! The office store: [`open`] opens it on the engine its configuration names and runs
//! operations on it; [`sqlite`] is its SQLite engine; [`layout`] names the companion
//! stores' durable folders every engine shares.

pub(crate) mod layout;
pub(crate) mod open;
pub(crate) mod sqlite;
