//! Per-channel tool-less question watcher and `channel summarize` service (`ADR-0024`, `TASK-48`, `C9`).
//!
//! - Uses [`org_copilot::watcher`] for tool-less, fenced-data, strict-JSON judgment runs.
//! - Uses [`WatcherStore`] for durable local-only watches, per-origin watermarks, notices,
//!   and `one_off` / caller usage ledger entries.
//! - Uses consumer-owned [`WatcherRuntimeProvider`] so production enforces strict tool-less
//!   isolation (`WATCH-4`: empty temp dir, stdin prompt, `CancelToken` kill+reap, fail-closed for
//!   drivers lacking a true tool-less mode) and tests inject deterministic fakes.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use org_copilot::watcher::{
    run_channel_summarize, run_watch_pass, ChannelSummarizeInput, SummarizeMessageInput,
    WatchError, WatchMatch, WatchMessageInput, WatchPassInput, WatchPassResult, WatchQuestionInput,
};
use org_copilot::{
    CancelToken, CopilotRuntime, Limits, RunGate, RunOutput, RunRequest, RuntimeError, Usage,
};
use org_office::messaging::features::AnswerCandidateDelivery;
use org_office::messaging::records::MessageRecord;
use org_office::organization_family::records::{EmployeeKind, ModelProfile, ProviderConnection};
use org_office::Office;
use org_office_sessions::{PendingWakeItem, SessionManager, WakeKind, WakeOutcome};
use org_proto::chat::{
    ChatChannelSummarizeParams, ChatChannelSummarizeResult, ChatChannelsParams, ConversationKind,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::watcher_store::{
    ChannelWatermark, UsageLedgerEntry, WatchEndNotice, WatchEntry, WatcherStore, WatchesFile,
    WatermarksFile,
};

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

/// Production CLI adapter implementing strict tool-less runs (`WATCH-4`: no tools, no files, no
/// MCP servers, no slash commands). Drivers without a true tool-less CLI mode fail closed (`None`)
/// per `doc-8` and `ADR-0024` d10.
pub struct CliWatcherRuntimeProvider;

impl WatcherRuntimeProvider for CliWatcherRuntimeProvider {
    fn runtime_for_profile(
        &self,
        profile: &ModelProfile,
        connection: Option<&ProviderConnection>,
    ) -> Option<Arc<dyn CopilotRuntime>> {
        let conn = connection?;
        if !conn.enabled || conn.archived {
            return None;
        }
        match conn.runtime_driver.as_str() {
            "claude-code" => Some(Arc::new(ClaudeToollessAdapter {
                model: profile.key.clone(),
            })),
            // Codex CLI currently has no true zero-tool/zero-file mode (`--sandbox read-only`
            // still exposes file-reading tools), so refuse at composition (`WATCH-4`, `doc-8`).
            _ => None,
        }
    }
}

struct TempWorkDir(PathBuf);

impl TempWorkDir {
    fn create(prefix: &str) -> Result<Self, RuntimeError> {
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| RuntimeError::Failed)?;
        Ok(Self(path))
    }
}

impl Drop for TempWorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ClaudeToollessAdapter {
    model: String,
}

impl CopilotRuntime for ClaudeToollessAdapter {
    fn available(&self) -> bool {
        Command::new("claude")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError> {
        if cancel.is_cancelled() {
            return Err(RuntimeError::TimedOut);
        }
        let work_dir = TempWorkDir::create("org-watcher-claude")?;
        let mut cmd = Command::new("claude");
        cmd.current_dir(&work_dir.0)
            .args([
                "-p",
                "--tools",
                "",
                "--strict-mcp-config",
                "--disable-slash-commands",
                "--output-format",
                "json",
                "--no-session-persistence",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if !self.model.is_empty() && self.model.starts_with("claude-") {
            cmd.args(["--model", &self.model]);
        }
        let mut child = cmd.spawn().map_err(|_| RuntimeError::Failed)?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(request.prompt.as_bytes());
        }
        let max_bytes = request.max_output_bytes.saturating_mul(4).max(256 * 1024);
        let stdout_bytes = wait_child_bounded(&mut child, cancel, request.timeout, max_bytes)?;
        let env: serde_json::Value =
            serde_json::from_slice(&stdout_bytes).map_err(|_| RuntimeError::Failed)?;
        if env
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err(RuntimeError::Failed);
        }
        let raw = env
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or(RuntimeError::Failed)?;
        let usage = env.get("usage").map(|u| Usage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()),
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()),
        });
        Ok(RunOutput {
            text: strip_markdown_json_fence(raw),
            usage,
        })
    }
}

/// Drains child stdout on a helper reader thread while polling `try_wait()` against `cancel` and
/// `timeout`; kills and reaps the child immediately on cancellation or timeout (`High #3`).
fn wait_child_bounded(
    child: &mut Child,
    cancel: &CancelToken,
    timeout: Duration,
    max_bytes: usize,
) -> Result<Vec<u8>, RuntimeError> {
    let stdout = child.stdout.take().ok_or(RuntimeError::Failed)?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut limited = stdout.take(max_bytes as u64 + 1);
        let _ = limited.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if cancel.is_cancelled() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(RuntimeError::TimedOut);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(RuntimeError::Failed);
            }
        }
    };
    let bytes = reader.join().map_err(|_| RuntimeError::Failed)?;
    if !status.success() || bytes.len() > max_bytes {
        return Err(RuntimeError::Failed);
    }
    Ok(bytes)
}

fn strip_markdown_json_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_owned();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_owned();
        }
    }
    trimmed.to_owned()
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

struct InFlightSlot {
    counter: Arc<AtomicUsize>,
}

impl InFlightSlot {
    fn try_acquire(counter: &Arc<AtomicUsize>, max: usize) -> Option<Self> {
        if counter.fetch_add(1, Ordering::SeqCst) >= max {
            counter.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self {
            counter: Arc::clone(counter),
        })
    }
}

impl Drop for InFlightSlot {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

struct ChannelPassGuard<'a> {
    set: &'a Mutex<BTreeSet<String>>,
    channel_id: String,
}

impl<'a> ChannelPassGuard<'a> {
    fn try_acquire(set: &'a Mutex<BTreeSet<String>>, channel_id: &str) -> Option<Self> {
        let mut guard = set.lock().unwrap_or_else(|p| p.into_inner());
        if !guard.insert(channel_id.to_owned()) {
            return None;
        }
        Some(Self {
            set,
            channel_id: channel_id.to_owned(),
        })
    }
}

impl Drop for ChannelPassGuard<'_> {
    fn drop(&mut self) {
        self.set
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.channel_id);
    }
}

/// Office-local channel question watcher & `channel.summarize` service (`ADR-0024`).
pub struct ChannelWatcher {
    office: Arc<Office>,
    sessions: Option<Arc<SessionManager>>,
    store: WatcherStore,
    runtime_provider: Arc<dyn WatcherRuntimeProvider>,
    config: WatcherConfig,
    /// Short-held mutex protecting watch/watermark read-modify-write transitions; NEVER held
    /// across blocking model subprocess calls (`High #2`).
    lock: Mutex<()>,
    in_flight_channels: Mutex<BTreeSet<String>>,
    dirty_channels: Mutex<BTreeSet<String>>,
    /// Shared office-wide concurrency cap across `channel.watch` & `channel.summarize` (`Med #10`).
    in_flight_runs: Arc<AtomicUsize>,
    summarize_gate: RunGate,
}

impl ChannelWatcher {
    pub fn new(
        office: Arc<Office>,
        sessions: Option<Arc<SessionManager>>,
        runtime_provider: Arc<dyn WatcherRuntimeProvider>,
        config: WatcherConfig,
    ) -> Self {
        let store = WatcherStore::open(office.durable_root(), &office.info().office_id);
        Self {
            office,
            sessions,
            store,
            runtime_provider,
            config,
            lock: Mutex::new(()),
            in_flight_channels: Mutex::new(BTreeSet::new()),
            dirty_channels: Mutex::new(BTreeSet::new()),
            in_flight_runs: Arc::new(AtomicUsize::new(0)),
            summarize_gate: RunGate::new(),
        }
    }

    pub fn production(office: Arc<Office>, sessions: Option<Arc<SessionManager>>) -> Self {
        Self::new(
            office,
            sessions,
            Arc::new(CliWatcherRuntimeProvider),
            WatcherConfig::default(),
        )
    }

    #[cfg(test)]
    pub fn store(&self) -> &WatcherStore {
        &self.store
    }

    /// Opens a watch when an agent asks a question in a channel (`ADR-0024` d2, d13, d14, d17).
    pub fn on_channel_ask_opened(
        &self,
        asker_id: &str,
        ask_id: &str,
        conversation_id: &str,
        conversation_kind: ConversationKind,
        question_text: &str,
    ) -> (bool, Option<String>) {
        if let Err(reason) = validate_watch_eligibility(
            &self.office,
            self.sessions.as_deref(),
            &*self.runtime_provider,
            asker_id,
            conversation_kind,
        ) {
            return (false, Some(reason.to_owned()));
        }
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        open_watch_locked(
            &self.office,
            &self.store,
            &self.config,
            asker_id,
            ask_id,
            conversation_id,
            question_text,
        )
    }

    /// Closes an open watch on answer or withdrawal (`WATCH-6`).
    pub fn on_question_closed(&self, question_id: &str, reason: &str) {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        end_watches_locked(&self.store, |w| w.question_id == question_id, reason, false);
    }

    /// Closes an open watch ONLY if `actor_id` is the watch's asker (`High #1`, `WATCH-6`).
    /// Returns `true` when at least one watch belonging to `actor_id` was closed.
    pub fn on_question_done_by(&self, actor_id: &str, question_id: &str) -> bool {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        end_watches_locked(
            &self.store,
            |w| w.question_id == question_id && w.asker_id == actor_id,
            "done",
            false,
        ) > 0
    }

    /// Ends open watches for an agent leaving a channel and records a notice (`WATCH-6`, d14).
    pub fn on_member_left_channel(&self, employee_id: &str, conversation_id: &str) {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        end_watches_locked(
            &self.store,
            |w| w.asker_id == employee_id && w.conversation_id == conversation_id,
            "left_channel",
            true,
        );
    }

    /// Ends open watches for channels `employee_id` is no longer a member of after a team move
    /// (`Low #12`, `WATCH-6`).
    pub fn on_agent_team_moved(&self, employee_id: &str) {
        let member_channels: BTreeSet<String> = self
            .office
            .list_channels(
                employee_id,
                ChatChannelsParams {
                    include_archived: false,
                },
            )
            .ok()
            .and_then(Result::ok)
            .map(|res| {
                res.channels
                    .into_iter()
                    .filter(|c| c.is_member)
                    .map(|c| c.id)
                    .collect()
            })
            .unwrap_or_default();
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        end_watches_locked(
            &self.store,
            |w| w.asker_id == employee_id && !member_channels.contains(&w.conversation_id),
            "left_channel",
            true,
        );
    }

    /// Ends all open watches for an offboarded agent (`WATCH-6`, d14).
    pub fn on_agent_offboarded(&self, employee_id: &str) {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        end_watches_locked(
            &self.store,
            |w| w.asker_id == employee_id,
            "offboarded",
            true,
        );
    }

    /// Ends all open watches for an agent moved to another office (`WATCH-6`, d14).
    #[cfg(test)]
    pub fn on_agent_moved(&self, employee_id: &str) {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        end_watches_locked(
            &self.store,
            |w| w.asker_id == employee_id,
            "moved_office",
            true,
        );
    }

    /// Drains formatted watch-ended notices for `employee_id` to prepend to its next turn
    /// (`ADR-0024` d14, `Med #8`).
    pub fn drain_watch_end_notices(&self, employee_id: &str) -> Vec<String> {
        self.store
            .drain_notices_for(employee_id)
            .into_iter()
            .map(|n| {
                format!(
                    "[Watch ended] Channel question `{}` in conversation `{}` is no longer watched ({}).",
                    n.question_id, n.conversation_id, n.reason
                )
            })
            .collect()
    }

    /// Records asker follow-up additions and queues a pass when another agent holds an open watch.
    pub fn on_channel_message_posted(&self, msg: &MessageRecord, conv_kind: ConversationKind) {
        if conv_kind != ConversationKind::Channel {
            return;
        }
        let should_queue = {
            let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
            record_channel_message_locked(&self.store, msg)
        };
        if should_queue {
            self.dirty_channels
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(msg.conversation_id.as_str().to_owned());
        }
    }

    /// Sweeps all dirty or watched channels whose cadence window has elapsed.
    pub fn process_due_channels(&self) -> Vec<(String, PassReport)> {
        let mut candidates: BTreeSet<String> = self
            .dirty_channels
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect();
        for w in self.store.read_watches().watches {
            if w.is_open() {
                candidates.insert(w.conversation_id);
            }
        }
        let mut reports = Vec::new();
        for conv_id in candidates {
            let report = self.run_channel_pass(&conv_id, false);
            if !matches!(
                report,
                PassReport::Skipped("cadence" | "in_flight" | "overloaded")
            ) {
                self.dirty_channels
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&conv_id);
            }
            reports.push((conv_id, report));
        }
        reports
    }

    /// Runs one bounded, tool-less watcher pass on `conversation_id` (`WATCH-3..8`).
    /// Never holds `self.lock` across blocking model subprocess calls (`High #2`).
    pub fn run_channel_pass(&self, conversation_id: &str, force_cadence: bool) -> PassReport {
        let (profile_id, runtime) = match resolve_watching_runtime(
            &self.office,
            self.sessions.as_deref(),
            &*self.runtime_provider,
        ) {
            Ok(pair) => pair,
            Err(reason) => return PassReport::Skipped(reason),
        };
        let Some(_chan_guard) =
            ChannelPassGuard::try_acquire(&self.in_flight_channels, conversation_id)
        else {
            return PassReport::Skipped("in_flight");
        };
        let Some(_slot) =
            InFlightSlot::try_acquire(&self.in_flight_runs, self.config.limits.max_concurrent_runs)
        else {
            return PassReport::Skipped("overloaded");
        };

        // Phase 1 (brief lock): snapshot open watches, watermarks, and unjudged messages.
        let snapshot = {
            let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
            prepare_pass_snapshot(
                &self.office,
                &self.store,
                &self.config,
                conversation_id,
                force_cadence,
                now_ms(),
            )
        };
        let snap = match snapshot {
            Ok(s) => s,
            Err(report) => return report,
        };

        // Phase 2 (unlocked): run blocking tool-less model pass and record `one_off` usage.
        let outcome = run_watch_pass(
            runtime,
            self.config.limits.clone(),
            conversation_id,
            &snap.pass_input,
        );
        if outcome.model_called {
            let _ = record_usage(
                &self.office,
                &self.store,
                UsageContext {
                    consumer_kind: "one_off",
                    employee_id: None,
                    project_id: snap.project_id.as_deref(),
                    conversation_id,
                    feature: "channel.watch",
                    model_profile_id: Some(&profile_id),
                    question_ids: snap
                        .open_watches
                        .iter()
                        .map(|w| w.question_id.clone())
                        .collect(),
                    outcome: if outcome.result.is_ok() {
                        "ok"
                    } else {
                        "failed"
                    },
                    usage: outcome.usage,
                    duration: outcome.duration,
                },
            );
        }

        // Phase 3 (brief lock): re-read watches/watermarks, filter closed watches, commit.
        let commit_now = now_ms();
        let (report, wake_work) = {
            let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
            finalize_pass_locked(
                &self.office,
                &self.store,
                &self.config,
                conversation_id,
                &snap,
                outcome.result,
                commit_now,
            )
        };

        // Phase 4 (unlocked): enqueue wakes for newly delivered confident person matches.
        if let Some(work) = wake_work {
            let woke = wake_confident_person_matches(
                &self.office,
                self.sessions.as_deref(),
                &self.config,
                conversation_id,
                &snap.new_batch,
                &work.newly_delivered,
                &work.confident_pairs,
                commit_now,
            );
            return PassReport::Delivered {
                judged_messages: snap.new_batch.len(),
                delivered_entries: work.newly_delivered.len(),
                woke_agents: woke,
                skipped_secrets: work.skipped_secrets,
            };
        }
        report
    }

    /// Runs `chat.channel_summarize` (`WATCH-10`, `ADR-0024` d18) over caller-visible messages.
    pub fn summarize_channel(
        &self,
        caller_id: &str,
        params: &ChatChannelSummarizeParams,
    ) -> Result<ChatChannelSummarizeResult, ApiError> {
        let Some(_caller_permit) = self
            .summarize_gate
            .try_acquire(caller_id, "channel.summarize")
        else {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "a channel summarize run for this caller is already in progress",
            ));
        };
        let Some(_slot) =
            InFlightSlot::try_acquire(&self.in_flight_runs, self.config.limits.max_concurrent_runs)
        else {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "too many channel watcher runs are in flight",
            ));
        };
        summarize_channel_impl(
            &self.office,
            self.sessions.as_deref(),
            &self.store,
            &*self.runtime_provider,
            &self.config,
            caller_id,
            params,
        )
    }
}

/// Spawns the background channel watcher tick loop with 50ms interruptible slices for prompt
/// shutdown (`ADR-0024` d9).
pub fn spawn_watcher_loop(
    watcher: Arc<ChannelWatcher>,
    stopping: Arc<AtomicBool>,
    interval: Duration,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("channel-watcher".into())
        .spawn(move || {
            let slice = Duration::from_millis(50);
            let mut elapsed = Duration::ZERO;
            while !stopping.load(Ordering::SeqCst) {
                if elapsed >= interval {
                    let _ = watcher.process_due_channels();
                    elapsed = Duration::ZERO;
                }
                std::thread::sleep(slice);
                elapsed += slice;
            }
        })
        .expect("spawn channel-watcher thread")
}

// ---- Focused free helpers ----

struct UsageContext<'a> {
    consumer_kind: &'a str,
    employee_id: Option<&'a str>,
    project_id: Option<&'a str>,
    conversation_id: &'a str,
    feature: &'a str,
    model_profile_id: Option<&'a str>,
    question_ids: Vec<String>,
    outcome: &'a str,
    usage: Option<Usage>,
    duration: Duration,
}

struct PassSnapshot {
    open_watches: Vec<WatchEntry>,
    project_id: Option<String>,
    new_batch: Vec<MessageRecord>,
    pass_input: WatchPassInput,
}

struct WakeWork {
    newly_delivered: Vec<(String, Id)>,
    confident_pairs: BTreeSet<(String, String)>,
    skipped_secrets: usize,
}

fn validate_watch_eligibility(
    office: &Office,
    sessions: Option<&SessionManager>,
    provider: &dyn WatcherRuntimeProvider,
    asker_id: &str,
    conversation_kind: ConversationKind,
) -> Result<(), &'static str> {
    if conversation_kind != ConversationKind::Channel {
        return Err("not_a_channel");
    }
    let is_agent = office
        .load_employee(asker_id)
        .ok()
        .flatten()
        .is_some_and(|e| e.kind == EmployeeKind::Agent);
    if !is_agent {
        return Err("person_asker");
    }
    resolve_watching_runtime(office, sessions, provider).map(|_| ())
}

fn open_watch_locked(
    office: &Office,
    store: &WatcherStore,
    config: &WatcherConfig,
    asker_id: &str,
    ask_id: &str,
    conversation_id: &str,
    question_text: &str,
) -> (bool, Option<String>) {
    let now = now_ms();
    let mut watches = store.read_watches();
    watches.version = 1;
    prune_expired_watches(&mut watches.watches, config, now);

    if watches
        .watches
        .iter()
        .any(|w| w.is_open() && w.question_id == ask_id)
    {
        return (true, None);
    }
    let open_for_asker = watches
        .watches
        .iter()
        .filter(|w| w.is_open() && w.asker_id == asker_id)
        .count();
    if open_for_asker >= config.max_watches_per_agent {
        let _ = store.write_watches(&watches);
        return (false, Some("watch_limit_reached".into()));
    }

    let channel_had_open = watches
        .watches
        .iter()
        .any(|w| w.is_open() && w.conversation_id == conversation_id);
    if !channel_had_open {
        seed_channel_watermarks(office, store, conversation_id);
    }

    watches.watches.push(WatchEntry {
        watch_id: next_local_id("watch"),
        conversation_id: conversation_id.to_owned(),
        question_id: ask_id.to_owned(),
        asker_id: asker_id.to_owned(),
        question_text: question_text.to_owned(),
        additions: Vec::new(),
        opened_at_ms: now,
        judged_count: 0,
        ended_at_ms: None,
        ended_reason: None,
    });
    let _ = store.write_watches(&watches);
    (true, None)
}

fn end_watches_locked(
    store: &WatcherStore,
    pred: impl Fn(&WatchEntry) -> bool,
    reason: &str,
    record_notice: bool,
) -> usize {
    let now = now_ms();
    let mut watches = store.read_watches();
    let mut ended = Vec::new();
    for w in &mut watches.watches {
        if w.is_open() && pred(w) {
            w.ended_at_ms = Some(now);
            w.ended_reason = Some(reason.to_owned());
            ended.push(WatchEndNotice {
                employee_id: w.asker_id.clone(),
                conversation_id: w.conversation_id.clone(),
                question_id: w.question_id.clone(),
                reason: reason.to_owned(),
                recorded_at_ms: now,
            });
        }
    }
    let count = ended.len();
    if !ended.is_empty() {
        let _ = store.write_watches(&watches);
        if record_notice {
            let _ = store.append_notices(&ended);
        }
    }
    count
}

fn record_channel_message_locked(store: &WatcherStore, msg: &MessageRecord) -> bool {
    let sender_id = msg.sender_employee_id.as_ref().map(Id::as_str);
    let conv_id = msg.conversation_id.as_str();
    let channel_visible = msg.thread_root_id.is_none() || msg.also_in_channel;
    let mut watches = store.read_watches();
    let mut dirty = false;
    let mut has_other_watch = false;
    for w in &mut watches.watches {
        if !w.is_open() || w.conversation_id != conv_id {
            continue;
        }
        let is_asker_followup = sender_id == Some(w.asker_id.as_str())
            && msg.thread_root_id.as_deref() == Some(w.question_id.as_str())
            && msg.id != w.question_id;
        if is_asker_followup && w.additions.len() < 20 {
            w.additions.push(msg.body_markdown.clone());
            dirty = true;
        } else if sender_id != Some(w.asker_id.as_str()) {
            has_other_watch = true;
        }
    }
    if dirty {
        let _ = store.write_watches(&watches);
    }
    channel_visible && has_other_watch
}

fn prepare_pass_snapshot(
    office: &Office,
    store: &WatcherStore,
    config: &WatcherConfig,
    conversation_id: &str,
    force_cadence: bool,
    now: u64,
) -> Result<PassSnapshot, PassReport> {
    let mut watches_file = store.read_watches();
    watches_file.version = 1;
    if prune_expired_watches(&mut watches_file.watches, config, now) {
        let _ = store.write_watches(&watches_file);
    }
    let open_watches: Vec<WatchEntry> = watches_file
        .watches
        .iter()
        .filter(|w| w.is_open() && w.conversation_id == conversation_id)
        .take(org_copilot::watcher::MAX_WATCH_QUESTIONS_PER_PASS)
        .cloned()
        .collect();
    if open_watches.is_empty() {
        return Err(PassReport::Idle);
    }
    let mut wm_file = store.read_watermarks();
    wm_file.version = 1;
    let current_wm = wm_file
        .channels
        .get(conversation_id)
        .cloned()
        .unwrap_or_default();
    let cadence_ms = config.cadence.as_millis() as u64;
    if !force_cadence
        && current_wm.last_pass_at_ms > 0
        && now.saturating_sub(current_wm.last_pass_at_ms) < cadence_ms
    {
        return Err(PassReport::Skipped("cadence"));
    }
    let project_id = office
        .conversation_by_id(conversation_id)
        .ok()
        .flatten()
        .and_then(|c| c.project_id)
        .map(|id| id.as_str().to_owned());
    let all_msgs = office
        .messages_in_channel(conversation_id)
        .unwrap_or_default();
    let (earlier, new_batch) = split_channel_messages(&all_msgs, &current_wm, &open_watches);
    if new_batch.is_empty() {
        return Err(PassReport::Idle);
    }
    // Medium #9: short-circuit without spending a model call if every new message was sent by
    // the sole open watch's own asker.
    let has_non_self_pair = new_batch.iter().any(|m| {
        let sender = m.sender_employee_id.as_ref().map(Id::as_str);
        open_watches
            .iter()
            .any(|w| sender != Some(w.asker_id.as_str()))
    });
    if !has_non_self_pair {
        let wm = wm_file
            .channels
            .entry(conversation_id.to_owned())
            .or_default();
        for m in &new_batch {
            wm.advance(
                m.origin_office_id.as_str(),
                &m.origin_incarnation,
                m.origin_sequence,
            );
        }
        let _ = store.write_watermarks(&wm_file);
        return Err(PassReport::Idle);
    }
    let pass_input = build_watch_pass_input(&open_watches, &earlier, &new_batch);
    Ok(PassSnapshot {
        open_watches,
        project_id,
        new_batch: new_batch.into_iter().cloned().collect(),
        pass_input,
    })
}

fn finalize_pass_locked(
    office: &Office,
    store: &WatcherStore,
    config: &WatcherConfig,
    conversation_id: &str,
    snap: &PassSnapshot,
    result: Result<WatchPassResult, WatchError>,
    now: u64,
) -> (PassReport, Option<WakeWork>) {
    let mut watches_file = store.read_watches();
    watches_file.version = 1;
    let mut wm_file = store.read_watermarks();
    wm_file.version = 1;
    wm_file
        .channels
        .entry(conversation_id.to_owned())
        .or_default()
        .last_pass_at_ms = now;

    let res = match result {
        Err(err) => {
            let report = handle_failed_pass(
                store,
                config,
                conversation_id,
                &mut wm_file,
                &snap.new_batch,
                err,
            );
            return (report, None);
        }
        Ok(res) => res,
    };

    // High #2: filter matches against watches that are STILL open after the unlocked model call.
    let still_open: Vec<WatchEntry> = watches_file
        .watches
        .iter()
        .filter(|w| w.is_open() && w.conversation_id == conversation_id)
        .cloned()
        .collect();
    let (deliveries, confident_pairs) = build_deliveries(&still_open, &res.matches);
    let newly_delivered = if deliveries.is_empty() {
        Vec::new()
    } else {
        match office.deliver_answer_candidates(conversation_id, deliveries) {
            Ok(Ok(delivered)) => delivered,
            _ => {
                // Medium #7: delivery failures route through handle_failed_pass so cadence and
                // the 3-failure retry cap apply cleanly.
                let report = handle_failed_pass(
                    store,
                    config,
                    conversation_id,
                    &mut wm_file,
                    &snap.new_batch,
                    WatchError::RuntimeFailed,
                );
                return (report, None);
            }
        }
    };

    advance_pass_state(
        store,
        config,
        conversation_id,
        &snap.open_watches,
        &snap.new_batch,
        &mut watches_file,
        &mut wm_file,
        now,
    );
    (
        PassReport::Idle,
        Some(WakeWork {
            newly_delivered,
            confident_pairs,
            skipped_secrets: res.skipped_secret_message_ids.len(),
        }),
    )
}

fn handle_failed_pass(
    store: &WatcherStore,
    config: &WatcherConfig,
    conversation_id: &str,
    wm_file: &mut WatermarksFile,
    new_batch: &[MessageRecord],
    err: WatchError,
) -> PassReport {
    let wm = wm_file
        .channels
        .entry(conversation_id.to_owned())
        .or_default();
    wm.consecutive_failures = wm.consecutive_failures.saturating_add(1);
    wm.total_failed_passes = wm.total_failed_passes.saturating_add(1);
    if wm.consecutive_failures >= config.max_consecutive_failures {
        let mut skipped_ids = Vec::with_capacity(new_batch.len());
        for m in new_batch {
            wm.advance(
                m.origin_office_id.as_str(),
                &m.origin_incarnation,
                m.origin_sequence,
            );
            skipped_ids.push(m.id.clone());
        }
        wm.failed_batch_ids.extend(skipped_ids.iter().cloned());
        wm.consecutive_failures = 0;
        let _ = store.write_watermarks(wm_file);
        PassReport::FailedAdvanced {
            skipped_message_ids: skipped_ids,
            error: err,
        }
    } else {
        let failures = wm.consecutive_failures;
        let _ = store.write_watermarks(wm_file);
        PassReport::FailedRetrying {
            consecutive_failures: failures,
            error: err,
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "pass state advancement updates store, watches, and watermarks from explicit pass results"
)]
fn advance_pass_state(
    store: &WatcherStore,
    config: &WatcherConfig,
    conversation_id: &str,
    served_watches: &[WatchEntry],
    new_batch: &[MessageRecord],
    watches_file: &mut WatchesFile,
    wm_file: &mut WatermarksFile,
    now: u64,
) {
    if let Some(wm) = wm_file.channels.get_mut(conversation_id) {
        for m in new_batch {
            wm.advance(
                m.origin_office_id.as_str(),
                &m.origin_incarnation,
                m.origin_sequence,
            );
        }
        wm.consecutive_failures = 0;
    }
    let served_qids: BTreeSet<&str> = served_watches
        .iter()
        .map(|w| w.question_id.as_str())
        .collect();
    for w in &mut watches_file.watches {
        if w.is_open() && served_qids.contains(w.question_id.as_str()) {
            w.judged_count = w.judged_count.saturating_add(new_batch.len());
            if w.judged_count >= config.max_watch_messages {
                w.ended_at_ms = Some(now);
                w.ended_reason = Some("message_limit".into());
            }
        }
    }
    let _ = store.write_watches(watches_file);
    let _ = store.write_watermarks(wm_file);
}

#[expect(
    clippy::too_many_arguments,
    reason = "confident wake delivery coordinates office, sessions, config, batch, matches, and timestamp"
)]
fn wake_confident_person_matches(
    office: &Office,
    sessions: Option<&SessionManager>,
    config: &WatcherConfig,
    conversation_id: &str,
    new_batch: &[MessageRecord],
    newly_delivered: &[(String, Id)],
    confident_pairs: &BTreeSet<(String, String)>,
    now: u64,
) -> Vec<String> {
    if !config.confident_wake_enabled {
        return Vec::new();
    }
    let Some(sessions) = sessions else {
        return Vec::new();
    };
    let msg_by_id: BTreeMap<&str, &MessageRecord> =
        new_batch.iter().map(|m| (m.id.as_str(), m)).collect();
    let mut woke = Vec::new();
    for (msg_id, asker_id) in newly_delivered {
        if !confident_pairs.contains(&(msg_id.clone(), asker_id.as_str().to_owned())) {
            continue;
        }
        let Some(msg) = msg_by_id.get(msg_id.as_str()) else {
            continue;
        };
        let Some(sender_id) = msg.sender_employee_id.as_ref() else {
            continue;
        };
        let is_person = office
            .load_employee(sender_id.as_str())
            .ok()
            .flatten()
            .is_some_and(|e| e.kind == EmployeeKind::Person)
            || sender_id.as_str() == office.owner_member_id();
        if !is_person {
            continue;
        }
        let item = PendingWakeItem {
            item_id: format!("watcher:{msg_id}:{asker_id}"),
            kind: WakeKind::WatcherAnswer,
            sender_id: sender_id.as_str().to_owned(),
            sender_name: msg.sender_display_name.clone(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: true,
            conversation_id: Some(conversation_id.to_owned()),
            source_id: msg_id.clone(),
            summary: msg.body_markdown.clone(),
            enqueued_at_ms: now,
        };
        if sessions.enqueue_wake(asker_id.as_str(), item) == WakeOutcome::Started {
            woke.push(asker_id.as_str().to_owned());
        }
    }
    woke
}

fn summarize_channel_impl(
    office: &Office,
    sessions: Option<&SessionManager>,
    store: &WatcherStore,
    provider: &dyn WatcherRuntimeProvider,
    config: &WatcherConfig,
    caller_id: &str,
    params: &ChatChannelSummarizeParams,
) -> Result<ChatChannelSummarizeResult, ApiError> {
    let question = params.question.trim();
    if question.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "question must not be empty",
        ));
    }
    let limit = params.limit.unwrap_or(200).clamp(1, 200) as usize;
    let (conv, msgs) = office
        .channel_messages_for_caller(caller_id, &params.channel, limit)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read channel"))??;

    let (profile_id, runtime) =
        resolve_watching_runtime(office, sessions, provider).map_err(|reason| {
            ApiError::new(
                ErrorCode::Unavailable,
                format!("channel summarize unavailable: {reason}"),
            )
        })?;
    let input = ChannelSummarizeInput {
        channel: conv
            .name
            .clone()
            .unwrap_or_else(|| conv.id.as_str().to_owned()),
        question: question.to_owned(),
        messages: msgs
            .iter()
            .map(|m| SummarizeMessageInput {
                id: m.id.clone(),
                author: m.sender_display_name.clone(),
                text: m.body_markdown.clone(),
                created_at: m.created_at.clone(),
            })
            .collect(),
    };
    let outcome = run_channel_summarize(runtime, config.limits.clone(), caller_id, &input);
    let usage_record_id = if outcome.model_called {
        record_usage(
            office,
            store,
            UsageContext {
                consumer_kind: "employee",
                employee_id: Some(caller_id),
                project_id: conv.project_id.as_ref().map(Id::as_str),
                conversation_id: conv.id.as_str(),
                feature: "channel.summarize",
                model_profile_id: Some(&profile_id),
                question_ids: Vec::new(),
                outcome: if outcome.result.is_ok() {
                    "ok"
                } else {
                    "failed"
                },
                usage: outcome.usage,
                duration: outcome.duration,
            },
        )
        .ok()
    } else {
        None
    };

    let res = outcome.result.map_err(|err| match err {
        WatchError::Timeout => ApiError::new(ErrorCode::Timeout, err.to_string()),
        WatchError::InputRejected => ApiError::new(ErrorCode::InvalidRequest, err.to_string()),
        _ => ApiError::new(ErrorCode::Unavailable, err.to_string()),
    })?;
    Ok(ChatChannelSummarizeResult {
        conversation_id: conv.id.as_str().to_owned(),
        summary: res.summary,
        message_ids: res.message_ids,
        derived_notice: res.derived_notice,
        usage_record_id,
    })
}

fn resolve_watching_runtime(
    office: &Office,
    sessions: Option<&SessionManager>,
    provider: &dyn WatcherRuntimeProvider,
) -> Result<(String, Arc<dyn CopilotRuntime>), &'static str> {
    if sessions.is_some_and(SessionManager::has_budget_hold) {
        return Err("budget_hold");
    }
    let Some(profile_id) = office
        .watching_model_setting()
        .ok()
        .flatten()
        .and_then(|s| s.model_profile_id)
    else {
        return Err("no_watching_model");
    };
    let Some(profile) = office
        .load_model_profile(profile_id.as_str())
        .ok()
        .flatten()
        .filter(|p| !p.archived)
    else {
        return Err("no_watching_model");
    };
    let conn = profile
        .connection_ids
        .first()
        .and_then(|cid| office.load_provider_connection(cid.as_str()).ok().flatten());
    let Some(rt) = provider.runtime_for_profile(&profile, conn.as_ref()) else {
        return Err("runtime_unavailable");
    };
    if !rt.available() {
        return Err("runtime_unavailable");
    }
    Ok((profile.id.as_str().to_owned(), rt))
}

fn seed_channel_watermarks(office: &Office, store: &WatcherStore, conversation_id: &str) {
    let msgs = office
        .messages_in_channel(conversation_id)
        .unwrap_or_default();
    let mut wm_file = store.read_watermarks();
    wm_file.version = 1;
    let wm = wm_file
        .channels
        .entry(conversation_id.to_owned())
        .or_default();
    for m in &msgs {
        wm.advance(
            m.origin_office_id.as_str(),
            &m.origin_incarnation,
            m.origin_sequence,
        );
    }
    let _ = store.write_watermarks(&wm_file);
}

fn prune_expired_watches(watches: &mut [WatchEntry], config: &WatcherConfig, now: u64) -> bool {
    let max_age_ms = config.max_watch_age.as_millis() as u64;
    let mut changed = false;
    for w in watches.iter_mut() {
        if !w.is_open() {
            continue;
        }
        if now.saturating_sub(w.opened_at_ms) >= max_age_ms {
            w.ended_at_ms = Some(now);
            w.ended_reason = Some("time_limit".into());
            changed = true;
        } else if w.judged_count >= config.max_watch_messages {
            w.ended_at_ms = Some(now);
            w.ended_reason = Some("message_limit".into());
            changed = true;
        }
    }
    changed
}

fn record_usage(
    office: &Office,
    store: &WatcherStore,
    ctx: UsageContext<'_>,
) -> std::io::Result<String> {
    let id = next_local_id("usage");
    let entry = UsageLedgerEntry {
        id: id.clone(),
        consumer_kind: ctx.consumer_kind.to_owned(),
        employee_id: ctx.employee_id.map(str::to_owned),
        organization_id: office.info().org_id.clone(),
        project_id: ctx.project_id.map(str::to_owned),
        conversation_id: ctx.conversation_id.to_owned(),
        feature: ctx.feature.to_owned(),
        model_profile_id: ctx.model_profile_id.map(str::to_owned),
        question_ids: ctx.question_ids,
        outcome: ctx.outcome.to_owned(),
        input_tokens: ctx.usage.and_then(|u| u.input_tokens),
        output_tokens: ctx.usage.and_then(|u| u.output_tokens),
        duration_ms: ctx.duration.as_millis() as u64,
        recorded_at: org_office::format_rfc3339_ms(now_ms()),
    };
    store.append_usage(&entry)?;
    let _ = office.record_office_usage(org_proto::costs::CostUsageRecordParams {
        id: None,
        producer_id: format!("watcher-{}", office.info().office_id),
        source_event_id: id.clone(),
        run_id: None,
        actor_kind: if entry.employee_id.is_some() {
            org_proto::costs::UsageActorKind::Agent
        } else {
            org_proto::costs::UsageActorKind::OneOff
        },
        employee_id: entry.employee_id.clone(),
        runtime: entry.feature.clone(),
        provider: None,
        model: entry.model_profile_id.clone(),
        occurred_at: entry.recorded_at.clone(),
        source_metadata_json: Some(
            serde_json::json!({
                "conversation_id": entry.conversation_id,
                "outcome": entry.outcome,
                "duration_ms": entry.duration_ms,
            })
            .to_string(),
        ),
        input_tokens: entry.input_tokens,
        output_tokens: entry.output_tokens,
        cache_read_tokens: (entry.input_tokens.is_some() || entry.output_tokens.is_some())
            .then_some(0),
        cache_write_tokens: (entry.input_tokens.is_some() || entry.output_tokens.is_some())
            .then_some(0),
        reasoning_tokens: (entry.input_tokens.is_some() || entry.output_tokens.is_some())
            .then_some(0),
        token_issue: None,
        runtime_cost_micro_usd: None,
        is_subscription: false,
        scope_kind: if entry.project_id.is_some() {
            org_proto::costs::UsageScopeKind::Project
        } else {
            org_proto::costs::UsageScopeKind::Organization
        },
        task_id: None,
        project_id: entry.project_id.clone(),
        team_id: None,
        feature_id: None,
        milestone_id: None,
        release_id: None,
        task_type_snapshot: None,
        work_type: None,
        attribution_basis: if entry.project_id.is_some() {
            org_proto::costs::AttributionBasis::ExplicitContext
        } else {
            org_proto::costs::AttributionBasis::Unattributed
        },
    });
    Ok(id)
}

/// Splits channel messages into already-judged context (<=20) and an unjudged batch (<=50)
/// preserving strictly contiguous per-`(origin_office_id, origin_incarnation)` `origin_sequence`
/// prefixes so advancing watermarks after truncation never skips an earlier sequence (`Med #9`).
fn split_channel_messages<'a>(
    all_msgs: &'a [MessageRecord],
    wm: &ChannelWatermark,
    open_watches: &[WatchEntry],
) -> (Vec<&'a MessageRecord>, Vec<&'a MessageRecord>) {
    let open_qids: BTreeSet<&str> = open_watches
        .iter()
        .map(|w| w.question_id.as_str())
        .collect();
    let mut earlier = Vec::new();
    let mut by_origin: BTreeMap<(String, String), Vec<&'a MessageRecord>> = BTreeMap::new();
    for m in all_msgs {
        if m.thread_root_id.is_some() && !m.also_in_channel {
            continue;
        }
        let last = wm.last_seq(m.origin_office_id.as_str(), &m.origin_incarnation);
        if m.origin_sequence <= last {
            earlier.push(m);
        } else if !open_qids.contains(m.id.as_str()) {
            by_origin
                .entry((
                    m.origin_office_id.as_str().to_owned(),
                    m.origin_incarnation.clone(),
                ))
                .or_default()
                .push(m);
        }
    }
    earlier.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    if earlier.len() > org_copilot::watcher::MAX_WATCH_CONTEXT_MESSAGES {
        earlier =
            earlier.split_off(earlier.len() - org_copilot::watcher::MAX_WATCH_CONTEXT_MESSAGES);
    }

    // Sort each origin strictly by origin_sequence ascending, then merge heads up to 50.
    let mut queues: Vec<std::collections::VecDeque<&'a MessageRecord>> = by_origin
        .into_values()
        .map(|mut list| {
            list.sort_by_key(|m| (m.origin_sequence, m.id.as_str()));
            list.into()
        })
        .collect();
    let mut unjudged = Vec::new();
    while unjudged.len() < org_copilot::watcher::MAX_WATCH_NEW_MESSAGES_PER_PASS {
        let best_idx = queues
            .iter()
            .enumerate()
            .filter_map(|(idx, q)| q.front().map(|m| (idx, m)))
            .min_by(|(_, a), (_, b)| {
                a.created_at
                    .cmp(&b.created_at)
                    .then_with(|| a.origin_office_id.cmp(&b.origin_office_id))
                    .then_with(|| a.origin_sequence.cmp(&b.origin_sequence))
                    .then_with(|| a.id.cmp(&b.id))
            })
            .map(|(idx, _)| idx);
        let Some(idx) = best_idx else { break };
        if let Some(m) = queues[idx].pop_front() {
            unjudged.push(m);
        }
    }
    (earlier, unjudged)
}

fn build_watch_pass_input(
    open_watches: &[WatchEntry],
    earlier: &[&MessageRecord],
    new_batch: &[&MessageRecord],
) -> WatchPassInput {
    WatchPassInput {
        open_questions: open_watches
            .iter()
            .map(|w| WatchQuestionInput {
                id: w.question_id.clone(),
                asker: w.asker_id.clone(),
                text: w.question_text.clone(),
                additions: w.additions.clone(),
            })
            .collect(),
        new_messages: new_batch
            .iter()
            .map(|m| WatchMessageInput {
                id: m.id.clone(),
                author: m
                    .sender_employee_id
                    .as_ref()
                    .map(|id| id.as_str().to_owned())
                    .unwrap_or_else(|| "office".to_owned()),
                text: m.body_markdown.clone(),
            })
            .collect(),
        earlier_messages: earlier
            .iter()
            .map(|m| WatchMessageInput {
                id: m.id.clone(),
                author: m
                    .sender_employee_id
                    .as_ref()
                    .map(|id| id.as_str().to_owned())
                    .unwrap_or_else(|| "office".to_owned()),
                text: m.body_markdown.clone(),
            })
            .collect(),
    }
}

fn build_deliveries(
    open_watches: &[WatchEntry],
    matches: &[WatchMatch],
) -> (Vec<AnswerCandidateDelivery>, BTreeSet<(String, String)>) {
    let watch_by_qid: BTreeMap<&str, &WatchEntry> = open_watches
        .iter()
        .map(|w| (w.question_id.as_str(), w))
        .collect();
    let mut grouped: BTreeMap<(String, String), (Vec<String>, Vec<String>)> = BTreeMap::new();
    let mut confident_pairs = BTreeSet::new();
    for m in matches {
        let Some(w) = watch_by_qid.get(m.question_id.as_str()) else {
            continue;
        };
        let key = (m.message_id.clone(), w.asker_id.clone());
        let entry = grouped.entry(key.clone()).or_default();
        if !entry.0.contains(&w.question_id) {
            entry.0.push(w.question_id.clone());
            entry.1.push(w.question_text.clone());
        }
        if m.confident {
            confident_pairs.insert(key);
        }
    }
    let deliveries = grouped
        .into_iter()
        .filter_map(|((message_id, asker_raw), (question_ids, texts))| {
            let asker_id = Id::parse(&asker_raw).ok()?;
            let context = format!("Re: {}", texts.join(" | "));
            Some(AnswerCandidateDelivery {
                message_id,
                asker_id,
                question_ids,
                question_context: context,
            })
        })
        .collect();
    (deliveries, confident_pairs)
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
#[path = "watcher_tests.rs"]
mod tests;
