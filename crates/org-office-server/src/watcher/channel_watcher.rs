//! The channel watcher: in-flight pass guards, the [`ChannelWatcher`] event hooks and pass driver,
//! and the background watcher loop.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::watcher_store::WatcherStore;
use org_copilot::watcher::run_watch_pass;
use org_copilot::RunGate;
use org_office::messaging::records::MessageRecord;
use org_office::Office;
use org_office_sessions::SessionManager;
use org_proto::chat::{
    ChatChannelSummarizeParams, ChatChannelSummarizeResult, ChatChannelsParams, ConversationKind,
};
use org_proto::{ApiError, ErrorCode};

use super::pass::{finalize_pass_locked, prepare_pass_snapshot, wake_confident_person_matches};
use super::runtime::CliWatcherRuntimeProvider;
use super::summarize::{record_usage, summarize_channel_impl, UsageContext};
use super::watches::{
    end_watches_locked, open_watch_locked, record_channel_message_locked, resolve_watching_runtime,
    validate_watch_eligibility,
};
use super::{now_ms, PassReport, WatcherConfig, WatcherRuntimeProvider};

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
        let runtimes = sessions
            .as_ref()
            .map(|s| s.runtimes().clone())
            .unwrap_or_else(org_office_sessions::RuntimeRegistry::production);
        Self::new(
            office,
            sessions,
            Arc::new(CliWatcherRuntimeProvider::new(runtimes)),
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
            .list_channels(employee_id, ChatChannelsParams::default())
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
