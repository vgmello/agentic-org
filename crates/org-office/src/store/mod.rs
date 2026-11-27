//! The office store: [`open`] opens it on the engine its configuration names and runs
//! operations on it; [`backend`] and [`erased`] provide the consumer-owned storage seams
//! implemented by external engine crates such as `org-office-sqlite`; [`layout`] names the
//! companion stores' durable folders every engine shares.

pub(crate) mod backend;
pub(crate) mod erased;
pub mod layout;
#[cfg(any(test, feature = "testkit"))]
pub mod open;
#[cfg(not(any(test, feature = "testkit")))]
pub(crate) mod open;
