//! The organization's structure on the native protocol: `org.snapshot` read from the
//! organization and projects families, and the structure commands (teams, people and
//! projects) mapped onto those families' operations and back. It sits above both families,
//! which never read it.

pub mod snapshot;
pub mod wire;
