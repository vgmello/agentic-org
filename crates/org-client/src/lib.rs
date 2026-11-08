//! # org-client: connect to the host and its offices
//!
//! Opens a connection to this machine's host or to an office server (local Unix sockets,
//! found through `org_proto::location`) and speaks `org-proto`. Used by the desktop app's
//! Tauri shell and by `org-agent`. Knows nothing about how an office stores its data.
