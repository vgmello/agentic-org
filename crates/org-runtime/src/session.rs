//! The sans-I/O [`Session`]: one runtime protocol as a pure state machine.

use std::fmt;
use std::time::Instant;

use crate::{ApprovalId, ExitInfo, Line, SessionEvent};

/// One live session's protocol, as a pure state machine. **It does no I/O**: it never
/// spawns, reads, writes, sleeps, or reads the clock.
///
/// A pump (in `org-office-sessions`) owns the process and drives it:
/// 1. Right after the spawn, it calls [`start`](Self::start).
/// 2. It splits stdout with [`crate::LineFramer`] and passes each frame in:
///    [`on_line`](Self::on_line) for a line, [`on_truncated`](Self::on_truncated) for one
///    the framer dropped.
/// 3. It splits stderr with a second [`crate::LineFramer`] and passes each line to
///    [`on_stderr_line`](Self::on_stderr_line) (it drops over-long stderr lines).
/// 4. When stdout ends, it calls [`on_stdout_closed`](Self::on_stdout_closed) once (after
///    feeding [`crate::LineFramer::finish`]'s frame, if any).
/// 5. After it reaps the process, it calls [`on_exit`](Self::on_exit) once, last. Before
///    that, it **drains stdout**: it keeps reading until EOF or a short bound (a second or
///    so) after the exit, so the session sees the runtime's last lines (a turn's usage and
///    result) before the exit. `on_exit` can come without step 4: a tool that left the
///    process group can hold stdout open after the runtime is gone.
/// 6. It calls [`control`](Self::control) for each office command.
/// 7. It calls [`on_tick`](Self::on_tick) once `now` reaches
///    [`next_deadline`](Self::next_deadline), re-reading the deadline after every call.
///
/// When the office closes the session, the supervisor's [`crate::StopPlan`] starts only
/// after the session's [`Output::CloseStdin`] (or a timeout waiting for it). Its graceful
/// steps keep their full grace, and the first signal the supervisor sends is always `SIGTERM`
/// to the runtime's own process.
///
/// Every call returns [`Output`]s, which the pump applies **in order** before the next call:
/// it writes each [`Output::Write`] to stdin in full, forwards each [`Output::Event`], and
/// closes stdin on [`Output::CloseStdin`] (writes after that are dropped).
///
/// `now` is the pump's `Instant::now()` at the call. It must never go backwards between
/// calls. Calling `on_tick` early, or more than once, is harmless.
///
/// # Contract rules
/// Every implementation must keep these; the office relies on them.
/// 1. **Normal input waits for the turn.** A [`Control::Send`] with `urgent: false` while a
///    turn is running returns [`ControlError::Busy`], and the office queues it.
/// 2. **An error changes nothing.** When [`control`](Self::control) returns `Err`, nothing was
///    emitted (no write, no event) and the session's state is exactly as before, so the office
///    can act on the error and retry.
/// 3. **[`SessionEvent::Ended`] comes exactly once, last.** It's emitted only by
///    [`on_exit`](Self::on_exit), as that call's last output, carrying the [`ExitInfo`] it
///    was given. Nothing else emits it ([`on_stdout_closed`](Self::on_stdout_closed)
///    included). After it, every call returns no outputs (a repeated `on_exit` included),
///    [`next_deadline`](Self::next_deadline) is `None`, and `control` returns
///    [`ControlError::Closed`].
///
/// The `testkit` feature's `testkit` module checks all three against any session.
pub trait Session: Send {
    /// The process has started. Returns the protocol's opening (e.g. Claude's `initialize`
    /// request). Called once, before any other method.
    fn start(&mut self, now: Instant) -> Vec<Output>;

    /// One line of stdout, without its `\n`. Bad input (invalid JSON, an unknown message)
    /// becomes [`SessionEvent::ProtocolError`] or [`SessionEvent::Unknown`]; it never ends the
    /// session.
    fn on_line(&mut self, line: &[u8], now: Instant) -> Vec<Output>;

    /// The framer dropped a line longer than [`crate::MAX_LINE_BYTES`]; `len` is its length.
    /// Usually becomes [`SessionEvent::Truncated`].
    fn on_truncated(&mut self, len: usize, now: Instant) -> Vec<Output>;

    /// One line of stderr, without its `\n`. Runtimes write diagnostics there (Antigravity's
    /// `AGY_ERROR: {...}`), so a session may map one to an event (e.g. a
    /// [`SessionEvent::Notice`], or how a start or turn failed); most sessions ignore it.
    /// Stderr text is runtime text: an event carrying it gets redacted like any other.
    fn on_stderr_line(&mut self, line: &[u8], now: Instant) -> Vec<Output>;

    /// Stdout reached end of file. The session records it, and may report how the start or
    /// the current turn ended if it can tell. It never emits [`SessionEvent::Ended`]: the
    /// process may still be running (contract rule 3).
    fn on_stdout_closed(&mut self, now: Instant) -> Vec<Output>;

    /// The process was reaped. The session reports anything it still owes (e.g. a turn the
    /// exit cut short), then [`SessionEvent::Ended`] with `exit`, always last and only the
    /// first time (contract rule 3).
    fn on_exit(&mut self, exit: ExitInfo, now: Instant) -> Vec<Output>;

    /// Time passed. The session fires the protocol timeouts that are due (e.g. no reply to
    /// `initialize`, or no ack of an interrupt).
    fn on_tick(&mut self, now: Instant) -> Vec<Output>;

    /// An office command. On `Err` nothing was emitted and the session's state is unchanged
    /// (contract rule 2), so the office can act on the error (e.g. interrupt, then send).
    fn control(&mut self, control: Control, now: Instant) -> Result<Vec<Output>, ControlError>;

    /// When the session next needs [`on_tick`](Self::on_tick), if ever.
    fn next_deadline(&self) -> Option<Instant>;
}

/// Something the pump must do, in order.
///
/// **Exhaustive on purpose**, like [`crate::Frame`] and the plan's enums: it's an instruction
/// to the pump, which can't safely skip one it doesn't know, so a new variant is a breaking change
/// that fails the pump's build until it's handled. Match it without a wildcard arm.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Write this line to stdin, in full. Only [`crate::encode_line`] makes a [`Line`], so
    /// every write is one well-formed protocol message.
    Write(Line),
    /// Forward this event to the office.
    Event(SessionEvent),
    /// Close stdin. The process should exit on its own; the supervisor's stop plan covers it
    /// if it doesn't.
    CloseStdin,
}

/// A command from the office to a live session.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Control {
    /// Send the agent a message.
    ///
    /// Normal input is sent only between turns: while a turn runs the session answers
    /// [`ControlError::Busy`] and the office queues it ([`Session`] contract rule 1).
    /// `urgent` input steers the running turn where the runtime can
    /// ([`crate::Capabilities::steer`]); where it can't, the session answers
    /// [`ControlError::NeedsInterrupt`], and the office interrupts, then sends.
    Send {
        /// The message.
        text: String,
        /// Whether it should reach the agent during a running turn.
        urgent: bool,
    },
    /// Stop the running turn; the process lives on. While idle it's accepted and does
    /// nothing visible on runtimes where [`crate::Capabilities::idle_interrupt_ends_turn`]
    /// is false.
    Interrupt,
    /// Answer an approval the session raised with [`SessionEvent::ApprovalRequested`].
    Answer {
        /// The request being answered, exactly as the event carried it.
        request_id: ApprovalId,
        /// The answer.
        decision: Decision,
    },
    /// End the session gracefully: any protocol goodbye the runtime needs, then
    /// [`Output::CloseStdin`]. The supervisor's stop plan takes over from there.
    Close,
}

/// The answer to an approval request. It allows or denies the call **exactly as the runtime
/// asked for it**: there is no way to change the tool's input.
///
/// "Allow always" is the office's business (a rule added to its policy); the runtime only
/// ever hears [`Decision::AllowOnce`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decision {
    /// Allow this one call.
    AllowOnce,
    /// Deny the call.
    Deny {
        /// Told to the model where the runtime passes it on
        /// ([`crate::Capabilities::deny_message_reaches_model`]) and the turn continues.
        message: Option<String>,
        /// Also end the turn. On Claude this hides `message` from the model.
        stop_turn: bool,
    },
}

/// Why a session refused a [`Control`]. Nothing was emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ControlError {
    /// The session hasn't finished its opening yet ([`SessionEvent::Ready`] not emitted).
    NotReady,
    /// A turn is running: queue the input until it ends.
    Busy,
    /// Urgent input needs the turn interrupted first on this runtime.
    NeedsInterrupt,
    /// No pending approval has this id (never raised, already answered, or cancelled).
    UnknownApproval,
    /// The session is closing or has ended.
    Closed,
    /// This runtime can't do it (or doesn't know the command).
    Unsupported,
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ControlError::NotReady => "the session isn't ready yet",
            ControlError::Busy => "a turn is running",
            ControlError::NeedsInterrupt => "the running turn must be interrupted first",
            ControlError::UnknownApproval => "no pending approval has this id",
            ControlError::Closed => "the session is closed",
            ControlError::Unsupported => "not supported by this runtime",
        })
    }
}

impl std::error::Error for ControlError {}
