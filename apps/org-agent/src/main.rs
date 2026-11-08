//! # org-agent: the CLI agents use
//!
//! Must start and finish in milliseconds: no async runtime, no network, no server code.
//! Writes to the local spool (`org-spool`); the office server picks it up.
//! Not in phase 1. The working predecessor is `org-agent` on branch `v1-old`.

fn main() {
    println!(
        "{} {}: not implemented yet",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
}
