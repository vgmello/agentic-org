//! The satellite's persistent sync loop: reconnect backoff, bootstrap, outbox flushing, keepalive,
//! and activity waits.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use crate::federation_link::{InboundDeliveryNotifier, SatelliteSyncSession, WakeRelayHandler};
use crate::org_feed::FeedWake;
use org_office::Office;
use org_sync_proto::SyncFrame;

use super::satellite_rpc::SatRpcContext;
use super::{
    SatSyncWake, IDLE_POLL_SLICE, KEEPALIVE_IDLE, KEEPALIVE_TIMEOUT, RECONNECT_MAX, RECONNECT_MIN,
};

/// Drains every buffered and socket-ready inbound frame of a persistent session, answering
/// relays, org events and session RPCs; the count handled, or why the session must end.
/// `federation_runtime` injects `federation::drain_satellite_inbound_persistent`, so this
/// stream never imports the federation module that drives it.
pub(crate) type DrainInbound = fn(
    &mut SatelliteSyncSession,
    Option<&WakeRelayHandler>,
    Option<&InboundDeliveryNotifier>,
    Option<&FeedWake>,
    &SatRpcContext<'_>,
) -> Result<usize, String>;

/// Runtime hooks passed into the satellite's persistent bidirectional mTLS loop.
pub(crate) struct PersistentSyncHooks {
    pub(crate) wake_handler: WakeRelayHandler,
    pub(crate) delivery_notifier: InboundDeliveryNotifier,
    pub(crate) feed_wake: Option<Arc<FeedWake>>,
    pub(crate) sessions: Weak<org_office_sessions::SessionManager>,
    pub(crate) drain_inbound: DrainInbound,
}

/// Runs the satellite's persistent bidirectional mTLS loop with instant local outbox flush,
/// instant HQ org/relay push handling, keepalive ping/pong, and exponential backoff reconnect.
pub(crate) fn run_persistent_satellite_sync(
    office: &Arc<Office>,
    stop: &Arc<AtomicBool>,
    sat_wake: &Arc<SatSyncWake>,
    hooks: &PersistentSyncHooks,
) {
    let mut backoff = RECONNECT_MIN;
    // One worker budget for every connection this loop makes: an RPC worker of a dropped
    // session keeps its slot until it finishes, so rapid reconnects can never run more than
    // `MAX_SATELLITE_RPC_WORKERS` at once (`TASK-174` AC #2).
    let rpc_workers = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::SeqCst) {
        let mut session =
            match SatelliteSyncSession::connect_cancellable(Arc::clone(office), Some(stop)) {
                Ok(s) => {
                    backoff = RECONNECT_MIN;
                    s
                }
                Err(_) => {
                    let seen = sat_wake.generation();
                    let _ = sat_wake.wait_or_stop(seen, backoff, stop);
                    backoff = (backoff * 2).min(RECONNECT_MAX);
                    continue;
                }
            };
        if run_connected_session(&mut session, office, stop, sat_wake, hooks, &rpc_workers).is_err()
        {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let seen = sat_wake.generation();
            let _ = sat_wake.wait_or_stop(seen, backoff, stop);
            backoff = (backoff * 2).min(RECONNECT_MAX);
        }
    }
}

struct PersistentLoopState {
    last_wake_gen: u64,
    buffered_flush_until: Option<Instant>,
    last_rx: Instant,
    ping_sent_at: Option<Instant>,
    ping_nonce: u64,
}

fn bootstrap_connected_session(
    session: &mut SatelliteSyncSession,
    office: &Arc<Office>,
    hooks: &PersistentSyncHooks,
) -> Result<(), String> {
    if office
        .sync_status()
        .is_ok_and(|s| s.initial_snapshot_required)
    {
        let op_id = office.next_uuid_v7();
        session.sync_initial_snapshot(&op_id)?;
        if let Some(wake) = &hooks.feed_wake {
            wake.notify();
        }
        if let Some(sm) = hooks.sessions.upgrade() {
            let _ = sm.recover_on_startup();
        }
    }
    let applied = session.pull_org_events()?;
    if applied > 0 {
        if let Some(wake) = &hooks.feed_wake {
            wake.notify();
        }
    }
    Ok(())
}

fn flush_satellite_outboxes(
    session: &mut SatelliteSyncSession,
    office: &Office,
    sat_wake: &SatSyncWake,
    state: &mut PersistentLoopState,
) -> Result<(), String> {
    let cur_gen = sat_wake.generation();
    let wake_fired = cur_gen != state.last_wake_gen;
    if wake_fired {
        state.last_wake_gen = cur_gen;
    }
    if wake_fired || office.has_pending_journal_pipeline() {
        // A local write is waiting on the journal: confirm it now rather than after the
        // store's group-flush interval (`TASK-140`); this never blocks the loop.
        office.request_store_flush();
    }
    let flushed = session.flush_work_outbox()? + session.flush_message_outbox()?;
    if flushed > 0 {
        state.buffered_flush_until = None;
        state.last_rx = Instant::now();
        state.ping_sent_at = None;
    } else if wake_fired || office.has_pending_journal_pipeline() {
        // Buffered `org-store` writes (e.g. `msg.reply`) return before the background
        // flusher confirms/materializes the entry a few milliseconds later; poll briefly at
        // 5 ms until the write clears the pipeline so RTT stays < 10 ms with 0% idle CPU.
        state.buffered_flush_until = Some(Instant::now() + Duration::from_millis(80));
    } else if state
        .buffered_flush_until
        .is_some_and(|until| Instant::now() >= until)
    {
        state.buffered_flush_until = None;
    }
    Ok(())
}

fn check_keepalive(
    session: &mut SatelliteSyncSession,
    state: &mut PersistentLoopState,
) -> Result<(), String> {
    if let Some(sent_at) = state.ping_sent_at {
        if sent_at.elapsed() >= KEEPALIVE_TIMEOUT {
            return Err("HQ keepalive ping timed out".into());
        }
    } else if state.last_rx.elapsed() >= KEEPALIVE_IDLE {
        state.ping_nonce = state.ping_nonce.wrapping_add(1);
        session.write_frame(&SyncFrame::Ping {
            nonce: state.ping_nonce,
        })?;
        state.ping_sent_at = Some(Instant::now());
    }
    Ok(())
}

fn has_immediate_satellite_work(
    sat_wake: &SatSyncWake,
    last_wake_gen: u64,
    replies: &Mutex<VecDeque<SyncFrame>>,
) -> bool {
    sat_wake.generation() != last_wake_gen
        || !replies.lock().unwrap_or_else(|p| p.into_inner()).is_empty()
}

fn wait_for_satellite_activity(
    session: &SatelliteSyncSession,
    office: &Office,
    stop: &AtomicBool,
    sat_wake: &SatSyncWake,
    state: &PersistentLoopState,
) -> Result<(), String> {
    let wait = if state.buffered_flush_until.is_some() || office.has_pending_journal_pipeline() {
        Duration::from_millis(5)
    } else {
        IDLE_POLL_SLICE
    };
    if let Some(wake_fd) = sat_wake.read_fd() {
        match org_socket::poll_readable_either(session.raw_fd(), wake_fd, wait) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(()),
            Err(e) => Err(format!("poll failed: {e}")),
        }
    } else {
        let _ = sat_wake.wait_or_stop(state.last_wake_gen, Duration::from_millis(50), stop);
        Ok(())
    }
}

fn run_connected_session(
    session: &mut SatelliteSyncSession,
    office: &Arc<Office>,
    stop: &Arc<AtomicBool>,
    sat_wake: &Arc<SatSyncWake>,
    hooks: &PersistentSyncHooks,
    rpc_workers: &Arc<AtomicUsize>,
) -> Result<(), String> {
    bootstrap_connected_session(session, office, hooks)?;
    let replies: Arc<Mutex<VecDeque<SyncFrame>>> = Arc::default();
    let mut state = PersistentLoopState {
        last_wake_gen: sat_wake.generation().wrapping_sub(1),
        buffered_flush_until: None,
        last_rx: Instant::now(),
        ping_sent_at: None,
        ping_nonce: 1,
    };

    while !stop.load(Ordering::SeqCst) {
        flush_satellite_outboxes(session, office, sat_wake, &mut state)?;
        flush_rpc_replies(session, &replies)?;
        let rpc_ctx = SatRpcContext {
            sessions: &hooks.sessions,
            replies: &replies,
            sat_wake,
            active_workers: rpc_workers,
        };
        let drained = (hooks.drain_inbound)(
            session,
            Some(&hooks.wake_handler),
            Some(&hooks.delivery_notifier),
            hooks.feed_wake.as_deref(),
            &rpc_ctx,
        )?;
        if drained > 0 {
            state.last_rx = Instant::now();
            state.ping_sent_at = None;
        }
        flush_rpc_replies(session, &replies)?;
        if has_immediate_satellite_work(sat_wake, state.last_wake_gen, &replies) {
            continue;
        }
        check_keepalive(session, &mut state)?;
        sat_wake.drain_pipe();
        if has_immediate_satellite_work(sat_wake, state.last_wake_gen, &replies) {
            continue;
        }
        wait_for_satellite_activity(session, office, stop, sat_wake, &state)?;
    }
    Ok(())
}

fn flush_rpc_replies(
    session: &mut SatelliteSyncSession,
    replies: &Arc<Mutex<VecDeque<SyncFrame>>>,
) -> Result<(), String> {
    loop {
        let next = replies
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pop_front();
        let Some(frame) = next else {
            return Ok(());
        };
        session.write_frame(&frame)?;
    }
}
