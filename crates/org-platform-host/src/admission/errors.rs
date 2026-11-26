//! Why an admission, a child, a connection's peer or the host's own image was refused.

use std::fmt;
use std::io;

use org_platform::Refused;

#[cfg(doc)]
use super::{admission, own_image, Admission};

/// Why an image was not admitted. The consumer words it for its own log.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageRefusal {
    /// The system's identity of the child could not be read. The text is the
    /// system's error, never a path or a secret.
    Identity(String),
    /// The child's code does not satisfy the requirement or the signing rule.
    NotTheImage,
    /// The image is pinned, but the system has no identity to read its hash by (a
    /// development image admitted without a check).
    Unidentified,
    /// The loaded image's hash could not be read.
    DigestUnavailable,
    /// The loaded image is not the pinned one.
    DigestMismatch,
    /// The child is no longer the image that was admitted: it replaced itself.
    Changed,
    /// A second admission of the same child gave another identity than the first.
    Readmitted,
    /// The image names no digest to check its bytes against, and it is not a
    /// development image (a system that executes verified bytes).
    Unpinned,
    /// The pinned file is not one whose bytes may be executed: the fixed reason (its
    /// kind, its owner or its mode). Never a path.
    UnsafeFile(&'static str),
    /// A scripted check refused, in the test's own words (`test-support`).
    #[cfg(any(test, feature = "test-support"))]
    Scripted(String),
}

impl fmt::Display for ImageRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(why) => write!(f, "the child's identity is unavailable: {why}"),
            Self::NotTheImage => {
                f.write_str("the child's code does not satisfy the requirement or the signing rule")
            }
            Self::Unidentified => {
                f.write_str("a pinned image needs an identity the system can read")
            }
            Self::DigestUnavailable => f.write_str("the loaded image's hash is unavailable"),
            Self::DigestMismatch => f.write_str("the loaded image is not the pinned image"),
            Self::Changed => f.write_str("the child replaced its image after it was checked"),
            Self::Readmitted => f.write_str("the child's identity changed between two admissions"),
            Self::Unpinned => f.write_str("a release image needs a SHA-256 digest"),
            Self::UnsafeFile(why) => write!(f, "the pinned image file {why}"),
            #[cfg(any(test, feature = "test-support"))]
            Self::Scripted(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ImageRefusal {}

/// The source of the error [`Admission::spawn`] and [`Admission::start`] return when
/// the child was started and then refused. The child is already killed and reaped. A
/// consumer classifies the failure by this type ([`SpawnRefused::of`]), never by the
/// message.
#[derive(Debug)]
pub struct SpawnRefused {
    /// The PID the refused child had. It names nothing any more.
    pub pid: u32,
    /// Why the child was refused.
    pub refusal: ImageRefusal,
}

impl SpawnRefused {
    /// The refusal inside `error`, if that is what it is.
    pub fn of(error: &io::Error) -> Option<&Self> {
        error.get_ref().and_then(|source| source.downcast_ref())
    }
}

impl fmt::Display for SpawnRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "child {} was refused: {}", self.pid, self.refusal)
    }
}

impl std::error::Error for SpawnRefused {}

/// The source of the error [`Admission::verify_reader`] returns when the connection's
/// peer is another process than the child, or another image of it than the admitted
/// one. Any other error of that function is the system's.
#[derive(Debug)]
pub struct NotTheChild;

impl NotTheChild {
    /// Whether `error` is this mismatch.
    pub fn is(error: &io::Error) -> bool {
        error
            .get_ref()
            .is_some_and(|source| source.is::<NotTheChild>())
    }
}

impl fmt::Display for NotTheChild {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the connection's peer is not the admitted child")
    }
}

impl std::error::Error for NotTheChild {}

/// Why no admission was made ([`admission`]). Nothing was started.
#[derive(Debug)]
pub enum AdmissionError {
    /// This system has no admission adapter.
    Unsupported(Refused),
    /// A system that executes verified bytes read the image and will not start it.
    Image(ImageRefusal),
    /// A step of preparing the image failed. The error names the system call.
    System(io::Error),
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(refused) => refused.fmt(f),
            Self::Image(refusal) => write!(f, "the image was refused: {refusal}"),
            Self::System(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AdmissionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unsupported(refused) => Some(refused),
            Self::Image(refusal) => Some(refusal),
            Self::System(error) => Some(error),
        }
    }
}

impl From<Refused> for AdmissionError {
    fn from(refused: Refused) -> Self {
        Self::Unsupported(refused)
    }
}

impl From<ImageRefusal> for AdmissionError {
    fn from(refusal: ImageRefusal) -> Self {
        Self::Image(refusal)
    }
}

impl From<io::Error> for AdmissionError {
    fn from(error: io::Error) -> Self {
        Self::System(error)
    }
}

/// For a caller whose own errors are `io::Error`s: a missing adapter stays an
/// [`io::ErrorKind::Unsupported`] error whose source is the [`Refused`], a refused image
/// is an error whose source is the [`ImageRefusal`], and the system's error is itself.
impl From<AdmissionError> for io::Error {
    fn from(error: AdmissionError) -> Self {
        match error {
            AdmissionError::Unsupported(refused) => refused.into(),
            AdmissionError::Image(refusal) => io::Error::other(refusal),
            AdmissionError::System(error) => error,
        }
    }
}

/// Why the host's own image is not the expected one ([`own_image`]).
#[derive(Debug)]
pub enum OwnImageError {
    /// This system has no loaded-image query.
    Unsupported(Refused),
    /// The system's query failed.
    Unavailable(io::Error),
    /// The loaded image is not the expected one, or nothing was expected of it.
    Mismatch,
}

impl fmt::Display for OwnImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(refused) => refused.fmt(f),
            Self::Unavailable(error) => write!(f, "the loaded image is unavailable: {error}"),
            Self::Mismatch => f.write_str("the loaded image is not the expected image"),
        }
    }
}

impl std::error::Error for OwnImageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unsupported(refused) => Some(refused),
            Self::Unavailable(error) => Some(error),
            Self::Mismatch => None,
        }
    }
}
