//! Browser gateway composition, started only after the verified host handoff.
//!
//! The service consumes an already-bound listener. It never binds a production
//! listener, opens an office database or obtains an office database key.

#![deny(unsafe_code)]

pub mod assets;
pub mod blocking;
pub mod path;
