//! Watch bookkeeping under the store lock: eligibility, opening and ending watches, recording
//! channel messages, watermarks, expiry, and resolving the watching runtime.

use std::sync::Arc;

use crate::watcher_store::{WatchEndNotice, WatchEntry, WatcherStore};
use org_copilot::CopilotRuntime;
use org_office::messaging::records::MessageRecord;
use org_office::organization_family::records::EmployeeKind;
use org_office::Office;
use org_office_sessions::SessionManager;
use org_proto::chat::ConversationKind;
use org_store::Id;

use super::{next_local_id, now_ms, WatcherConfig, WatcherRuntimeProvider};

pub(super) fn validate_watch_eligibility(
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

pub(super) fn open_watch_locked(
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

pub(super) fn end_watches_locked(
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

pub(super) fn record_channel_message_locked(store: &WatcherStore, msg: &MessageRecord) -> bool {
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

pub(super) fn resolve_watching_runtime(
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

pub(super) fn prune_expired_watches(
    watches: &mut [WatchEntry],
    config: &WatcherConfig,
    now: u64,
) -> bool {
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
