//! The browser listener's TLS pair (ADR-0017 decision 10, implementation plan X7).
//!
//! Certificate generation, X.509/rustls verification, PEM file reading, and background renewal
//! live in `org-host-tls`; this module keeps the host-registry and host-secrets lifecycle
//! ([`WebTls`]) in `store` and `store_set`.

mod store;
mod store_set;

#[cfg(test)]
pub(crate) mod tests;

use std::sync::Arc;
use std::time::Duration;

pub(crate) use org_host_tls::{
    check, hex, leaf_fingerprint, make, Renewal, DAY_SECS, RENEW_EVERY, RENEW_WITHIN_DAYS,
};
pub use org_host_tls::{now, read_supplied, refusal_text, Checked, Fingerprint, Source};
pub(crate) use store::{Unservable, WebTls};

const _: () = assert!(org_host_tls::MAX_KEY_BYTES == crate::host_secrets::MAX_BYTES);

/// Starts the background TLS renewal thread wired to `tls` and the host log.
pub(crate) fn start_renewal(
    tls: Arc<WebTls>,
    every: Duration,
    renewed: Box<dyn Fn() + Send>,
) -> std::io::Result<Renewal> {
    Renewal::start(
        every,
        Box::new(move |now_secs| tls.renew_if_expiring(now_secs)),
        renewed,
        Box::new(crate::host_log::log),
    )
}
