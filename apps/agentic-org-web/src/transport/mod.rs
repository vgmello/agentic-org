//! Connection lifetime bounds. HTTP parsing remains Hyper's responsibility.

mod linger;
mod server;
pub mod write_stall;

pub(crate) use server::try_run_http;
pub use server::{run_http, HttpLimits, ListenMode};
