use std::fmt;

/// A stored probe outcome reason; unknown text is preserved for future readers.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// A destructive signal failed.
    Kill,
    /// A required recovery checkpoint failed.
    Checkpoint,
    /// A process scan failed.
    Scan,
    /// Reaping the probe leader failed.
    Reap,
    /// The probe no longer owns the child process.
    OwnershipLost,
    /// The live stop exhausted its deadline.
    Deadline,
    /// The platform scanner could not be initialized.
    ScannerUnavailable,
    /// The saved process inventory exceeded its bound.
    InventoryOverflow,
    /// Startup recovery exhausted its shared deadline.
    RecoveryDeadline,
    /// Directory permissions prevented removal.
    Permissions,
    /// A directory belongs to another user.
    Foreign,
    /// A mount boundary prevented removal.
    Mount,
    /// An immutable filesystem flag prevented removal.
    Uchg,
    /// The directory nesting bound was reached.
    Depth,
    /// A directory entry was replaced during cleanup.
    Replaced,
    /// Cleanup exhausted its time or entry budget.
    Budget,
    /// Startup recovery failed with a named I/O error kind.
    Recovery(ErrorKindName),
    /// The directory anchor could not be inspected.
    Anchor(ErrorKindName),
    /// A filesystem operation failed with a symbolic or numeric errno.
    Error(ErrnoName),
    /// An unrecognized stored spelling, retained byte for byte.
    Unknown(String),
}

impl Reason {
    /// Parse stored text without losing unknown spellings.
    pub fn parse(text: &str) -> Self {
        match text {
            "kill" => Self::Kill,
            "checkpoint" => Self::Checkpoint,
            "scan" => Self::Scan,
            "reap" => Self::Reap,
            "ownership_lost" => Self::OwnershipLost,
            "deadline" => Self::Deadline,
            "scanner_unavailable" => Self::ScannerUnavailable,
            "inventory_overflow" => Self::InventoryOverflow,
            "recovery: deadline" => Self::RecoveryDeadline,
            "permissions" => Self::Permissions,
            "foreign" => Self::Foreign,
            "mount" => Self::Mount,
            "uchg" => Self::Uchg,
            "depth" => Self::Depth,
            "replaced" => Self::Replaced,
            "budget" => Self::Budget,
            _ => match text.split_once(": ") {
                Some(("recovery", parameter)) => {
                    ErrorKindName::parse(parameter).map(Self::Recovery)
                }
                Some(("anchor", parameter)) => ErrorKindName::parse(parameter).map(Self::Anchor),
                Some(("error", parameter)) => ErrnoName::parse(parameter).map(Self::Error),
                _ => None,
            }
            .unwrap_or_else(|| Self::Unknown(text.to_owned())),
        }
    }

    /// Whether this reason requires manual action rather than another startup.
    pub fn is_persistent(&self) -> bool {
        matches!(
            self,
            Self::InventoryOverflow
                | Self::Permissions
                | Self::Foreign
                | Self::Mount
                | Self::Uchg
                | Self::Depth
        )
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kill => f.write_str("kill"),
            Self::Checkpoint => f.write_str("checkpoint"),
            Self::Scan => f.write_str("scan"),
            Self::Reap => f.write_str("reap"),
            Self::OwnershipLost => f.write_str("ownership_lost"),
            Self::Deadline => f.write_str("deadline"),
            Self::ScannerUnavailable => f.write_str("scanner_unavailable"),
            Self::InventoryOverflow => f.write_str("inventory_overflow"),
            Self::RecoveryDeadline => f.write_str("recovery: deadline"),
            Self::Permissions => f.write_str("permissions"),
            Self::Foreign => f.write_str("foreign"),
            Self::Mount => f.write_str("mount"),
            Self::Uchg => f.write_str("uchg"),
            Self::Depth => f.write_str("depth"),
            Self::Replaced => f.write_str("replaced"),
            Self::Budget => f.write_str("budget"),
            Self::Recovery(kind) => write!(f, "recovery: {}", kind.as_str()),
            Self::Anchor(kind) => write!(f, "anchor: {}", kind.as_str()),
            Self::Error(errno) => write!(f, "error: {}", errno.as_str()),
            Self::Unknown(raw) => f.write_str(raw),
        }
    }
}

/// A non-empty ASCII alphabetic I/O error name, including future names.
///
/// ```
/// use org_supervise::recovery::probe::ErrorKindName;
/// let name = ErrorKindName::parse("FutureKind").unwrap();
/// assert_eq!(name.as_str(), "FutureKind");
/// ```
/// ```compile_fail
/// use org_supervise::recovery::probe::ErrorKindName;
/// let name = ErrorKindName("FutureKind".to_owned());
/// assert_eq!(name.as_str(), "FutureKind");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorKindName(String);

impl ErrorKindName {
    /// Validate stored syntax without restricting the reader to known Rust kinds.
    pub fn parse(text: &str) -> Option<Self> {
        (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphabetic()))
            .then(|| Self(text.to_owned()))
    }

    /// Borrow the validated spelling without normalization.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A symbolic `E[A-Z0-9]+` name or `errno N` for canonical positive `i32` N.
///
/// ```
/// use org_supervise::recovery::probe::ErrnoName;
/// let name = ErrnoName::parse("errno 2147483647").unwrap();
/// assert_eq!(name.as_str(), "errno 2147483647");
/// ```
/// ```compile_fail
/// use org_supervise::recovery::probe::ErrnoName;
/// let name = ErrnoName("errno 2147483647".to_owned());
/// assert_eq!(name.as_str(), "errno 2147483647");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrnoName(String);

impl ErrnoName {
    /// Validate a symbolic errno or a canonical decimal in `1..=i32::MAX`.
    pub fn parse(text: &str) -> Option<Self> {
        let symbolic = text.strip_prefix('E').is_some_and(|tail| {
            !tail.is_empty()
                && tail
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        });
        let numeric = text.strip_prefix("errno ").is_some_and(|number| {
            number
                .parse::<i32>()
                .is_ok_and(|errno| errno > 0 && errno.to_string() == number)
        });
        (symbolic || numeric).then(|| Self(text.to_owned()))
    }

    /// Borrow the validated spelling without normalization.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
