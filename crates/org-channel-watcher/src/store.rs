//! Durable local-only storage for the channel question watcher (`ADR-0024` d15–16, `WATCH-6/7`).
//!
//! Stored under `<office-data-root>/local/watcher/` — never organization data and never replicated:
//! - `watches.v1.json`: open and recently ended watches per asker & channel.
//! - `watermarks.v1.json`: per-channel, per-`(origin_office_id, origin_incarnation)` high-water
//!   marks and consecutive-failure counters (`WATCH-7`, `ADR-0024` d9).
//! - `watch-notices.v1.json`: pending watch-ended notices for agents' next turns (`ADR-0024` d14).
//! - `usage-one-off.v1.jsonl`: append-only durable usage records for `one_off` watcher passes
//!   and caller-attributed `channel.summarize` runs (`ADR-0024` d15/d18, `WATCH-6`).

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// One channel question watch (`ADR-0024` d2, d14, d16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEntry {
    pub watch_id: String,
    pub conversation_id: String,
    pub question_id: String,
    pub asker_id: String,
    pub question_text: String,
    #[serde(default)]
    pub additions: Vec<String>,
    pub opened_at_ms: u64,
    #[serde(default)]
    pub judged_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_reason: Option<String>,
}

impl WatchEntry {
    pub fn is_open(&self) -> bool {
        self.ended_at_ms.is_none()
    }
}

/// Persisted `watches.v1.json` envelope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchesFile {
    pub version: u32,
    #[serde(default)]
    pub watches: Vec<WatchEntry>,
}

/// Per-channel durable watcher watermarks and retry state (`ADR-0024` d9, d16, `WATCH-7`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelWatermark {
    /// Highest judged `origin_sequence` per `"{origin_office_id}:{origin_incarnation}"`.
    #[serde(default)]
    pub origin_watermarks: BTreeMap<String, u64>,
    #[serde(default)]
    pub last_pass_at_ms: u64,
    #[serde(default)]
    pub consecutive_failures: u32,
    #[serde(default)]
    pub failed_batch_ids: Vec<String>,
    #[serde(default)]
    pub total_failed_passes: u64,
}

impl ChannelWatermark {
    pub fn origin_key(origin_office_id: &str, origin_incarnation: &str) -> String {
        format!("{origin_office_id}:{origin_incarnation}")
    }

    pub fn last_seq(&self, origin_office_id: &str, origin_incarnation: &str) -> u64 {
        let key = Self::origin_key(origin_office_id, origin_incarnation);
        self.origin_watermarks.get(&key).copied().unwrap_or(0)
    }

    pub fn advance(&mut self, origin_office_id: &str, origin_incarnation: &str, seq: u64) {
        let key = Self::origin_key(origin_office_id, origin_incarnation);
        let entry = self.origin_watermarks.entry(key).or_insert(0);
        if seq > *entry {
            *entry = seq;
        }
    }
}

/// Persisted `watermarks.v1.json` envelope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatermarksFile {
    pub version: u32,
    #[serde(default)]
    pub channels: BTreeMap<String, ChannelWatermark>,
}

/// Pending notice informing an agent at its next turn why a watch ended (`ADR-0024` d14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEndNotice {
    pub employee_id: String,
    pub conversation_id: String,
    pub question_id: String,
    pub reason: String,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchNoticesFile {
    pub version: u32,
    #[serde(default)]
    pub notices: Vec<WatchEndNotice>,
}

/// Durable usage record written BEFORE inbox delivery (`ADR-0024` d15 & d18, `WATCH-6`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageLedgerEntry {
    pub id: String,
    /// `"one_off"` for product-run channel watcher passes; `"employee"` for `channel.summarize`.
    pub consumer_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    pub organization_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub conversation_id: String,
    pub feature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile_id: Option<String>,
    #[serde(default)]
    pub question_ids: Vec<String>,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    pub duration_ms: u64,
    pub recorded_at: String,
}

/// Local filesystem store for channel watcher state under `<root>/local/watcher/`.
pub struct WatcherStore {
    dir: PathBuf,
    io_lock: Mutex<()>,
}

impl WatcherStore {
    pub fn open(office_durable_root: &Path, office_id: &str) -> Self {
        let dir = office_durable_root
            .join("local")
            .join("offices")
            .join(office_id)
            .join("watcher");
        let _ = fs::create_dir_all(&dir);
        Self {
            dir,
            io_lock: Mutex::new(()),
        }
    }

    fn watches_path(&self) -> PathBuf {
        self.dir.join("watches.v1.json")
    }

    fn watermarks_path(&self) -> PathBuf {
        self.dir.join("watermarks.v1.json")
    }

    fn notices_path(&self) -> PathBuf {
        self.dir.join("watch-notices.v1.json")
    }

    fn usage_path(&self) -> PathBuf {
        self.dir.join("usage-one-off.v1.jsonl")
    }

    pub fn read_watches(&self) -> WatchesFile {
        let _g = self.io_lock.lock().unwrap_or_else(|p| p.into_inner());
        read_json_or_default(&self.watches_path(), || WatchesFile {
            version: 1,
            watches: Vec::new(),
        })
    }

    pub fn write_watches(&self, file: &WatchesFile) -> std::io::Result<()> {
        let _g = self.io_lock.lock().unwrap_or_else(|p| p.into_inner());
        write_atomic_json(&self.watches_path(), file)
    }

    pub fn read_watermarks(&self) -> WatermarksFile {
        let _g = self.io_lock.lock().unwrap_or_else(|p| p.into_inner());
        read_json_or_default(&self.watermarks_path(), || WatermarksFile {
            version: 1,
            channels: BTreeMap::new(),
        })
    }

    pub fn write_watermarks(&self, file: &WatermarksFile) -> std::io::Result<()> {
        let _g = self.io_lock.lock().unwrap_or_else(|p| p.into_inner());
        write_atomic_json(&self.watermarks_path(), file)
    }

    pub fn append_notices(&self, new_notices: &[WatchEndNotice]) -> std::io::Result<()> {
        if new_notices.is_empty() {
            return Ok(());
        }
        let _g = self.io_lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut file = read_json_or_default(&self.notices_path(), || WatchNoticesFile {
            version: 1,
            notices: Vec::new(),
        });
        file.version = 1;
        file.notices.extend(new_notices.iter().cloned());
        const MAX_NOTICES: usize = 200;
        if file.notices.len() > MAX_NOTICES {
            let drop_n = file.notices.len() - MAX_NOTICES;
            file.notices.drain(..drop_n);
        }
        write_atomic_json(&self.notices_path(), &file)
    }

    pub fn drain_notices_for(&self, employee_id: &str) -> Vec<WatchEndNotice> {
        let _g = self.io_lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut file = read_json_or_default(&self.notices_path(), || WatchNoticesFile {
            version: 1,
            notices: Vec::new(),
        });
        let (mine, rest): (Vec<_>, Vec<_>) = file
            .notices
            .into_iter()
            .partition(|n| n.employee_id == employee_id);
        if !mine.is_empty() {
            file.notices = rest;
            let _ = write_atomic_json(&self.notices_path(), &file);
        }
        mine
    }

    pub fn append_usage(&self, entry: &UsageLedgerEntry) -> std::io::Result<()> {
        let _g = self.io_lock.lock().unwrap_or_else(|p| p.into_inner());
        fs::create_dir_all(&self.dir)?;
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.usage_path())?;
        writeln!(file, "{line}")?;
        file.sync_all()
    }

    #[cfg(any(test, feature = "testkit"))]
    pub fn read_usage_records(&self) -> Vec<UsageLedgerEntry> {
        let _g = self.io_lock.lock().unwrap_or_else(|p| p.into_inner());
        let Ok(raw) = fs::read_to_string(self.usage_path()) else {
            return Vec::new();
        };
        raw.lines()
            .filter_map(|line| serde_json::from_str::<UsageLedgerEntry>(line).ok())
            .collect()
    }
}

fn read_json_or_default<T: for<'de> Deserialize<'de>>(
    path: &Path,
    default: impl FnOnce() -> T,
) -> T {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(default)
}

fn write_atomic_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)
}
