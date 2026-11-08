//! # org-spool: the agent-side local queue
//!
//! Fast, crash-safe local writes (write, fsync, atomic rename) used by `org-agent`.
//! The office server ingests from it. Port of the working `org-msg` crate from branch `v1-old`.
