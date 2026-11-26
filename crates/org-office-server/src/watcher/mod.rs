//! Per-channel tool-less question watcher and `channel summarize` service (`ADR-0024`, `TASK-48`, `C9`).
//!
//! - Uses [`org_copilot::watcher`] for tool-less, fenced-data, strict-JSON judgment runs.
//! - Uses [`WatcherStore`] for durable local-only watches, per-origin watermarks, notices,
//!   and `one_off` / caller usage ledger entries.
//! - Uses consumer-owned [`WatcherRuntimeProvider`] so production enforces strict tool-less
//!   isolation (`WATCH-4`: empty temp dir, stdin prompt, `CancelToken` kill+reap, fail-closed for
//!   drivers lacking a true tool-less mode) and tests inject deterministic fakes.

mod catchup;
mod channel_watcher;
mod pass;
mod runtime;
mod summarize;
mod watches;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use org_copilot::watcher::WatchError;
use org_copilot::{CopilotRuntime, Limits};
use org_office::organization_family::records::{ModelProfile, ProviderConnection};

pub use self::channel_watcher::{spawn_watcher_loop, ChannelWatcher};

/// Default minimum gap between watcher passes for the same channel (`ADR-0024` d9).
pub const DEFAULT_WATCH_CADENCE: Duration = Duration::from_secs(30);
/// Default maximum open watches held simultaneously by one agent (`ADR-0024` d14).
pub const DEFAULT_MAX_WATCHES_PER_AGENT: usize = 5;
/// Default maximum age of a watch before expiry (`ADR-0024` d14: 24 hours).
pub const DEFAULT_MAX_WATCH_AGE: Duration = Duration::from_secs(24 * 3600);
/// Default maximum judged messages per watch (`ADR-0024` d14).
pub const DEFAULT_MAX_WATCH_MESSAGES: usize = 200;
/// Default maximum consecutive pass failures before advancing watermarks past the batch (`ADR-0024` d9).
pub const DEFAULT_MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Configurable bounds for [`ChannelWatcher`] (`ADR-0024` d9, d14, d19).
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    pub cadence: Duration,
    pub max_watches_per_agent: usize,
    pub max_watch_age: Duration,
    pub max_watch_messages: usize,
    pub max_consecutive_failures: u32,
    /// Decision 19 gate (`WATCH-11`): confident person-message matches start idle-agent turns
    /// ONLY when the watching model has passed the >= 90% confident-match precision bar.
    pub confident_wake_enabled: bool,
    pub limits: Limits,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            cadence: DEFAULT_WATCH_CADENCE,
            max_watches_per_agent: DEFAULT_MAX_WATCHES_PER_AGENT,
            max_watch_age: DEFAULT_MAX_WATCH_AGE,
            max_watch_messages: DEFAULT_MAX_WATCH_MESSAGES,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            confident_wake_enabled: true,
            limits: Limits::default(),
        }
    }
}

/// Consumer-owned adapter resolving a configured organization [`ModelProfile`] into a tool-less
/// [`CopilotRuntime`] (`doc-8` adapter contract).
pub trait WatcherRuntimeProvider: Send + Sync + 'static {
    fn runtime_for_profile(
        &self,
        profile: &ModelProfile,
        connection: Option<&ProviderConnection>,
    ) -> Option<Arc<dyn CopilotRuntime>>;
}

/// Summary of one `ChannelWatcher::run_channel_pass` invocation (used for diagnostics & tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassReport {
    Skipped(&'static str),
    Idle,
    Delivered {
        judged_messages: usize,
        delivered_entries: usize,
        woke_agents: Vec<String>,
        skipped_secrets: usize,
    },
    FailedRetrying {
        consecutive_failures: u32,
        error: WatchError,
    },
    FailedAdvanced {
        skipped_message_ids: Vec<String>,
        error: WatchError,
    },
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn next_local_id(prefix: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{seq}", now_ms())
}

#[cfg(test)]
mod tests;
