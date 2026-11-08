//! # org-client: connect to an office
//!
//! Opens a connection to an office server (a local Unix socket today, an authenticated
//! remote connection later) and speaks `org-proto`. Used by the desktop app's Tauri shell.
//! Knows nothing about how an office stores its data.
