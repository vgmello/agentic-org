//! Why the startup UI build was refused: the one error type the build loader (`assets`),
//! its policy check (`csp`) and the signed-release reader (`release`) share. A leaf, so
//! `csp` can name it without depending on `assets`.

/// Why the startup UI build was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    TooLarge,
    TooManyFiles,
    InvalidKey,
    DuplicateKey,
    MissingEntry,
    InvalidEntry,
    InvalidHtml,
    MissingInlineScript,
    InlineEventHandler,
    JavascriptUrl,
    CspMetaTag,
    InvalidPolicy,
    UnreadableBuild,
    UnsafeBuildEntry,
    UnauthenticatedRelease,
    ReleaseImageMismatch,
}
