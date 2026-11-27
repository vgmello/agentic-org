//! Narrow channel-watcher test hooks exposed under `feature = "testkit"` for downstream
//! integration suites (such as `org-office-server`). Never compiled into production builds.

pub use crate::catchup::CATCHUP_QUESTION;
pub use crate::summarize::{record_usage, UsageContext};
