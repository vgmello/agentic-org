//! Coarse live activity of the running turn (`TASK-103`): derived from **redacted** session
//! events, kept on [`super::SessionRuntimeState::activity`] for session views, and published to
//! the run's [`crate::stream_hub::StreamPublisher`] only when it changes (coalesced: a stream of
//! text deltas is one `Responding`). Only a tool's coarse name ever rides along: never its
//! input, output, a diff or thinking text.

use org_agent_runtime::{SessionEvent, TurnOutcome};
use org_proto::sessions::ActivityPhase;

use crate::credentials::rfc3339_now;
use crate::stream_hub::{coarse_tool_name, SessionActivity, SessionStreamEvent, TurnEnd};

use super::PumpContext;

/// The activity `event` leads to from `current`, or `None` when it changes nothing. A turn a
/// person interrupted stays [`ActivityPhase::Interrupting`] until it ends.
pub(super) fn next_activity(
    current: &SessionActivity,
    event: &SessionEvent,
    now: &str,
) -> Option<SessionActivity> {
    let next = match event {
        SessionEvent::TurnEnded { .. }
        | SessionEvent::Ended { .. }
        | SessionEvent::StartFailed { .. } => current.idle(),
        _ if current.phase == Some(ActivityPhase::Interrupting) => return None,
        SessionEvent::TurnStarted { .. } | SessionEvent::Thinking => {
            current.in_phase(ActivityPhase::Thinking)
        }
        SessionEvent::TextDelta { .. } | SessionEvent::AssistantText { .. } => {
            current.in_phase(ActivityPhase::Responding)
        }
        SessionEvent::ToolStarted { name, .. } => SessionActivity {
            phase: Some(ActivityPhase::RunningTool),
            active_tool_name: coarse_tool_name(name),
            active_tool_started_at: Some(now.to_owned()),
            supports_steer: current.supports_steer,
        },
        SessionEvent::ToolFinished { .. } if current.phase == Some(ActivityPhase::RunningTool) => {
            current.in_phase(ActivityPhase::Thinking)
        }
        _ => return None,
    };
    (next != *current).then_some(next)
}

/// Records the activity `event` leads to and publishes what changed, plus the turn's streaming
/// text and its end. `interrupted` is whether a person interrupted the running turn, so its end
/// reads as interrupted whatever the runtime reported.
pub(super) fn track_activity(ctx: &PumpContext, event: &SessionEvent, interrupted: bool) {
    let now = rfc3339_now();
    let changed = {
        let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        let next = next_activity(&state.activity, event, &now);
        if let Some(next) = next.as_ref() {
            state.activity = next.clone();
        }
        next
    };
    let Some(stream) = ctx.stream.as_ref() else {
        return;
    };
    if let Some(activity) = changed {
        stream.publish(now.clone(), SessionStreamEvent::Activity(activity));
    }
    match event {
        SessionEvent::TextDelta { text } => {
            stream.publish(now, SessionStreamEvent::Delta { text: text.clone() });
        }
        SessionEvent::TurnEnded { outcome, .. } => {
            let outcome = match outcome {
                _ if interrupted => TurnEnd::Interrupted,
                TurnOutcome::Completed => TurnEnd::Completed,
                TurnOutcome::Interrupted => TurnEnd::Interrupted,
                _ => TurnEnd::Failed,
            };
            stream.publish(now, SessionStreamEvent::TurnEnded { outcome });
        }
        _ => {}
    }
}

/// A person's interrupt went to the runtime: the turn is winding down.
pub(super) fn mark_interrupting(ctx: &PumpContext) {
    set_activity(ctx, |current| current.in_phase(ActivityPhase::Interrupting));
}

/// The run is over (or never had a turn to end): nothing is running any more.
pub(super) fn settle_activity_idle(ctx: &PumpContext) {
    set_activity(ctx, SessionActivity::idle);
}

fn set_activity(ctx: &PumpContext, to: impl FnOnce(&SessionActivity) -> SessionActivity) {
    let changed = {
        let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        let next = to(&state.activity);
        (next != state.activity).then(|| {
            state.activity = next.clone();
            next
        })
    };
    if let (Some(activity), Some(stream)) = (changed, ctx.stream.as_ref()) {
        stream.publish(rfc3339_now(), SessionStreamEvent::Activity(activity));
    }
}
