//! One watch pass: the snapshot taken under the lock, message splitting and pass input, wake
//! deliveries, and finalizing, failure and state advance.

use std::collections::{BTreeMap, BTreeSet};

use crate::watcher_store::{
    ChannelWatermark, WatchEntry, WatcherStore, WatchesFile, WatermarksFile,
};
use org_copilot::watcher::{
    WatchError, WatchMatch, WatchMessageInput, WatchPassInput, WatchPassResult, WatchQuestionInput,
};
use org_office::messaging::features::AnswerCandidateDelivery;
use org_office::messaging::records::MessageRecord;
use org_office::organization_family::records::EmployeeKind;
use org_office::Office;
use org_office_sessions::{PendingWakeItem, SessionManager, WakeKind, WakeOutcome};
use org_store::Id;

use super::watches::prune_expired_watches;
use super::{PassReport, WatcherConfig};

pub(super) struct PassSnapshot {
    pub(super) open_watches: Vec<WatchEntry>,
    pub(super) project_id: Option<String>,
    pub(super) new_batch: Vec<MessageRecord>,
    pub(super) pass_input: WatchPassInput,
}

pub(super) struct WakeWork {
    pub(super) newly_delivered: Vec<(String, Id)>,
    pub(super) confident_pairs: BTreeSet<(String, String)>,
    pub(super) skipped_secrets: usize,
}

pub(super) fn prepare_pass_snapshot(
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

pub(super) fn finalize_pass_locked(
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
pub(super) fn wake_confident_person_matches(
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
            attachment_ids: msg.attachments.iter().map(|a| a.id.clone()).collect(),
            enqueued_at_ms: now,
        };
        if sessions.enqueue_wake(asker_id.as_str(), item) == WakeOutcome::Started {
            woke.push(asker_id.as_str().to_owned());
        }
    }
    woke
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
