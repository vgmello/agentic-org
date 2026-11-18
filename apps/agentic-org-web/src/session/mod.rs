//! Browser session layer (A3 B5; web-api-v1 §4, ADR-0013). The web process mints
//! the cookie and proof, sends only their digests to the host, and keeps an
//! in-memory cache of validated sessions keyed by the cookie digest. It stores
//! nothing on disk; the host owns every session record.
pub mod cache;
pub mod cookie;

pub use cache::SessionCache;
pub use cookie::{BrowserPair, CookieName, PresentedPair};
