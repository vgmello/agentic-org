//! An instance's browser listener as setup and the registered start both see it: whether the
//! listener saved for a start still matches the one setup froze, and what is said about a
//! plain-HTTP listener on a published port.

use org_host_control::listen::{ListenConfig, ListenMode};

/// What setup and the host say about the exposure of an explicit plain-HTTP listener.
pub(crate) fn published_http_notice(listen: &ListenConfig) -> String {
    format!(
        "serving plain HTTP on a published port ({}). Every machine that can reach the \
         listen address can reach sign-in and sessions in plain text. In Docker, publish \
         only on the host's 127.0.0.1 (docker run -p 127.0.0.1:{port}:{port}). Open {}. \
         For HTTPS, set up an instance with \"scheme\": \"https\".",
        std::net::SocketAddr::new(listen.address(), listen.port()),
        listen.origin().as_str(),
        port = listen.port(),
    )
}

/// Whether an instance's saved browser listener matches its frozen setup record: either
/// identical, or an opt-in HTTPS upgrade / HSTS update (`ADR-0033`) preserving the exact
/// reserved listen address, port, origin host, and origin port. Downgrades from `https`
/// to plain HTTP and any address/port/host changes are refused.
pub(crate) fn browser_matches_setup(active: &ListenConfig, setup: &ListenConfig) -> bool {
    if active == setup {
        return true;
    }
    active.scheme() == ListenMode::Https
        && active.address() == setup.address()
        && active.port() == setup.port()
        && active.origin().host() == setup.origin().host()
        && active.origin().port() == setup.origin().port()
}
