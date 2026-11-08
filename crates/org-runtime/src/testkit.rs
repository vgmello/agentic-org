//! Conformance checks for [`Session`] implementations (the `testkit` feature).
//!
//! The [`Session`] trait states three contract rules the office relies on. These checks
//! drive any session through them, using the adapter's own fixture lines (a [`Script`]), and
//! panic with a message naming the broken rule. Every adapter runs them in its tests:
//!
//! ```ignore
//! let script = Script {
//!     opening: vec![...],
//!     turn: vec![...],
//!     turn_end: vec![...],
//!     stderr: vec![...],
//! };
//! org_runtime::testkit::check_all(&mut || my_session(), &script, Instant::now());
//! ```
//!
//! `make` builds a fresh session in its initial state each time it's called (e.g. from
//! `plan_session` on a fixture spec); the checks need several, some of them side by side.

use std::time::{Duration, Instant};

use crate::{
    ApprovalId, Control, ControlError, Decision, ExitInfo, Output, Session, SessionEvent,
    MAX_LINE_BYTES,
};

/// Runtime lines that walk a session through one turn: what the runtime would write to stdout,
/// each without its `\n`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Script {
    /// Fed right after `start`: they finish the opening, so the session emits
    /// [`SessionEvent::Ready`].
    pub opening: Vec<Vec<u8>>,
    /// Fed after the checks' normal [`Control::Send`]: they leave the turn running (e.g. a
    /// turn-started message). Empty if the send alone starts the turn.
    pub turn: Vec<Vec<u8>>,
    /// Fed while the turn runs: they end it.
    pub turn_end: Vec<Vec<u8>>,
    /// Lines the runtime may write to **stderr** (e.g. its error format). Rule 2 feeds them
    /// while idle, rule 3 before each exit (during a turn too), so they may end a turn but
    /// must never end the session. May be empty.
    pub stderr: Vec<Vec<u8>>,
}

/// Runs every check.
pub fn check_all(make: &mut dyn FnMut() -> Box<dyn Session>, script: &Script, now: Instant) {
    check_busy_during_turn(make, script, now);
    check_error_changes_nothing(make, script, now);
    check_ended_once_and_last(make, script, now);
}

/// Rule 1: a normal [`Control::Send`] during a turn returns [`ControlError::Busy`], and is
/// accepted again once the turn ends.
pub fn check_busy_during_turn(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    now: Instant,
) {
    let mut session = make();
    open(session.as_mut(), script, now);
    into_turn(session.as_mut(), script, now);
    assert_eq!(
        session.control(send(false), now),
        Err(ControlError::Busy),
        "rule 1 broken: a normal Send during a turn must return Busy"
    );
    end_turn(session.as_mut(), script, now);
    assert!(
        session.control(send(false), now).is_ok(),
        "testkit: after the script's turn_end lines, a normal Send was still refused (the \
         script doesn't end the turn, or rule 2 is broken: the refused Send was queued)"
    );
}

/// Rule 2: a [`Session::control`] that returns `Err` changes nothing.
///
/// It runs two identical sessions side by side, gives only one of them each control that
/// fails, then feeds both the same input to the end. Their outputs and deadlines must stay
/// identical at every step, which they can't if the failed control changed anything they
/// act on later. It's a black-box check: a change that never shows on the script's inputs
/// (say, a stray entry in a table only an unknown id would reach) goes unseen.
pub fn check_error_changes_nothing(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    now: Instant,
) {
    let mut pair = Pair {
        a: make(),
        b: make(),
        now,
    };
    pair.both("start", &|s| s.start(now));
    pair.try_on_a("a Send while opening", send(false), false);
    pair.try_on_a("an Answer while opening", answer_never_raised(), true);
    for line in &script.opening {
        pair.both("an opening line", &|s| s.on_line(line, now));
    }
    pair.try_on_a(
        "an Answer for an approval never raised",
        answer_never_raised(),
        true,
    );
    for line in &script.stderr {
        pair.both("a stderr line", &|s| s.on_stderr_line(line, now));
    }
    pair.both("a Send while idle", &|s| {
        s.control(send(false), now)
            .unwrap_or_else(|e| panic!("testkit: a normal Send while idle was refused: {e}"))
    });
    for line in &script.turn {
        pair.both("a turn line", &|s| s.on_line(line, now));
    }
    pair.try_on_a("a normal Send during the turn", send(false), true);
    pair.try_on_a(
        "an Answer never raised, during the turn",
        answer_never_raised(),
        true,
    );
    pair.try_on_a("an urgent Send during the turn", send(true), false);
    for line in &script.turn_end {
        pair.both("a turn_end line", &|s| s.on_line(line, now));
    }
    // A refused Send must not have been queued: this one writes the same as on the twin.
    pair.both("a Send after the turn", &|s| {
        s.control(send(false), now).unwrap_or_else(|e| {
            panic!("testkit: after the script's turn_end lines, a normal Send was refused: {e}")
        })
    });
    let later = now + Duration::from_secs(3600);
    let latest = later + Duration::from_secs(3600);
    pair.both("a late tick", &|s| s.on_tick(later));
    pair.both("stdout closing", &|s| s.on_stdout_closed(later));
    pair.both("a tick after stdout closed", &|s| s.on_tick(latest));
    pair.both("the exit", &|s| s.on_exit(EXIT, latest));
}

/// Rule 3: [`SessionEvent::Ended`] comes only from [`Session::on_exit`], exactly once, as its
/// last output, carrying the exit; after it every call is silent, there's no deadline, and
/// every control returns [`ControlError::Closed`].
///
/// It checks exits at every stage: right after the start, while idle, during a turn, after
/// [`Control::Close`], each with stdout closed first and (where it matters) still open, and
/// each after a tick far past every deadline, the script's stderr lines and a truncated line,
/// and (once stdout closed) another tick far past that, none of which may end the session. Each exit carries a different [`ExitInfo`], which
/// `Ended` must carry back.
pub fn check_ended_once_and_last(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    now: Instant,
) {
    type Lead = fn(&mut dyn Session, &Script, Instant) -> Vec<Output>;
    let scenarios: [(&str, Lead, bool); 7] = [
        (
            "an exit right after the start",
            |s, _, now| s.start(now),
            true,
        ),
        (
            "an exit right after the start, stdout still open",
            |s, _, now| s.start(now),
            false,
        ),
        ("an exit while idle", open, true),
        (
            "an exit during a turn",
            |s, script, now| [open(s, script, now), into_turn(s, script, now)].concat(),
            true,
        ),
        (
            "an exit during a turn, stdout still open",
            |s, script, now| [open(s, script, now), into_turn(s, script, now)].concat(),
            false,
        ),
        (
            "an exit after the turn",
            |s, script, now| {
                [
                    open(s, script, now),
                    into_turn(s, script, now),
                    end_turn(s, script, now),
                ]
                .concat()
            },
            true,
        ),
        (
            "an exit after Close",
            |s, script, now| {
                let mut out = open(s, script, now);
                out.extend(
                    s.control(Control::Close, now)
                        .unwrap_or_else(|e| panic!("testkit: Close was refused: {e}")),
                );
                out
            },
            true,
        ),
    ];
    let late = now + Duration::from_secs(24 * 3600);
    let after_eof = late + Duration::from_secs(24 * 3600);
    for (i, (name, lead, stdout_closes_first)) in scenarios.into_iter().enumerate() {
        let exit = varied_exit(i);
        let mut session = make();
        let mut before = lead(session.as_mut(), script, now);
        // Things that happen before an exit and must not end the session themselves.
        for line in &script.stderr {
            before.extend(session.on_stderr_line(line, now));
        }
        before.extend(session.on_truncated(MAX_LINE_BYTES + 1, now));
        before.extend(session.on_tick(late));
        if stdout_closes_first {
            before.extend(session.on_stdout_closed(late));
            // A session must not end itself some time after EOF: only the exit ends it.
            before.extend(session.on_tick(after_eof));
        }
        assert!(
            !before.iter().any(is_ended),
            "rule 3 broken ({name}): Ended came before on_exit: {before:?}"
        );
        let out = session.on_exit(exit, after_eof);
        assert_eq!(
            out.iter().filter(|o| is_ended(o)).count(),
            1,
            "rule 3 broken ({name}): on_exit must emit Ended exactly once: {out:?}"
        );
        assert_eq!(
            out.last(),
            Some(&Output::Event(SessionEvent::Ended { exit })),
            "rule 3 broken ({name}): Ended, with the exit given, must be on_exit's last output"
        );
        assert_finished(session.as_mut(), script, name, after_eof);
    }
}

/// After Ended: every call is silent, no deadline, every control is refused as Closed.
fn assert_finished(session: &mut dyn Session, script: &Script, name: &str, now: Instant) {
    let later = now + Duration::from_secs(3600);
    let mut calls = vec![
        ("on_exit again", session.on_exit(varied_exit(1), later)),
        ("on_stdout_closed", session.on_stdout_closed(later)),
        ("on_truncated", session.on_truncated(10, later)),
        ("on_stderr_line", session.on_stderr_line(b"error", later)),
        ("on_tick", session.on_tick(later)),
        ("on_line (not JSON)", session.on_line(b"not json", later)),
    ];
    let lines = script
        .opening
        .iter()
        .chain(&script.turn)
        .chain(&script.turn_end);
    for line in lines {
        calls.push(("on_line (a script line)", session.on_line(line, later)));
    }
    for line in &script.stderr {
        calls.push((
            "on_stderr_line (a script line)",
            session.on_stderr_line(line, later),
        ));
    }
    for (what, out) in calls {
        assert!(
            out.is_empty(),
            "rule 3 broken ({name}): {what} after Ended emitted {out:?}"
        );
    }
    assert_eq!(
        session.next_deadline(),
        None,
        "rule 3 broken ({name}): a deadline after Ended"
    );
    for control in [
        send(false),
        send(true),
        Control::Interrupt,
        Control::Close,
        answer_never_raised(),
    ] {
        assert_eq!(
            session.control(control.clone(), later),
            Err(ControlError::Closed),
            "rule 3 broken ({name}): {control:?} after Ended must return Closed"
        );
    }
}

const EXIT: ExitInfo = ExitInfo {
    code: Some(0),
    signal: None,
};

/// A different exit per scenario, so a session can't pass by always reporting the same one.
fn varied_exit(i: usize) -> ExitInfo {
    match i % 4 {
        0 => EXIT,
        1 => ExitInfo {
            code: Some(3),
            signal: None,
        },
        2 => ExitInfo {
            code: None,
            signal: Some(15),
        },
        _ => ExitInfo {
            code: None,
            signal: None,
        },
    }
}

fn send(urgent: bool) -> Control {
    Control::Send {
        text: "org-runtime testkit".to_owned(),
        urgent,
    }
}

fn answer_never_raised() -> Control {
    Control::Answer {
        request_id: ApprovalId::new("org-runtime-testkit-never-raised"),
        decision: Decision::AllowOnce,
    }
}

fn is_ended(output: &Output) -> bool {
    matches!(output, Output::Event(SessionEvent::Ended { .. }))
}

/// Starts the session and feeds the opening; it must emit Ready.
fn open(session: &mut dyn Session, script: &Script, now: Instant) -> Vec<Output> {
    let mut out = session.start(now);
    for line in &script.opening {
        out.extend(session.on_line(line, now));
    }
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))),
        "testkit: the script's opening lines didn't make the session Ready: {out:?}"
    );
    out
}

/// Sends a normal message while idle and feeds the turn lines.
fn into_turn(session: &mut dyn Session, script: &Script, now: Instant) -> Vec<Output> {
    let mut out = session
        .control(send(false), now)
        .unwrap_or_else(|e| panic!("testkit: a normal Send while idle was refused: {e}"));
    for line in &script.turn {
        out.extend(session.on_line(line, now));
    }
    out
}

fn end_turn(session: &mut dyn Session, script: &Script, now: Instant) -> Vec<Output> {
    let mut out = Vec::new();
    for line in &script.turn_end {
        out.extend(session.on_line(line, now));
    }
    out
}

/// Two sessions in lockstep, for rule 2.
struct Pair {
    a: Box<dyn Session>,
    b: Box<dyn Session>,
    now: Instant,
}

impl Pair {
    /// The same step on both; they must answer alike.
    fn both(&mut self, what: &str, step: &dyn Fn(&mut dyn Session) -> Vec<Output>) {
        let a = step(self.a.as_mut());
        let b = step(self.b.as_mut());
        assert_eq!(
            a, b,
            "rule 2 broken: after a refused control, the session answered {what} differently"
        );
        self.same_deadline(what);
    }

    /// `control` on `a` alone if it fails there (the point of the check); if it succeeds, on
    /// `b` too, so they stay twins. `must_fail`: succeeding is itself a bug.
    fn try_on_a(&mut self, what: &str, control: Control, must_fail: bool) {
        match self.a.control(control.clone(), self.now) {
            Err(_) => self.same_deadline(what),
            Ok(out) => {
                assert!(
                    !must_fail,
                    "testkit: {what} must be refused, but was accepted: {out:?}"
                );
                let twin = self.b.control(control, self.now);
                assert_eq!(
                    Ok(out),
                    twin,
                    "rule 2 broken (or the session isn't deterministic): {what} was handled \
                     differently by the twin that never saw the earlier refused controls"
                );
                self.same_deadline(what);
            }
        }
    }

    fn same_deadline(&self, what: &str) {
        assert_eq!(
            self.a.next_deadline(),
            self.b.next_deadline(),
            "rule 2 broken: the deadline differs after {what}"
        );
    }
}
