//! Scalars: ids, timestamps and paths.
//!
//! Each one is a newtype over `String`. serde (de)serializes a newtype struct as its inner value,
//! so on the wire every scalar is a plain JSON string, and the generated TypeScript keeps the
//! draft's named aliases (`export type MachineId = string;`). The contract doesn't validate them:
//! `org-core` validates names and keys before building any path from them.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! string_scalar {
    ($( $(#[$doc:meta])* $name:ident; )*) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
    )*};
}

string_scalar! {
    /// Stable UUID each orgd generates on first start.
    MachineId;
    /// `<project-key>-<role>-<name>`, e.g. `ac-staff-engineer-ada`. `hr` for the HR agent.
    AgentId;
    /// 2-16 lowercase letters or digits, e.g. `ac`.
    ProjectKey;
    /// Catalog id from `hr/roles/<id>.toml`, e.g. `software-engineer`.
    RoleId;
    /// Claude Code session id (UUID). Sessions are always addressed by id, never by name.
    SessionId;
    /// RFC 3339, UTC.
    Timestamp;
    /// `YYYY-MM-DD`.
    IsoDate;
    /// Path relative to the workspace root, `/`-separated. Unvalidated: whoever reads a file
    /// through one confines it to the workspace (no absolute paths, no `..`).
    WorkspacePath;
}
