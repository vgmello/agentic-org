//! Connection lifetime bounds and the consolidated local Unix socket adapter.
//! HTTP parsing remains Hyper's responsibility.

mod linger;
pub mod local_unix;
mod server;
pub mod write_stall;

pub(crate) use server::try_run_http;
pub use server::{run_http, HttpLimits, ListenMode, PeerAddress};
