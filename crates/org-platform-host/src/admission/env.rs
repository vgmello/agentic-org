//! The environment of a child: exactly the variables the host sets, never a loader
//! variable.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;

/// Whether `name` is a variable that makes a child load code its image does not name:
/// any name beginning `LD_` or `DYLD_` (the loaders), `GLIBC_TUNABLES`, and
/// `GCONV_PATH` (glibc loads character-set converters, shared objects, from there).
/// The same on every system, so that no caller depends on which loader the child gets.
pub fn is_loader_variable(name: &OsStr) -> bool {
    let name = name.as_bytes();
    name.starts_with(b"LD_")
        || name.starts_with(b"DYLD_")
        || name == b"GLIBC_TUNABLES"
        || name == b"GCONV_PATH"
}

/// The whole environment of a child: exactly these variables, nothing of the host's.
/// It holds no loader variable ([`is_loader_variable`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChildEnv(Vec<(OsString, OsString)>);

impl ChildEnv {
    /// An empty environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `name=value`. A loader variable is refused with
    /// [`io::ErrorKind::InvalidInput`] and nothing is added; the error names the rule,
    /// never the variable's value.
    pub fn set(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> io::Result<()> {
        let name = name.into();
        if is_loader_variable(&name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a child's environment may not set a loader variable",
            ));
        }
        self.0.push((name, value.into()));
        Ok(())
    }

    /// The variables, in the order they were set.
    pub fn pairs(&self) -> &[(OsString, OsString)] {
        &self.0
    }
}
