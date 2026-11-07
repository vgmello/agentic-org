use std::fmt;
use std::io;
use std::path::PathBuf;

/// Errors that stop an operation. Anything wrong *inside* a workspace is a
/// [`crate::Problem`] on the model instead.
#[derive(Debug)]
pub enum Error {
    /// The workspace root doesn't exist.
    RootNotFound(PathBuf),
    /// The workspace root isn't a directory.
    RootNotDirectory(PathBuf),
    /// A workspace-relative path was absolute.
    AbsolutePath(PathBuf),
    /// A workspace-relative path had a `..` component.
    ParentComponent(PathBuf),
    /// A path contained a NUL byte.
    InvalidPath(PathBuf),
    /// The path doesn't exist in the workspace.
    NotFound(PathBuf),
    /// The path resolves (through a symlink) to somewhere outside the workspace.
    OutsideWorkspace(PathBuf),
    Io {
        path: PathBuf,
        source: io::Error,
    },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootNotFound(p) => write!(f, "workspace root {} doesn't exist", p.display()),
            Self::RootNotDirectory(p) => {
                write!(f, "workspace root {} isn't a directory", p.display())
            }
            Self::AbsolutePath(p) => write!(
                f,
                "{} is absolute; expected a workspace-relative path",
                p.display()
            ),
            Self::ParentComponent(p) => write!(f, "{} contains '..'", p.display()),
            Self::InvalidPath(p) => write!(f, "{} isn't a valid path", p.display()),
            Self::NotFound(p) => write!(f, "{} doesn't exist in the workspace", p.display()),
            Self::OutsideWorkspace(p) => write!(f, "{} points outside the workspace", p.display()),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
