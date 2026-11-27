//! Live turn controls (`TASK-103`): a person's explicit interrupt of the running turn and their
//! mid-turn steer. Both keep the warm process alive: an interrupt stops only the turn
//! ([`Control::Interrupt`], never [`Control::Close`]), and a turn a person interrupted never
//! fails over to another provider connection ([`super::events::handle_turn_ended`]).
//!
//! A steer reaches the running turn the way the runtime allows
//! ([`org_agent_runtime::Capabilities::steer_route`]): natively as a fenced mid-turn notice
//! ([`Control::DeliverMidTurn`], e.g. Codex `turn/steer`), or, when the runtime needs it
//! ([`ControlError::NeedsInterrupt`], e.g. Claude), by interrupting the turn and running the
//! steer as the next turn, ahead of every queued turn. A steer the runtime did not take
//! (refused, or unconfirmed when the turn ended) runs as the next turn too: at least once,
//! never lost. Ordinary `chat.send` / `msg.send` turns are never affected: they queue as before.
//!
//! A steer is counted in [`super::SessionRuntimeState::queued_turns`] from the moment it is
//! queued ([`super::queue_steer`]) until it is delivered into the turn or dispatched as one; the pump
//! queues and folds it with [`PumpLoopState`]'s steer queue.

use std::time::Instant;

use org_agent_runtime::{Control, ControlError};

use super::activity::mark_interrupting;
use super::events::apply_outputs;
use super::{PumpContext, PumpLoopState, SessionRuntimeState};

/// A person's steer as the pump receives it: one direction, composed for both ways it can
/// reach the agent.
#[derive(Clone, Debug)]
pub(crate) struct SteerRequest {
    /// Correlates the runtime's delivery confirmation or refusal.
    pub delivery_id: String,
    /// The fenced mid-turn notice delivered natively into the running turn.
    pub notice: String,
    /// The fenced objective it runs as when it becomes a turn of its own.
    pub turn: String,
}

impl SteerRequest {
    /// Folds a later steer into this one (both still to be delivered).
    fn absorb(&mut self, later: SteerRequest) {
        self.notice.push_str("\n\n");
        self.notice.push_str(&later.notice);
        self.turn.push_str("\n\n");
        self.turn.push_str(&later.turn);
    }
}

/// A person asked to interrupt the running turn. With no turn running there is nothing to do.
pub(super) fn request_interrupt(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    if !loop_state.turn_active {
        return;
    }
    loop_state.interrupt_wanted = true;
    try_interrupt(stdin, ctx, loop_state, session);
}

/// Sends the wanted interrupt once; a runtime not ready for it (`NotReady`) is asked again on
/// the next pass, and one that refuses it otherwise leaves the turn running.
fn try_interrupt(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    if !loop_state.interrupt_wanted || loop_state.interrupt_sent {
        return;
    }
    if !loop_state.turn_active || loop_state.stop_requested {
        loop_state.interrupt_wanted = false;
        return;
    }
    match session.control(Control::Interrupt, Instant::now()) {
        Ok(outs) => {
            loop_state.interrupt_sent = true;
            mark_interrupting(ctx);
            apply_outputs(outs, stdin, ctx, loop_state, session);
        }
        Err(ControlError::NotReady) => {}
        Err(_) => loop_state.interrupt_wanted = false,
    }
}

/// Retries what had to wait for the runtime: a wanted interrupt, and a native steer it was not
/// ready for. Runs on every pump pass, before the next turn is dispatched.
pub(super) fn drive_turn_controls(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    try_interrupt(stdin, ctx, loop_state, session);
    if let Some(waiting) = loop_state.pending_steer.take() {
        steer(stdin, ctx, loop_state, session, waiting);
    }
}

/// Routes a person's steer (already counted as one queued turn).
pub(super) fn steer(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
    request: SteerRequest,
) {
    if !loop_state.ready || !loop_state.turn_active || loop_state.interrupt_wanted {
        // No turn to steer, or it is already being interrupted: the steer runs next.
        loop_state.queue_steer_turn(&mut lock_state(ctx), request.turn);
        return;
    }
    let caps = ctx.runtime.capabilities();
    let routed = caps.steer_route().and_then(|()| {
        session.control(
            Control::DeliverMidTurn {
                delivery_id: request.delivery_id.clone(),
                text: request.notice.clone(),
                images: Vec::new(),
            },
            Instant::now(),
        )
    });
    match routed {
        Ok(outs) => {
            lock_state(ctx).dequeue_turns(1);
            // Recorded before the outputs are applied: a runtime may confirm at once.
            loop_state
                .in_flight_steers
                .push((request.delivery_id, request.turn));
            apply_outputs(outs, stdin, ctx, loop_state, session);
        }
        Err(ControlError::NotReady) => match loop_state.pending_steer.as_mut() {
            Some(waiting) => {
                waiting.absorb(request);
                lock_state(ctx).dequeue_turns(1);
            }
            None => loop_state.pending_steer = Some(request),
        },
        Err(ControlError::NeedsInterrupt | ControlError::Unsupported) if caps.interrupt_in_band => {
            loop_state.queue_steer_turn(&mut lock_state(ctx), request.turn);
            request_interrupt(stdin, ctx, loop_state, session);
        }
        Err(_) => loop_state.queue_steer_turn(&mut lock_state(ctx), request.turn),
    }
}

fn lock_state(ctx: &PumpContext) -> std::sync::MutexGuard<'_, SessionRuntimeState> {
    ctx.state.lock().unwrap_or_else(|p| p.into_inner())
}
